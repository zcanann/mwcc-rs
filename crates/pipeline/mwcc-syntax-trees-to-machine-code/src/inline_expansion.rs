//! Semantics-preserving composition of retained inline function bodies.
//!
//! The frontend keeps skipped inline definitions out of object emission but
//! retains their parsed ASTs. This module owns the conservative subset that can
//! be spliced into a caller without changing argument evaluation: void bodies
//! with automatic scalar locals and no non-local control flow, called as
//! standalone statements with stable scalar arguments. Callee locals are
//! alpha-renamed and initialized at the call site rather than caller entry.

mod call_sites;
mod frame_residue;
mod ordinal_residue;
mod returns;
mod safety;
mod substitution;
mod value_body;
mod value_calls;

use call_sites::collect_function_calls;
use crate::inline_source_order::DefinitionOrder;
use mwcc_syntax_trees::{
    ArmBody, AsmItem, Expression, Function, InlineAsmBlock, Statement, SwitchArm, Type,
};
use returns::rewrite_inline_returns;
use safety::{
    automatic_composable_function, composable_function, materializable_arguments,
    parameter_requires_materialization, stable_argument, stable_arguments, stable_local_values,
    terminal_scalar_arguments,
};
use std::collections::{HashMap, HashSet};
use substitution::substitute_statement;
use value_body::ValueInlineBody;

#[derive(Clone, Debug, Default)]
pub struct InlineBodySet {
    /// Read-only ordinary definitions available to semantic transaction
    /// lowerers. Generic automatic inlining still uses the narrower `bodies`
    /// maps below; retaining this view does not make arbitrary functions
    /// composable.
    definitions: HashMap<String, Function>,
    /// Read-only skipped-inline definitions. Transaction lowerers may inspect
    /// these even when generic AST composition correctly rejects their control
    /// flow; lookup alone never makes them callable or composable.
    retained_definitions: HashMap<String, Function>,
    bodies: HashMap<String, Function>,
    /// Ordinary small definitions that MWCC may expand selectively at hot
    /// structured call sites even when the TU calls them more than once.
    repeatable_bodies: HashMap<String, Function>,
    values: HashMap<String, ValueInlineBody>,
    required: HashSet<String>,
    /// Retained pure inline-assembly helpers are already in their final
    /// instruction vocabulary. Keep them as call-site fragments instead of
    /// feeding their empty semantic-statement list through AST composition,
    /// which would erase the call and its assembly body.
    asm_fragments: HashMap<String, Vec<AsmItem>>,
    /// Whole-file IPA may erase a base vptr immediately overwritten by the
    /// derived constructor. Ordinary automatic inlining preserves each
    /// construction-phase installation because MWCC does.
    elide_overwritten_vptr_stores: bool,
}

pub(crate) fn legacy_frame_residue_bytes(
    function: &Function,
    facts: mwcc_syntax_trees::InlineExpansionFacts,
) -> usize {
    frame_residue::legacy_frame_residue_bytes(function, facts)
}

pub(crate) fn legacy_statement_body_frame_residue_bytes(
    function: &Function,
    substitutions: usize,
) -> usize {
    frame_residue::legacy_statement_body_frame_residue_bytes(function, substitutions)
}

pub(crate) fn ordinal_residue(
    facts: mwcc_syntax_trees::InlineExpansionFacts,
    statement_body_substitutions: usize,
    value_body_substitutions: usize,
    statement_body_weight: u8,
) -> u32 {
    ordinal_residue::ordinal_residue(
        facts,
        statement_body_substitutions,
        value_body_substitutions,
        statement_body_weight,
    )
}

pub(crate) struct ExpandedCalls {
    pub(crate) function: Function,
    pub(crate) statement_body_substitutions: usize,
    pub(crate) statement_frame_residue_substitutions: usize,
    pub(crate) value_body_substitutions: usize,
}

impl InlineBodySet {
    pub fn analyze(skipped: &[Function]) -> Self {
        Self::analyze_with_definitions(&[], skipped)
    }

    /// Analyze retained inline definitions plus ordinary definitions that the
    /// automatic inliner sees exactly once in this translation unit.  A
    /// one-call definition remains emitted when it has external linkage, but
    /// its body is also available for call-site composition.
    pub fn analyze_with_definitions(definitions: &[Function], skipped: &[Function]) -> Self {
        let retained_names: HashSet<&str> = skipped
            .iter()
            .map(|function| function.name.as_str())
            .collect();
        let callable_fallbacks: HashSet<&str> = definitions
            .iter()
            .map(|function| function.name.as_str())
            .filter(|name| retained_names.contains(name))
            .collect();
        let asm_fragments: HashMap<_, _> = skipped
            .iter()
            .filter_map(|function| {
                let [block] = function.inline_asm_blocks.as_slice() else {
                    return None;
                };
                (function.return_type == Type::Void
                    && function.parameters.is_empty()
                    && function.locals.is_empty()
                    && function.statements.is_empty()
                    && function.guards.is_empty()
                    && function.return_expression.is_none()
                    && function.asm_body.is_none()
                    && block.statement_index == 0)
                    .then(|| (function.name.clone(), block.items.clone()))
            })
            .collect();
        let required: HashSet<String> = skipped
            .iter()
            .filter(|function| !asm_fragments.contains_key(&function.name))
            .filter(|function| !callable_fallbacks.contains(function.name.as_str()))
            .map(|function| function.name.clone())
            .collect();
        let mut call_counts = HashMap::<String, usize>::new();
        let definition_order = DefinitionOrder::new(definitions);
        let mut source_visible_call_counts = HashMap::<String, usize>::new();
        // Ordinary same-TU definitions are selected for automatic inlining by
        // their calls from emitted definitions. An unreferenced retained inline
        // body is compiled for analysis but dropped; a call written only inside
        // it must not turn the one live call into a repeatable/multi-use case.
        for (caller_index, function) in definitions.iter().enumerate() {
            let mut calls = HashMap::new();
            collect_function_calls(function, &mut calls);
            for (callee, count) in calls {
                *call_counts.entry(callee.clone()).or_default() += count;
                if definition_order.is_visible_to(&callee, caller_index) {
                    *source_visible_call_counts.entry(callee).or_default() += count;
                }
            }
        }
        let mut bodies = HashMap::new();
        for function in skipped {
            if asm_fragments.contains_key(&function.name)
                || callable_fallbacks.contains(function.name.as_str())
            {
                continue;
            }
            let materialized = materialize_embedded_asm_statements(function);
            let function = materialized.as_ref().unwrap_or(function);
            if composable_function(function) {
                bodies.insert(function.name.clone(), function.clone());
            }
        }
        for function in definitions.iter().filter(|function| {
            automatic_composable_function(function)
                && call_counts.get(&function.name).copied() == Some(1)
                && source_visible_call_counts
                    .get(&function.name)
                    .copied()
                    == Some(1)
        }) {
            bodies
                .entry(function.name.clone())
                .or_insert_with(|| function.clone());
        }
        let repeatable_bodies = definitions
            .iter()
            .filter(|function| {
                automatic_composable_function(function)
                    && call_counts.get(&function.name).copied().unwrap_or(0) > 1
                    && source_visible_call_counts
                        .get(&function.name)
                        .copied()
                        == call_counts.get(&function.name).copied()
            })
            .map(|function| (function.name.clone(), function.clone()))
            .collect();
        let mut values: HashMap<_, _> = skipped
            .iter()
            .filter(|function| !asm_fragments.contains_key(&function.name))
            .filter(|function| !callable_fallbacks.contains(function.name.as_str()))
            .filter_map(|function| {
                value_body::summarize(function).map(|body| (function.name.clone(), body))
            })
            .collect();
        for function in definitions
            .iter()
            .filter(|function| !callable_fallbacks.contains(function.name.as_str()))
            .filter(|function| {
                source_visible_call_counts
                    .get(&function.name)
                    .copied()
                    .unwrap_or(0)
                    == call_counts.get(&function.name).copied().unwrap_or(0)
            })
        {
            if let Some(body) = value_body::summarize_automatic(function) {
                values.entry(function.name.clone()).or_insert(body);
            } else if call_counts.get(&function.name).copied() == Some(1) {
                if let Some(body) = value_body::summarize_automatic_void_forward(function) {
                    values.entry(function.name.clone()).or_insert(body);
                }
            }
        }
        if let Some(needle) = std::env::var_os("MWCC_CAPTURE_INLINE") {
            let needle = needle.to_string_lossy();
            for function in definitions
                .iter()
                .filter(|function| function.name.contains(needle.as_ref()))
            {
                eprintln!(
                    "automatic inline summary {}: eligible={} calls={:?} parameters={} locals={} statements={}",
                    function.name,
                    automatic_composable_function(function),
                    call_counts.get(&function.name),
                    function.parameters.len(),
                    function.locals.len(),
                    function.statements.len(),
                );
            }
            for function in skipped
                .iter()
                .filter(|function| function.name.contains(needle.as_ref()))
            {
                eprintln!(
                    "inline summary {}: statement={} value={} parameters={} locals={} statements={} return={:?}",
                    function.name,
                    bodies.contains_key(&function.name),
                    values.contains_key(&function.name),
                    function.parameters.len(),
                    function.locals.len(),
                    function.statements.len(),
                    function.return_expression,
                );
            }
        }
        Self {
            definitions: definitions
                .iter()
                .map(|function| (function.name.clone(), function.clone()))
                .collect(),
            retained_definitions: skipped
                .iter()
                .map(|function| (function.name.clone(), function.clone()))
                .collect(),
            bodies,
            repeatable_bodies,
            values,
            required,
            asm_fragments,
            elide_overwritten_vptr_stores: false,
        }
    }

    /// Find retained-inline bodies reached beyond MWCC's nested automatic
    /// inlining budget. Each returned name is grouped after the emitted
    /// definition whose expansion first needs its weak callable fallback.
    pub fn depth_limited_fallbacks(
        definitions: &[Function],
        skipped: &[Function],
        maximum_depth: usize,
    ) -> Vec<Vec<String>> {
        let bodies: HashMap<&str, &Function> = skipped
            .iter()
            .map(|function| (function.name.as_str(), function))
            .collect();
        let mut emitted: HashSet<String> = definitions
            .iter()
            .map(|function| function.name.clone())
            .collect();
        definitions
            .iter()
            .map(|definition| {
                let mut group = Vec::new();
                collect_depth_limited_fallbacks(
                    definition,
                    0,
                    maximum_depth,
                    &bodies,
                    &mut emitted,
                    &mut HashSet::new(),
                    &mut group,
                );
                group
            })
            .collect()
    }

    pub fn with_overwritten_vptr_elision(mut self, enabled: bool) -> Self {
        self.elide_overwritten_vptr_stores = enabled;
        self
    }

    /// A zero-argument, void retained inline-assembly helper that can be
    /// assembled directly at its call site.
    pub(crate) fn asm_fragment(&self, name: &str) -> Option<&[AsmItem]> {
        self.asm_fragments.get(name).map(Vec::as_slice)
    }

    /// The retained source body for an ordinary automatic-inline candidate.
    ///
    /// Whole-transaction lowerers use this read-only view to validate the
    /// helper semantics they are about to compose. Keeping the body lookup here
    /// avoids exposing the inliner's storage policy or making those lowerers
    /// guess from a callee name alone.
    pub(crate) fn composable_body(&self, name: &str) -> Option<&Function> {
        self.bodies
            .get(name)
            .or_else(|| self.repeatable_bodies.get(name))
    }

    /// An ordinary same-translation-unit definition, exposed only for
    /// lowerers that validate the complete callee shape before composing it.
    pub(crate) fn definition_body(&self, name: &str) -> Option<&Function> {
        self.definitions.get(name)
    }

    /// A skipped inline's retained semantic body, including definitions whose
    /// control flow lies outside the conservative generic composer.
    pub(crate) fn retained_body(&self, name: &str) -> Option<&Function> {
        self.retained_definitions.get(name)
    }

    /// Whether this function calls a definition that cannot be materialized as
    /// an ordinary callable symbol. Optional one-call auto-inline candidates
    /// are deliberately excluded: if composition declines, they remain calls.
    pub(crate) fn calls_required(&self, function: &Function) -> bool {
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        calls.keys().any(|name| self.required.contains(name))
    }

    /// Whether a function references a retained body by its canonical AST
    /// identity. This supplements the frontend's legacy skipped-name set,
    /// whose C++ entries may still use an unmangled spelling.
    pub(crate) fn calls_any(&self, function: &Function) -> bool {
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        calls
            .keys()
            .any(|name| self.bodies.contains_key(name) || self.values.contains_key(name))
            || function
                .statements
                .iter()
                .any(|statement| self.contains_call(statement))
    }

    /// Expand a constructor call embedded in scalar `new` without inventing a
    /// caller-visible AST local.
    ///
    /// `ConstructedNew` owns allocation and the null guard in instruction
    /// selection, so it cannot be rewritten as an ordinary source call.  Its
    /// retained inline constructor body can still use the same recursive value
    /// composition as every other inline expression once the allocator result
    /// has a temporary variable identity.  Decline bodies that need hygienic
    /// locals; frame allocation for those belongs in a later, explicit model.
    pub(crate) fn expand_constructed_new_body(
        &self,
        constructor: &str,
        result_name: &str,
        arguments: &[Expression],
    ) -> Option<Expression> {
        let mut call_arguments = Vec::with_capacity(arguments.len() + 1);
        call_arguments.push(Expression::Variable(result_name.to_owned()));
        call_arguments.extend_from_slice(arguments);
        let call = Expression::Call {
            name: constructor.to_owned(),
            arguments: call_arguments,
        };
        let mut locals = Vec::new();
        let mut occupied_names = HashSet::from([result_name.to_owned()]);
        let mut next_local_id = 0;
        let mut allocator = value_calls::LocalAllocator {
            locals: &mut locals,
            occupied_names: &mut occupied_names,
            next_local_id: &mut next_local_id,
        };
        let mut active = HashSet::new();
        let stable_variables = HashSet::from([result_name.to_owned()]);
        let mut changed = false;
        let mut substitutions = 0;
        let expanded = value_calls::expand_expression(
            &call,
            &self.values,
            &stable_variables,
            &mut active,
            &mut changed,
            &mut substitutions,
            &mut allocator,
        );
        if !changed || !locals.is_empty() || self.expression_contains_call(&expanded) {
            return None;
        }
        Some(expanded)
    }

    /// Expand every composable retained-inline call in `function`.
    ///
    /// Returning `None` means either nothing was expanded or at least one call
    /// to a retained composable body remained in a context this subset cannot
    /// preserve. The caller must then keep the ordinary safe deferral.
    pub(crate) fn expand_calls(&self, function: &Function) -> Option<Function> {
        self.expand_calls_with_facts(function)
            .map(|expanded| expanded.function)
    }

    /// Expand a repeatable small definition only when its call is directly in
    /// the sole loop body. This models MWCC's loop-site inlining decision
    /// without making a multi-use helper expand indiscriminately at every call
    /// site in the translation unit.
    pub(crate) fn expand_repeatable_loop_calls(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        let [Statement::Loop { body, .. }] = function.statements.as_slice() else {
            return None;
        };
        let eligible = body.iter().any(|statement| {
            matches!(statement,
                Statement::Expression(Expression::Call { name, .. })
                    if self.repeatable_bodies.contains_key(name))
        });
        if !eligible {
            return None;
        }
        let mut expanded = self.clone();
        expanded.bodies.extend(self.repeatable_bodies.clone());
        expanded.expand_calls_with_facts_policy(function, true)
    }

    pub(crate) fn expand_calls_with_facts(&self, function: &Function) -> Option<ExpandedCalls> {
        self.expand_calls_with_facts_policy(function, false)
    }

    fn expand_calls_with_facts_policy(
        &self,
        function: &Function,
        allow_changing_scalar_arguments: bool,
    ) -> Option<ExpandedCalls> {
        let mut changed = false;
        let mut statement_body_substitutions = 0;
        let mut statement_frame_residue_substitutions = 0;
        let mut value_body_substitutions = 0;
        let mut active = HashSet::new();
        let stable_variables = stable_local_values(function);
        let mut locals = function.locals.clone();
        let mut occupied_names: HashSet<String> = function
            .parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .chain(function.locals.iter().map(|local| local.name.clone()))
            .collect();
        let mut next_local_id = 0usize;
        let statements = self.expand_statements(
            &function.statements,
            &stable_variables,
            &mut active,
            &mut changed,
            &mut locals,
            &mut occupied_names,
            &mut next_local_id,
            &mut statement_body_substitutions,
            &mut statement_frame_residue_substitutions,
            function.return_expression.is_none(),
            allow_changing_scalar_arguments,
        );
        let initializers: Vec<_> = locals
            .iter()
            .enumerate()
            .filter_map(|(index, local)| local.initializer.clone().map(|value| (index, value)))
            .collect();
        let mut allocator = value_calls::LocalAllocator {
            locals: &mut locals,
            occupied_names: &mut occupied_names,
            next_local_id: &mut next_local_id,
        };
        let statements: Vec<_> = statements
            .iter()
            .map(|statement| {
                value_calls::expand_statement(
                    statement,
                    &self.values,
                    &stable_variables,
                    &mut active,
                    &mut changed,
                    &mut value_body_substitutions,
                    &mut allocator,
                )
            })
            .collect();
        for (index, initializer) in initializers {
            let initializer = value_calls::expand_expression(
                &initializer,
                &self.values,
                &stable_variables,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
                &mut allocator,
            );
            allocator.locals[index].initializer = Some(initializer);
        }
        let mut expanded = function.clone();
        for guard in &mut expanded.guards {
            guard.condition = value_calls::expand_expression(
                &guard.condition,
                &self.values,
                &stable_variables,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
                &mut allocator,
            );
            guard.value = value_calls::expand_expression(
                &guard.value,
                &self.values,
                &stable_variables,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
                &mut allocator,
            );
        }
        if let Some(return_expression) = &expanded.return_expression {
            expanded.return_expression = Some(value_calls::expand_expression(
                return_expression,
                &self.values,
                &stable_variables,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
                &mut allocator,
            ));
        }
        drop(allocator);
        expanded.locals = locals;
        expanded.statements = if self.elide_overwritten_vptr_stores {
            remove_overwritten_vptr_stores(statements)
        } else {
            statements
        };
        let calls_remain = self.calls_required(&expanded);
        if calls_remain
            && std::env::var_os("MWCC_CAPTURE_FUNCTION")
                .is_some_and(|name| name == std::ffi::OsStr::new(&function.name))
        {
            let mut calls = HashMap::new();
            collect_function_calls(&expanded, &mut calls);
            let mut retained = calls
                .into_keys()
                .filter(|name| self.bodies.contains_key(name) || self.values.contains_key(name))
                .collect::<Vec<_>>();
            retained.sort();
            eprintln!("unexpanded retained inline calls: {}", retained.join(", "));
        }
        if !changed || calls_remain {
            return None;
        }
        Some(ExpandedCalls {
            function: expanded,
            statement_body_substitutions,
            statement_frame_residue_substitutions,
            value_body_substitutions,
        })
    }

    fn expand_statements(
        &self,
        statements: &[Statement],
        stable_variables: &HashSet<String>,
        active: &mut HashSet<String>,
        changed: &mut bool,
        locals: &mut Vec<mwcc_syntax_trees::LocalDeclaration>,
        occupied_names: &mut HashSet<String>,
        next_local_id: &mut usize,
        statement_body_substitutions: &mut usize,
        statement_frame_residue_substitutions: &mut usize,
        allow_terminal_local_reuse: bool,
        allow_changing_scalar_arguments: bool,
    ) -> Vec<Statement> {
        let mut output = Vec::new();
        for (statement_index, statement) in statements.iter().enumerate() {
            match statement {
                Statement::Expression(Expression::Call { name, arguments })
                    if self.bodies.contains_key(name)
                        && !active.contains(name)
                        && active.len() < 2
                        && (stable_arguments(
                            &self.bodies[name],
                            arguments,
                            stable_variables,
                        ) || materializable_arguments(
                            &self.bodies[name],
                            arguments,
                            stable_variables,
                            allow_changing_scalar_arguments,
                        ) || (allow_terminal_local_reuse
                            && statement_index + 1 == statements.len()
                            && terminal_scalar_arguments(
                                &self.bodies[name],
                                arguments,
                                stable_variables,
                            ))) =>
                {
                    let callee = &self.bodies[name];
                    if callee.parameters.len() != arguments.len() {
                        output.push(statement.clone());
                        continue;
                    }
                    let callee_stable = stable_local_values(callee);
                    let mut nested_stable_variables = stable_variables.clone();
                    let terminal_direct = allow_terminal_local_reuse
                        && statement_index + 1 == statements.len()
                        && terminal_scalar_arguments(callee, arguments, stable_variables);
                    let materialize = !terminal_direct
                        && !stable_arguments(callee, arguments, stable_variables);
                    let mut replacements = HashMap::new();
                    let mut substituted = Vec::new();
                    for (parameter, argument) in callee.parameters.iter().zip(arguments) {
                        let parameter_is_mutable =
                            parameter_requires_materialization(callee, &parameter.name);
                        if (!parameter_is_mutable || terminal_direct)
                            && (!materialize || stable_argument(argument, stable_variables))
                        {
                            replacements.insert(parameter.name.clone(), argument.clone());
                            continue;
                        }
                        let unique_name = loop {
                            let candidate = format!(
                                "__mwcc_inline_{}_{}_{}",
                                name, *next_local_id, parameter.name
                            );
                            *next_local_id += 1;
                            if occupied_names.insert(candidate.clone()) {
                                break candidate;
                            }
                        };
                        replacements.insert(
                            parameter.name.clone(),
                            Expression::Variable(unique_name.clone()),
                        );
                        nested_stable_variables.insert(unique_name.clone());
                        locals.push(mwcc_syntax_trees::LocalDeclaration {
                            declared_type: parameter.parameter_type,
                            name: unique_name.clone(),
                            initializer: None,
                            is_volatile: false,
                            array_length: None,
                            is_static: false,
                            data_bytes: None,
                            data_relocations: Vec::new(),
                            is_const: false,
                            row_bytes: None,
                        });
                        substituted.push(Statement::Assign {
                            name: unique_name,
                            value: argument.clone(),
                        });
                    }
                    for local in &callee.locals {
                        let unique_name = loop {
                            let candidate =
                                format!("__mwcc_inline_{}_{}_{}", name, *next_local_id, local.name);
                            *next_local_id += 1;
                            if occupied_names.insert(candidate.clone()) {
                                break candidate;
                            }
                        };
                        replacements.insert(
                            local.name.clone(),
                            Expression::Variable(unique_name.clone()),
                        );
                        if callee_stable.contains(&local.name) {
                            nested_stable_variables.insert(unique_name.clone());
                        }
                        let mut declaration = local.clone();
                        declaration.name = unique_name;
                        declaration.initializer = None;
                        locals.push(declaration);
                    }
                    substituted.extend(callee.locals.iter().filter_map(|local| {
                        local.initializer.as_ref().map(|initializer| {
                            substitute_statement(
                                &Statement::Assign {
                                    name: local.name.clone(),
                                    value: initializer.clone(),
                                },
                                &replacements,
                            )
                        })
                    }));
                    substituted.extend(
                        callee
                            .statements
                            .iter()
                            .map(|statement| substitute_statement(statement, &replacements)),
                    );
                    substituted = fold_constant_inline_branches(substituted);
                    // A return exits the callee instance, not its caller.  Give
                    // every expansion a private forward boundary so nested
                    // control flow preserves that distinction through the
                    // shared structured-body lowering path.
                    let return_boundary =
                        format!("__mwcc_inline_return_{}_{}", name, *next_local_id);
                    *next_local_id += 1;
                    if rewrite_inline_returns(&mut substituted, &return_boundary) {
                        substituted.push(Statement::Label(return_boundary));
                    }
                    *changed = true;
                    *statement_body_substitutions += 1;
                    let mut callee_calls = HashMap::new();
                    collect_function_calls(callee, &mut callee_calls);
                    if !callee_calls.is_empty() && self.required.contains(name) {
                        *statement_frame_residue_substitutions += 1;
                    }
                    active.insert(name.clone());
                    output.extend(self.expand_statements(
                        &substituted,
                        &nested_stable_variables,
                        active,
                        changed,
                        locals,
                        occupied_names,
                        next_local_id,
                        statement_body_substitutions,
                        statement_frame_residue_substitutions,
                        false,
                        allow_changing_scalar_arguments,
                    ));
                    active.remove(name);
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => output.push(Statement::If {
                    condition: condition.clone(),
                    then_body: self.expand_statements(
                        then_body,
                        stable_variables,
                        active,
                        changed,
                        locals,
                        occupied_names,
                        next_local_id,
                        statement_body_substitutions,
                        statement_frame_residue_substitutions,
                        false,
                        allow_changing_scalar_arguments,
                    ),
                    else_body: self.expand_statements(
                        else_body,
                        stable_variables,
                        active,
                        changed,
                        locals,
                        occupied_names,
                        next_local_id,
                        statement_body_substitutions,
                        statement_frame_residue_substitutions,
                        false,
                        allow_changing_scalar_arguments,
                    ),
                }),
                Statement::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => output.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body: self.expand_statements(
                        body,
                        stable_variables,
                        active,
                        changed,
                        locals,
                        occupied_names,
                        next_local_id,
                        statement_body_substitutions,
                        statement_frame_residue_substitutions,
                        false,
                        allow_changing_scalar_arguments,
                    ),
                }),
                Statement::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    let mut expanded_arms = Vec::with_capacity(arms.len());
                    for arm in arms {
                        let body = match &arm.body {
                            ArmBody::Return(value) => ArmBody::Return(value.clone()),
                            ArmBody::Statements(body) => ArmBody::Statements(
                                self.expand_statements(
                                    body,
                                    stable_variables,
                                    active,
                                    changed,
                                    locals,
                                    occupied_names,
                                    next_local_id,
                                    statement_body_substitutions,
                                    statement_frame_residue_substitutions,
                                    false,
                                    allow_changing_scalar_arguments,
                                ),
                            ),
                        };
                        expanded_arms.push(SwitchArm {
                            value: arm.value,
                            body,
                            falls_through: arm.falls_through,
                        });
                    }
                    let expanded_default = default.as_ref().map(|body| match body {
                        ArmBody::Return(value) => ArmBody::Return(value.clone()),
                        ArmBody::Statements(body) => ArmBody::Statements(
                            self.expand_statements(
                                body,
                                stable_variables,
                                active,
                                changed,
                                locals,
                                occupied_names,
                                next_local_id,
                                statement_body_substitutions,
                                statement_frame_residue_substitutions,
                                false,
                                allow_changing_scalar_arguments,
                            ),
                        ),
                    });
                    output.push(Statement::Switch {
                        scrutinee: scrutinee.clone(),
                        arms: expanded_arms,
                        default: expanded_default,
                    });
                }
                _ => output.push(statement.clone()),
            }
        }
        output
    }

    fn contains_call(&self, statement: &Statement) -> bool {
        match statement {
            Statement::Store { target, value } => {
                self.expression_contains_call(target) || self.expression_contains_call(value)
            }
            Statement::Assign { value, .. } => self.expression_contains_call(value),
            Statement::Expression(expression) => self.expression_contains_call(expression),
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                self.expression_contains_call(condition)
                    || then_body
                        .iter()
                        .any(|statement| self.contains_call(statement))
                    || else_body
                        .iter()
                        .any(|statement| self.contains_call(statement))
            }
            Statement::Return(expression) => expression
                .as_ref()
                .is_some_and(|expression| self.expression_contains_call(expression)),
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                self.expression_contains_call(scrutinee)
                    || arms.iter().any(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Return(expression) => {
                            self.expression_contains_call(expression)
                        }
                        mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                            .iter()
                            .any(|statement| self.contains_call(statement)),
                    })
                    || default.as_ref().is_some_and(|body| match body {
                        mwcc_syntax_trees::ArmBody::Return(expression) => {
                            self.expression_contains_call(expression)
                        }
                        mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                            .iter()
                            .any(|statement| self.contains_call(statement)),
                    })
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                initializer
                    .as_ref()
                    .is_some_and(|expression| self.expression_contains_call(expression))
                    || condition
                        .as_ref()
                        .is_some_and(|expression| self.expression_contains_call(expression))
                    || step
                        .as_ref()
                        .is_some_and(|expression| self.expression_contains_call(expression))
                    || body.iter().any(|statement| self.contains_call(statement))
            }
            Statement::InlineAsm(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => false,
        }
    }

    fn expression_contains_call(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Call { name, arguments } => {
                self.bodies.contains_key(name)
                    || self.values.contains_key(name)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_contains_call(argument))
            }
            Expression::Binary { left, right, .. }
            | Expression::Assign {
                target: left,
                value: right,
            }
            | Expression::Comma { left, right } => {
                self.expression_contains_call(left) || self.expression_contains_call(right)
            }
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                ..
            } => {
                self.expression_contains_call(condition)
                    || self.expression_contains_call(when_true)
                    || self.expression_contains_call(when_false)
            }
            Expression::Unary { operand, .. }
            | Expression::Cast { operand, .. }
            | Expression::BitFieldRead {
                extracted: operand, ..
            }
            | Expression::IndexedUpdateValue { value: operand }
            | Expression::Dereference { pointer: operand }
            | Expression::AddressOf { operand }
            | Expression::PostStep {
                target: operand, ..
            } => self.expression_contains_call(operand),
            Expression::Index { base, index } => {
                self.expression_contains_call(base) || self.expression_contains_call(index)
            }
            Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
                self.expression_contains_call(base)
            }
            Expression::CallThrough { target, arguments } => {
                self.expression_contains_call(target)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_contains_call(argument))
            }
            Expression::VirtualCall {
                object, arguments, ..
            } => {
                self.expression_contains_call(object)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_contains_call(argument))
            }
            Expression::ConstructedNew {
                allocation,
                arguments,
                ..
            } => {
                self.expression_contains_call(allocation)
                    || arguments
                        .iter()
                        .any(|argument| self.expression_contains_call(argument))
            }
            Expression::AggregateLiteral(elements) => elements
                .iter()
                .any(|element| self.expression_contains_call(element)),
            Expression::IntegerLiteral(_)
            | Expression::FloatLiteral(_)
            | Expression::StringLiteral(_)
            | Expression::Variable(_)
            | Expression::CompoundLiteral { .. } => false,
        }
    }
}

/// Move top-level embedded assembly blocks into the ordered statement tree used
/// by retained-inline composition. A zero-argument inline helper can then be
/// spliced into an arbitrarily nested caller arm without losing the assembly's
/// semantic position. Parameterized blocks remain outside this subset because
/// their symbolic operands require C-local register binding, not textual
/// substitution.
fn materialize_embedded_asm_statements(function: &Function) -> Option<Function> {
    if function.inline_asm_blocks.is_empty() {
        return None;
    }
    if !function.parameters.is_empty() || !function.locals.is_empty() {
        return None;
    }
    if function
        .inline_asm_blocks
        .iter()
        .any(|block| block.statement_index > function.statements.len())
    {
        return None;
    }

    let mut blocks: Vec<&InlineAsmBlock> = function.inline_asm_blocks.iter().collect();
    blocks.sort_by_key(|block| block.statement_index);
    let mut statements =
        Vec::with_capacity(function.statements.len() + function.inline_asm_blocks.len());
    let mut cursor = 0;
    for index in 0..=function.statements.len() {
        while blocks
            .get(cursor)
            .is_some_and(|block| block.statement_index == index)
        {
            statements.push(Statement::InlineAsm(blocks[cursor].items.clone()));
            cursor += 1;
        }
        if let Some(statement) = function.statements.get(index) {
            statements.push(statement.clone());
        }
    }

    Some(Function {
        statements,
        inline_asm_blocks: Vec::new(),
        ..function.clone()
    })
}

/// Parameter substitution can turn a callee guard into a compile-time branch
/// (`base::~base(this, 0)` makes its deleting guard `0 > 0`). Eliminate that
/// dead path before structured lowering sees an expression with no register.
fn fold_constant_inline_branches(statements: Vec<Statement>) -> Vec<Statement> {
    let mut output = Vec::new();
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                if let Some(value) = constant_inline_condition(&condition) {
                    let selected = if value { then_body } else { else_body };
                    output.extend(fold_constant_inline_branches(selected));
                } else {
                    output.push(Statement::If {
                        condition,
                        then_body: fold_constant_inline_branches(then_body),
                        else_body: fold_constant_inline_branches(else_body),
                    });
                }
            }
            statement => output.push(statement),
        }
    }
    output
}

fn constant_inline_condition(condition: &Expression) -> Option<bool> {
    if let Some(value) = crate::analysis::constant_value(condition) {
        return Some(value != 0);
    }
    let Expression::Binary {
        operator,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let left = crate::analysis::constant_value(left)?;
    let right = crate::analysis::constant_value(right)?;
    use mwcc_syntax_trees::BinaryOperator;
    Some(match operator {
        BinaryOperator::Equal => left == right,
        BinaryOperator::NotEqual => left != right,
        BinaryOperator::Less => left < right,
        BinaryOperator::LessEqual => left <= right,
        BinaryOperator::Greater => left > right,
        BinaryOperator::GreaterEqual => left >= right,
        BinaryOperator::LogicalAnd => left != 0 && right != 0,
        BinaryOperator::LogicalOr => left != 0 || right != 0,
        _ => return None,
    })
}

/// Remove a base-constructor vptr installation when an inlined derived
/// constructor immediately overwrites the same slot. Restrict this to adjacent
/// compiler-synthesized vtable-address stores: neither right-hand side can have
/// side effects, and no intervening statement can observe the base value.
fn remove_overwritten_vptr_stores(statements: Vec<Statement>) -> Vec<Statement> {
    fn target(statement: &Statement) -> Option<(&str, u32)> {
        let Statement::Store {
            target:
                Expression::Member {
                    base,
                    offset,
                    index_stride: None,
                    ..
                },
            value: Expression::AddressOf { operand },
        } = statement
        else {
            return None;
        };
        let Expression::Variable(base) = base.as_ref() else {
            return None;
        };
        let Expression::Variable(vtable) = operand.as_ref() else {
            return None;
        };
        vtable.starts_with("__vt__").then_some((base, *offset))
    }

    let mut output = Vec::with_capacity(statements.len());
    for statement in statements {
        if let (Some(previous), Some(current)) =
            (output.last().and_then(target), target(&statement))
        {
            if previous == current {
                output.pop();
            }
        }
        output.push(statement);
    }
    output
}

fn collect_depth_limited_fallbacks(
    function: &Function,
    depth: usize,
    maximum_depth: usize,
    bodies: &HashMap<&str, &Function>,
    emitted: &mut HashSet<String>,
    active: &mut HashSet<String>,
    output: &mut Vec<String>,
) {
    let mut calls = HashMap::new();
    collect_function_calls(function, &mut calls);
    let mut names = calls.into_keys().collect::<Vec<_>>();
    names.sort();
    for name in names {
        let Some(body) = bodies.get(name.as_str()).copied() else {
            continue;
        };
        if depth >= maximum_depth {
            if emitted.insert(name.clone()) {
                output.push(name.clone());
                // A materialized fallback is a fresh compilation root. Its
                // own automatic-inlining budget starts over.
                collect_depth_limited_fallbacks(
                    body,
                    0,
                    maximum_depth,
                    bodies,
                    emitted,
                    &mut HashSet::new(),
                    output,
                );
            }
            continue;
        }
        if active.insert(name.clone()) {
            collect_depth_limited_fallbacks(
                body,
                depth + 1,
                maximum_depth,
                bodies,
                emitted,
                active,
                output,
            );
            active.remove(&name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{
        AsmInstruction, AsmItem, AsmOperand, BinaryOperator, InlineAsmBlock,
        LocalDeclaration, LoopKind, Parameter, Pointee, Type,
    };

    fn function(name: &str, parameters: Vec<Parameter>, statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: name.to_owned(),
            is_static: true,
            is_weak: false,
            parameters,
            locals: Vec::new(),
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn local(name: &str, declared_type: Type, initializer: Expression) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: Some(initializer),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    #[test]
    fn elides_an_overwritten_base_vptr_only_under_whole_file_ipa() {
        fn vptr_store(vtable: &str) -> Statement {
            Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".into())),
                    offset: 0,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: Expression::AddressOf {
                    operand: Box::new(Expression::Variable(vtable.into())),
                },
            }
        }

        let object_parameter = Parameter {
            parameter_type: Type::StructPointer { element_size: 8 },
            name: "this".into(),
        };
        let base = function(
            "base_constructor",
            vec![object_parameter.clone()],
            vec![vptr_store("__vt__4Base")],
        );
        let derived = function(
            "derived_constructor",
            vec![object_parameter],
            vec![
                Statement::Expression(Expression::Call {
                    name: "base_constructor".into(),
                    arguments: vec![Expression::Variable("this".into())],
                }),
                vptr_store("__vt__7Derived"),
            ],
        );

        let retained = InlineBodySet::analyze(&[base.clone(), derived.clone()])
            .expand_calls(&derived)
            .expect("the trivial base constructor should inline");
        assert_eq!(
            retained
                .statements
                .iter()
                .filter(|statement| matches!(statement, Statement::Store { .. }))
                .count(),
            2,
            "ordinary automatic inlining preserves construction-phase vptrs"
        );

        let expanded = InlineBodySet::analyze(&[base, derived.clone()])
            .with_overwritten_vptr_elision(true)
            .expand_calls(&derived)
            .expect("the trivial base constructor should inline");
        assert!(matches!(
            expanded.statements.as_slice(),
            [Statement::Store {
                value: Expression::AddressOf { operand },
                ..
            }] if matches!(operand.as_ref(), Expression::Variable(name) if name == "__vt__7Derived")
        ));
    }

    #[test]
    fn retained_pure_inline_asm_is_a_call_site_fragment_not_an_empty_value_body() {
        let mut helper = function("configure", Vec::new(), Vec::new());
        helper.inline_asm_blocks = vec![mwcc_syntax_trees::InlineAsmBlock {
            statement_index: 0,
            items: vec![AsmItem::Instruction(mwcc_syntax_trees::AsmInstruction {
                mnemonic: "li".into(),
                operands: vec![
                    mwcc_syntax_trees::AsmOperand::Gpr(3),
                    mwcc_syntax_trees::AsmOperand::Immediate(4),
                ],
                source_line: 1,
            })],
        }];
        let bodies = InlineBodySet::analyze(&[helper]);
        assert!(bodies.asm_fragment("configure").is_some());

        let caller = function(
            "caller",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "configure".into(),
                arguments: Vec::new(),
            })],
        );
        assert!(!bodies.calls_required(&caller));
        assert!(!bodies.calls_any(&caller));
        assert!(bodies.expand_calls(&caller).is_none());
    }

    #[test]
    fn frame_residue_counts_only_call_bearing_statement_bodies() {
        let leaf = function(
            "leaf",
            Vec::new(),
            vec![Statement::Store {
                target: Expression::Variable("memory".into()),
                value: Expression::IntegerLiteral(0),
            }],
        );
        let call_bearing = function(
            "call_bearing",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "external".into(),
                arguments: Vec::new(),
            })],
        );
        let caller = function(
            "caller",
            Vec::new(),
            vec![
                Statement::Expression(Expression::Call {
                    name: "leaf".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::Call {
                    name: "call_bearing".into(),
                    arguments: Vec::new(),
                }),
            ],
        );

        let expanded = InlineBodySet::analyze(&[leaf, call_bearing])
            .expand_calls_with_facts(&caller)
            .expect("both statement bodies should compose");
        assert_eq!(expanded.statement_body_substitutions, 2);
        assert_eq!(expanded.statement_frame_residue_substitutions, 1);
    }

    #[test]
    fn ordinary_statement_body_does_not_leave_retained_inline_frame_residue() {
        let helper = function(
            "helper",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "external".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );

        let expanded =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone()], &[])
                .expand_calls_with_facts(&caller)
                .expect("the ordinary statement body should compose");
        assert_eq!(expanded.statement_body_substitutions, 1);
        assert_eq!(expanded.statement_frame_residue_substitutions, 0);
    }

    #[test]
    fn composes_zero_argument_embedded_asm_at_a_nested_call_site() {
        let mut helper = function("configure", Vec::new(), Vec::new());
        helper.inline_asm_blocks = vec![InlineAsmBlock {
            statement_index: 0,
            items: vec![AsmItem::Instruction(AsmInstruction {
                mnemonic: "li".into(),
                operands: vec![AsmOperand::Gpr(3), AsmOperand::Immediate(4)],
                source_line: 7,
            })],
        }];
        let caller = function(
            "caller",
            Vec::new(),
            vec![Statement::If {
                condition: Expression::Variable("enabled".into()),
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "configure".into(),
                    arguments: Vec::new(),
                })],
                else_body: Vec::new(),
            }],
        );

        let expanded = InlineBodySet::analyze(&[helper])
            .expand_calls(&caller)
            .expect("the embedded-asm helper should compose");
        assert!(matches!(
            expanded.statements.as_slice(),
            [Statement::If { then_body, .. }]
                if matches!(
                    then_body.as_slice(),
                    [Statement::InlineAsm(items)]
                        if matches!(
                            items.as_slice(),
                            [AsmItem::Instruction(AsmInstruction { mnemonic, .. })]
                                if mnemonic == "li"
                        )
                )
        ));
    }

    #[test]
    fn alpha_renames_locals_and_initializes_them_at_each_call_site() {
        let mut adjust = function(
            "adjust",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "input".into(),
            }],
            vec![
                Statement::Assign {
                    name: "value".into(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable("value".into())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    },
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("value".into())],
                }),
            ],
        );
        adjust.locals = vec![local(
            "value",
            Type::Int,
            Expression::Variable("input".into()),
        )];
        let mut caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "input".into(),
            }],
            vec![
                Statement::Expression(Expression::Call {
                    name: "adjust".into(),
                    arguments: vec![Expression::Variable("input".into())],
                }),
                Statement::Expression(Expression::Call {
                    name: "adjust".into(),
                    arguments: vec![Expression::Variable("input".into())],
                }),
            ],
        );
        caller.locals = vec![local("value", Type::Int, Expression::IntegerLiteral(9))];

        let expanded = InlineBodySet::analyze(&[adjust])
            .expand_calls(&caller)
            .expect("a local-bearing retained body should compose");
        assert_eq!(expanded.locals.len(), 3);
        let first = &expanded.locals[1].name;
        let second = &expanded.locals[2].name;
        assert_ne!(first, "value");
        assert_ne!(first, second);
        assert!(expanded.locals[1..]
            .iter()
            .all(|local| local.initializer.is_none()));
        assert!(matches!(
            expanded.statements.as_slice(),
            [
                Statement::Assign { name: first_init, value: Expression::Variable(first_value) },
                Statement::Assign { name: first_update, .. },
                Statement::Expression(Expression::Call { arguments: first_arguments, .. }),
                Statement::Assign { name: second_init, value: Expression::Variable(second_value) },
                Statement::Assign { name: second_update, .. },
                Statement::Expression(Expression::Call { arguments: second_arguments, .. }),
            ] if first_init == first && first_update == first && first_value == "input"
                && matches!(first_arguments.as_slice(), [Expression::Variable(name)] if name == first)
                && second_init == second && second_update == second && second_value == "input"
                && matches!(second_arguments.as_slice(), [Expression::Variable(name)] if name == second)
        ));
    }

    #[test]
    fn alpha_renames_automatic_arrays_at_each_call_site() {
        let mut format = function(
            "format",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::AddressOf {
                    operand: Box::new(Expression::Variable("text".into())),
                }],
            })],
        );
        format.locals = vec![LocalDeclaration {
            declared_type: Type::Char,
            name: "text".into(),
            initializer: None,
            is_volatile: false,
            array_length: Some(3),
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }];
        let caller = function(
            "caller",
            Vec::new(),
            vec![
                Statement::Expression(Expression::Call {
                    name: "format".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::Call {
                    name: "format".into(),
                    arguments: Vec::new(),
                }),
            ],
        );

        let expanded = InlineBodySet::analyze(&[format])
            .expand_calls(&caller)
            .expect("an automatic array should compose hygienically");
        assert_eq!(expanded.locals.len(), 2);
        let first = &expanded.locals[0].name;
        let second = &expanded.locals[1].name;
        assert_ne!(first, second);
        assert!(expanded
            .locals
            .iter()
            .all(|local| local.array_length == Some(3)));
        assert!(matches!(
            expanded.statements.as_slice(),
            [
                Statement::Expression(Expression::Call {
                    arguments: first_arguments,
                    ..
                }),
                Statement::Expression(Expression::Call {
                    arguments: second_arguments,
                    ..
                }),
            ] if matches!(
                first_arguments.as_slice(),
                [Expression::AddressOf { operand }]
                    if matches!(operand.as_ref(), Expression::Variable(name) if name == first)
            ) && matches!(
                second_arguments.as_slice(),
                [Expression::AddressOf { operand }]
                    if matches!(operand.as_ref(), Expression::Variable(name) if name == second)
            )
        ));
    }

    #[test]
    fn composes_a_counter_loop_from_a_nested_switch_call_site() {
        let pointer = Type::StructPointer { element_size: 272 };
        let mut clear = function(
            "clear",
            vec![Parameter {
                parameter_type: pointer,
                name: "object".into(),
            }],
            vec![
                Statement::Loop {
                    kind: LoopKind::For,
                    initializer: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("index".into())),
                        value: Box::new(Expression::IntegerLiteral(0)),
                    }),
                    condition: Some(Expression::Binary {
                        operator: BinaryOperator::Less,
                        left: Box::new(Expression::Variable("index".into())),
                        right: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("object".into())),
                            offset: 260,
                            member_type: Type::Int,
                            index_stride: None,
                        }),
                    }),
                    step: Some(Expression::PostStep {
                        target: Box::new(Expression::Variable("index".into())),
                        operator: BinaryOperator::Add,
                        pointer_link: None,
                    }),
                    body: vec![Statement::Store {
                        target: Expression::Index {
                            base: Box::new(Expression::MemberAddress {
                                base: Box::new(Expression::Variable("object".into())),
                                offset: 4,
                                element: Pointee::Char,
                                index_stride: None,
                            }),
                            index: Box::new(Expression::Variable("index".into())),
                        },
                        value: Expression::IntegerLiteral(32),
                    }],
                },
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 260,
                        member_type: Type::Int,
                        index_stride: None,
                    },
                    value: Expression::IntegerLiteral(0),
                },
            ],
        );
        clear.locals = vec![LocalDeclaration {
            declared_type: Type::Int,
            name: "index".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }];
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: pointer,
                name: "object".into(),
            }],
            vec![Statement::Switch {
                scrutinee: Expression::IntegerLiteral(1),
                arms: vec![SwitchArm {
                    value: 1,
                    body: ArmBody::Statements(vec![Statement::Expression(
                        Expression::Call {
                            name: "clear".into(),
                            arguments: vec![Expression::Variable("object".into())],
                        },
                    )]),
                    falls_through: false,
                }],
                default: None,
            }],
        );

        let expanded = InlineBodySet::analyze(&[clear])
            .expand_calls(&caller)
            .expect("the canonical retained counter loop should compose");
        let renamed = &expanded.locals[0].name;
        assert!(matches!(
            expanded.statements.as_slice(),
            [Statement::Switch { arms, .. }]
                if matches!(
                    arms[0].body,
                    ArmBody::Statements(ref body)
                        if matches!(
                            body.as_slice(),
                            [
                                Statement::Loop {
                                    initializer: Some(Expression::Assign { target, .. }),
                                    body: loop_body,
                                    ..
                                },
                                Statement::Store { .. },
                            ] if matches!(target.as_ref(), Expression::Variable(name) if name == renamed)
                                && matches!(
                                    loop_body.as_slice(),
                                    [Statement::Store {
                                        target: Expression::Index { index, .. },
                                        ..
                                    }] if matches!(index.as_ref(), Expression::Variable(name) if name == renamed)
                                )
                        )
                )
        ));
    }

    #[test]
    fn materializes_a_scalar_member_argument_before_statement_body_expansion() {
        let pointer = Type::StructPointer { element_size: 8 };
        let member = |base: &str, offset| Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset,
            member_type: Type::Float,
            index_stride: None,
        };
        let mut clamp = function(
            "clamp",
            vec![
                Parameter {
                    parameter_type: pointer,
                    name: "object".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "limit".into(),
                },
            ],
            vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Greater,
                    left: Box::new(Expression::Variable("value".into())),
                    right: Box::new(Expression::Variable("limit".into())),
                },
                then_body: vec![Statement::Store {
                    target: member("object", 0),
                    value: Expression::Variable("limit".into()),
                }],
                else_body: Vec::new(),
            }],
        );
        clamp.locals = vec![local("value", Type::Float, member("object", 0))];
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: pointer,
                name: "object".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "clamp".into(),
                arguments: vec![
                    Expression::Variable("object".into()),
                    member("object", 4),
                ],
            })],
        );

        let expanded = InlineBodySet::analyze_with_definitions(
            &[clamp, caller.clone()],
            &[],
        )
        .expand_calls(&caller)
        .expect("the member argument should be evaluated once before expansion");

        assert_eq!(expanded.locals.len(), 2);
        assert!(matches!(
            expanded.statements.as_slice(),
            [
                Statement::Assign {
                    name: parameter_temp,
                    value: Expression::Member { offset: 4, .. },
                },
                Statement::Assign {
                    name: callee_local,
                    value: Expression::Member { offset: 0, .. },
                },
                Statement::If {
                    condition: Expression::Binary { right, .. },
                    ..
                },
            ] if parameter_temp == &expanded.locals[0].name
                && callee_local == &expanded.locals[1].name
                && matches!(right.as_ref(), Expression::Variable(name)
                    if name == parameter_temp)
        ));
    }

    #[test]
    fn expands_constructor_body_when_its_return_value_is_discarded() {
        let aggregate = Type::Struct { size: 12, align: 4 };
        let pointer = Type::StructPointer { element_size: 16 };
        let mut constructor = function(
            "constructor",
            vec![
                Parameter {
                    parameter_type: pointer,
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 0 },
                    name: "source".into(),
                },
            ],
            vec![
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("this".into())),
                        offset: 0,
                        member_type: Type::StructPointer { element_size: 0 },
                        index_stride: None,
                    },
                    value: Expression::AddressOf {
                        operand: Box::new(Expression::Variable("vtable".into())),
                    },
                },
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("this".into())),
                        offset: 4,
                        member_type: aggregate,
                        index_stride: None,
                    },
                    value: Expression::Variable("source".into()),
                },
            ],
        );
        constructor.return_type = pointer;
        constructor.return_expression = Some(Expression::Variable("this".into()));
        let caller = function(
            "caller",
            vec![],
            vec![Statement::Expression(Expression::Call {
                name: "constructor".into(),
                arguments: vec![
                    Expression::AddressOf {
                        operand: Box::new(Expression::Variable("target".into())),
                    },
                    Expression::Variable("source".into()),
                ],
            })],
        );

        let expanded = InlineBodySet::analyze(&[constructor])
            .expand_calls(&caller)
            .expect("a discarded constructor call should compose");
        assert!(matches!(
            expanded.statements.as_slice(),
            [Statement::Store { .. }, Statement::Store {
                target: Expression::Member { base, offset: 4, .. },
                value: Expression::Variable(source),
            }] if matches!(base.as_ref(), Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Variable(target) if target == "target"))
                && source == "source"
        ));
    }

    #[test]
    fn expands_nested_constructor_body_for_guarded_scalar_new() {
        let pointer = Type::StructPointer { element_size: 16 };
        let mut base = function(
            "base_constructor",
            vec![
                Parameter {
                    parameter_type: pointer,
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Char),
                    name: "name".into(),
                },
            ],
            vec![Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".into())),
                    offset: 4,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
                value: Expression::IntegerLiteral(7),
            }],
        );
        base.return_type = pointer;
        base.return_expression = Some(Expression::Variable("this".into()));
        let mut derived = function(
            "derived_constructor",
            vec![Parameter {
                parameter_type: pointer,
                name: "this".into(),
            }],
            vec![
                Statement::Expression(Expression::Call {
                    name: "base_constructor".into(),
                    arguments: vec![
                        Expression::Variable("this".into()),
                        Expression::StringLiteral(b"state".to_vec()),
                    ],
                }),
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("this".into())),
                        offset: 0,
                        member_type: Type::StructPointer { element_size: 0 },
                        index_stride: None,
                    },
                    value: Expression::AddressOf {
                        operand: Box::new(Expression::Variable("derived_vtable".into())),
                    },
                },
            ],
        );
        derived.return_type = pointer;
        derived.return_expression = Some(Expression::Variable("this".into()));

        let expanded = InlineBodySet::analyze(&[base, derived])
            .expand_constructed_new_body("derived_constructor", "allocation", &[])
            .expect("a local-free constructor chain should compose inside new");
        fn assigned_offsets(expression: &Expression, output: &mut Vec<u32>) {
            match expression {
                Expression::Assign { target, .. } => {
                    if let Expression::Member { offset, .. } = target.as_ref() {
                        output.push(*offset);
                    }
                }
                Expression::Comma { left, right } => {
                    assigned_offsets(left, output);
                    assigned_offsets(right, output);
                }
                _ => {}
            }
        }
        fn terminal(expression: &Expression) -> &Expression {
            match expression {
                Expression::Comma { right, .. } => terminal(right),
                expression => expression,
            }
        }
        let mut offsets = Vec::new();
        assigned_offsets(&expanded, &mut offsets);
        assert_eq!(offsets, vec![4, 0]);
        assert!(matches!(terminal(&expanded), Expression::Variable(name) if name == "allocation"));
    }

    #[test]
    fn expands_nested_void_statement_bodies_with_stable_arguments() {
        let check = function(
            "check",
            vec![Parameter {
                parameter_type: Type::UnsignedInt,
                name: "size".into(),
            }],
            vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Greater,
                    left: Box::new(Expression::Variable("size".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "overflow".into(),
                    arguments: Vec::new(),
                })],
                else_body: Vec::new(),
            }],
        );
        let write = function(
            "write",
            vec![Parameter {
                parameter_type: Type::UnsignedChar,
                name: "byte".into(),
            }],
            vec![Statement::Store {
                target: Expression::Variable("sink".into()),
                value: Expression::Variable("byte".into()),
            }],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::UnsignedChar,
                name: "data".into(),
            }],
            vec![
                Statement::Expression(Expression::Call {
                    name: "check".into(),
                    arguments: vec![Expression::IntegerLiteral(1)],
                }),
                Statement::Expression(Expression::Call {
                    name: "write".into(),
                    arguments: vec![Expression::Variable("data".into())],
                }),
            ],
        );

        let expanded = InlineBodySet::analyze(&[check, write])
            .expand_calls(&caller)
            .expect("both retained bodies should compose");
        assert_eq!(expanded.statements.len(), 2);
        assert!(matches!(
            &expanded.statements[0],
            Statement::Expression(Expression::Call { name, arguments })
                if name == "overflow" && arguments.is_empty()
        ));
        assert!(matches!(
            &expanded.statements[1],
            Statement::Store {
                value: Expression::Variable(name), ..
            } if name == "data"
        ));
    }

    #[test]
    fn materializes_and_calls_beyond_the_nested_inline_budget() {
        fn caller(name: &str, callee: &str) -> Function {
            function(
                name,
                Vec::new(),
                vec![Statement::Expression(Expression::Call {
                    name: callee.into(),
                    arguments: Vec::new(),
                })],
            )
        }

        let root = caller("root", "first");
        let first = caller("first", "second");
        let second = caller("second", "third");
        let third = function(
            "third",
            Vec::new(),
            vec![Statement::Store {
                target: Expression::Variable("sink".into()),
                value: Expression::IntegerLiteral(1),
            }],
        );
        let skipped = [first, second, third.clone()];

        assert_eq!(
            InlineBodySet::depth_limited_fallbacks(
                std::slice::from_ref(&root),
                &skipped,
                2
            ),
            [vec!["third".to_string()]]
        );

        let expanded = InlineBodySet::analyze_with_definitions(
            &[root.clone(), third],
            &skipped,
        )
        .expand_calls(&root)
        .expect("a callable depth-limited fallback may remain");
        assert!(matches!(
            expanded.statements.as_slice(),
            [Statement::Expression(Expression::Call { name, arguments })]
                if name == "third" && arguments.is_empty()
        ));
    }

    #[test]
    fn expands_a_stable_adjusted_this_argument() {
        let member = || Expression::Member {
            base: Box::new(Expression::Variable("this".into())),
            offset: 4,
            member_type: Type::UnsignedInt,
            index_stride: None,
        };
        let setter = function(
            "enable",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 8 },
                name: "this".into(),
            }],
            vec![Statement::Store {
                target: member(),
                value: Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: Box::new(member()),
                    right: Box::new(Expression::IntegerLiteral(2)),
                },
            }],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 112 },
                name: "this".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "enable".into(),
                arguments: vec![Expression::MemberAddress {
                    base: Box::new(Expression::Variable("this".into())),
                    offset: 104,
                    element: Pointee::UnsignedChar,
                    index_stride: None,
                }],
            })],
        );

        let expanded = InlineBodySet::analyze(&[setter])
            .expand_calls(&caller)
            .expect("an adjusted stable object pointer should compose");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::Store {
                target: Expression::Member { base, offset: 108, .. },
                ..
            }
        ] if matches!(base.as_ref(), Expression::Variable(name) if name == "this")));
    }

    #[test]
    fn folds_an_embedded_object_receiver_into_an_inlined_pointer_member_store() {
        let setter = function(
            "set_status",
            vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 300 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 60 },
                    name: "status".into(),
                },
            ],
            vec![Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".into())),
                    offset: 68,
                    member_type: Type::StructPointer { element_size: 60 },
                    index_stride: None,
                },
                value: Expression::Variable("status".into()),
            }],
        );
        let addressed_status = Expression::AddressOf {
            operand: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("actor".into())),
                offset: 668,
                member_type: Type::Struct { size: 60, align: 4 },
                index_stride: None,
            }),
        };
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 1028 },
                name: "actor".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "set_status".into(),
                arguments: vec![
                    Expression::Member {
                        base: Box::new(Expression::Variable("actor".into())),
                        offset: 728,
                        member_type: Type::Struct { size: 300, align: 4 },
                        index_stride: None,
                    },
                    addressed_status.clone(),
                ],
            })],
        );

        let expanded = InlineBodySet::analyze(&[setter])
            .expand_calls(&caller)
            .expect("the embedded receiver should compose into the final field");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::Store {
                target: Expression::Member { base, offset: 796, member_type: Type::StructPointer { element_size: 60 }, .. },
                value,
            }
        ] if matches!(base.as_ref(), Expression::Variable(name) if name == "actor")
            && matches!(value, Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Member { base, offset: 668, member_type: Type::Struct { size: 60, align: 4 }, .. }
                    if matches!(base.as_ref(), Expression::Variable(name) if name == "actor")))));
    }

    #[test]
    fn expands_a_scalarized_copy_through_an_embedded_adjusted_object() {
        let aggregate = Type::Struct { size: 12, align: 4 };
        let setter = function(
            "set_center",
            vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 20 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 0 },
                    name: "source".into(),
                },
            ],
            vec![Statement::Expression(Expression::Assign {
                target: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("this".into())),
                    offset: 0,
                    member_type: Type::Float,
                    index_stride: None,
                }),
                value: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("source".into())),
                    offset: 0,
                    member_type: Type::Float,
                    index_stride: None,
                }),
            })],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 1028 },
                name: "object".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "set_center".into(),
                arguments: vec![
                    Expression::MemberAddress {
                        base: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("object".into())),
                            offset: 728,
                            member_type: Type::Struct { size: 300, align: 4 },
                            index_stride: None,
                        }),
                        offset: 280,
                        element: Pointee::UnsignedChar,
                        index_stride: None,
                    },
                    Expression::Member {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 504,
                        member_type: aggregate,
                        index_stride: None,
                    },
                ],
            })],
        );

        let expanded = InlineBodySet::analyze(&[setter])
            .expand_calls(&caller)
            .expect("the typed aggregate lvalue and adjusted object should compose");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::Expression(Expression::Assign { target, value })
        ] if matches!(target.as_ref(), Expression::Member { base, offset: 1008, member_type: Type::Float, .. }
                if matches!(base.as_ref(), Expression::Variable(name) if name == "object"))
            && matches!(value.as_ref(), Expression::Member { base, offset: 504, member_type: Type::Float, .. }
                if matches!(base.as_ref(), Expression::Variable(name) if name == "object"))));
    }

    #[test]
    fn expands_a_stable_member_address_argument() {
        let setter = function(
            "set_scale",
            vec![Parameter {
                parameter_type: Type::Pointer(Pointee::Float),
                name: "scale".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "consume_scale".into(),
                arguments: vec![Expression::Variable("scale".into())],
            })],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 64 },
                name: "jobj".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "set_scale".into(),
                arguments: vec![Expression::AddressOf {
                    operand: Box::new(Expression::Member {
                        base: Box::new(Expression::Variable("jobj".into())),
                        offset: 44,
                        member_type: Type::Float,
                        index_stride: None,
                    }),
                }],
            })],
        );

        let expanded = InlineBodySet::analyze(&[setter])
            .expand_calls(&caller)
            .expect("a stable lvalue address should compose");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::Expression(Expression::Call { arguments, .. })
        ] if matches!(arguments.as_slice(), [Expression::AddressOf { operand }]
            if matches!(operand.as_ref(), Expression::Member { offset: 44, .. }))));
    }

    #[test]
    fn materializes_an_impure_value_inline_argument_once() {
        let mut identity = function(
            "write",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            Vec::new(),
        );
        identity.return_type = Type::Int;
        identity.return_expression = Some(Expression::Variable("value".into()));
        let mut caller = function("caller", Vec::new(), Vec::new());
        caller.return_type = Type::Int;
        caller.return_expression = Some(Expression::Call {
            name: "write".into(),
            arguments: vec![Expression::Call {
                name: "side_effect".into(),
                arguments: Vec::new(),
            }],
        });
        let expanded = InlineBodySet::analyze(&[identity])
            .expand_calls(&caller)
            .expect("an impure argument should be captured at the call site");
        assert_eq!(expanded.locals.len(), 1);
        assert!(matches!(expanded.return_expression,
            Some(Expression::Comma { left, .. })
        if matches!(left.as_ref(), Expression::Assign { value, .. }
            if matches!(value.as_ref(), Expression::Call { name, .. } if name == "side_effect"))));
    }

    #[test]
    fn expands_a_single_store_that_consumes_one_impure_argument_once() {
        let setter = function(
            "setter",
            vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 8 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "value".into(),
                },
            ],
            vec![Statement::Store {
                target: Expression::Member {
                    base: Box::new(Expression::Variable("this".into())),
                    offset: 4,
                    member_type: Type::Int,
                    index_stride: None,
                },
                value: Expression::Variable("value".into()),
            }],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 8 },
                name: "object".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "setter".into(),
                arguments: vec![
                    Expression::Variable("object".into()),
                    Expression::Call {
                        name: "get_value".into(),
                        arguments: Vec::new(),
                    },
                ],
            })],
        );

        let expanded = InlineBodySet::analyze(&[setter])
            .expand_calls(&caller)
            .expect("the getter call is evaluated once by the substituted store");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::Store {
                target: Expression::Member { base, offset: 4, .. },
                value: Expression::Call { name, arguments },
            }
        ] if matches!(base.as_ref(), Expression::Variable(object) if object == "object")
            && name == "get_value" && arguments.is_empty()));
    }

    #[test]
    fn rejects_a_changing_caller_value_and_an_escape() {
        let write = function(
            "write",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Variable("value".into()))],
        );
        let mut caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "data".into(),
            }],
            vec![
                Statement::Expression(Expression::Call {
                    name: "write".into(),
                    arguments: vec![Expression::Variable("data".into())],
                }),
                Statement::Assign {
                    name: "data".into(),
                    value: Expression::IntegerLiteral(3),
                },
            ],
        );
        assert!(InlineBodySet::analyze(&[write])
            .expand_calls(&caller)
            .is_none());

        caller.statements.pop();
        assert!(InlineBodySet::analyze(&[function(
            "write",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::AddressOf {
                operand: Box::new(Expression::Variable("value".into())),
            })],
        )])
        .expand_calls(&caller)
        .is_none());
    }

    #[test]
    fn composes_a_one_call_ordinary_void_definition_and_localizes_its_return() {
        let helper = function(
            "helper",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::If {
                condition: Expression::Variable("value".into()),
                then_body: vec![Statement::Return(None)],
                else_body: Vec::new(),
            }],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );

        let bodies =
            InlineBodySet::analyze_with_definitions(&[helper.clone(), caller.clone()], &[]);
        assert!(!bodies.calls_required(&caller));
        assert!(bodies.calls_any(&caller));
        let expanded = bodies
            .expand_calls(&caller)
            .expect("a sole ordinary call should be an automatic-inline candidate");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::If { then_body, .. },
            Statement::Label(boundary),
        ] if matches!(then_body.as_slice(), [Statement::Goto(target)] if target == boundary)));

        let forward =
            InlineBodySet::analyze_with_definitions(&[caller.clone(), helper.clone()], &[]);
        assert!(!forward.calls_any(&caller));
        assert!(
            forward.expand_calls(&caller).is_none(),
            "ordinary file IPA cannot use a definition it has not reached yet"
        );

        let required = InlineBodySet::analyze(&[helper.clone()]);
        assert!(required.calls_required(&caller));

        let mut second_caller = caller.clone();
        second_caller.name = "second_caller".into();
        let repeated =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone(), second_caller], &[]);
        assert!(repeated.expand_calls(&caller).is_none());
    }

    #[test]
    fn ignores_calls_from_unreferenced_retained_bodies_when_selecting_an_ordinary_inline() {
        let helper = function(
            "helper",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Store {
                target: Expression::Variable("output".into()),
                value: Expression::Variable("value".into()),
            }],
        );
        let caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );
        let retained = function(
            "unused_inline",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );

        let bodies =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone()], &[retained]);
        assert!(bodies.calls_any(&caller));
        assert!(bodies.expand_calls(&caller).is_some());
    }

    #[test]
    fn composes_a_repeated_ordinary_definition_only_at_a_loop_site() {
        let helper = function(
            "helper",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );
        let loop_caller = function(
            "loop_caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::Variable("value".into())),
                step: None,
                body: vec![
                    Statement::Expression(Expression::Call {
                        name: "helper".into(),
                        arguments: vec![Expression::Variable("value".into())],
                    }),
                    Statement::Assign {
                        name: "value".into(),
                        value: Expression::IntegerLiteral(3),
                    },
                ],
            }],
        );
        let ordinary_caller = function(
            "ordinary_caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );

        let bodies = InlineBodySet::analyze_with_definitions(
            &[helper, loop_caller.clone(), ordinary_caller.clone()],
            &[],
        );
        assert!(bodies.expand_calls(&ordinary_caller).is_none());
        let expanded = bodies
            .expand_repeatable_loop_calls(&loop_caller)
            .expect("the loop call should be eligible for repeated automatic inlining");
        assert!(matches!(expanded.function.statements.as_slice(), [
            Statement::Loop { body, .. }
        ] if matches!(body.as_slice(), [
            Statement::Assign { name: captured, value: Expression::Variable(source) },
            Statement::Expression(Expression::Call { name, arguments }),
            Statement::Assign { name: reassigned, value: Expression::IntegerLiteral(3) },
        ] if captured.starts_with("__mwcc_inline_helper_")
            && source == "value"
            && name == "consume"
            && matches!(arguments.as_slice(), [Expression::Variable(argument)] if argument == captured)
            && reassigned == "value")));
    }

    #[test]
    fn reuses_a_terminal_caller_lane_for_mutable_parameter_selection() {
        let helper = function(
            "select_and_store",
            vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 8 },
                    name: "object".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "value".into(),
                },
            ],
            vec![
                Statement::If {
                    condition: Expression::Binary {
                        operator: BinaryOperator::Greater,
                        left: Box::new(Expression::Variable("value".into())),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    },
                    then_body: vec![Statement::Assign {
                        name: "value".into(),
                        value: Expression::IntegerLiteral(0),
                    }],
                    else_body: Vec::new(),
                },
                Statement::Store {
                    target: Expression::Member {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 4,
                        member_type: Type::Float,
                        index_stride: None,
                    },
                    value: Expression::Variable("value".into()),
                },
            ],
        );
        let caller = function(
            "caller",
            vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 8 },
                    name: "object".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "selected".into(),
                },
            ],
            vec![
                Statement::Assign {
                    name: "selected".into(),
                    value: Expression::FloatLiteral(1.0),
                },
                Statement::Expression(Expression::Call {
                    name: "select_and_store".into(),
                    arguments: vec![
                        Expression::Variable("object".into()),
                        Expression::Variable("selected".into()),
                    ],
                }),
            ],
        );

        let expanded = InlineBodySet::analyze_with_definitions(
            &[helper, caller.clone()],
            &[],
        )
        .expand_calls(&caller)
        .expect("the terminal call should reuse the caller's dead value lane");
        assert!(!expanded
            .locals
            .iter()
            .any(|local| local.name.starts_with("__mwcc_inline_select_and_store_")));
        assert!(!expanded.statements.iter().any(|statement| {
            matches!(statement,
                Statement::Expression(Expression::Call { name, .. })
                    if name == "select_and_store")
        }));
    }

    #[test]
    fn composes_a_one_use_void_forwarder_with_changing_arguments() {
        let helper = function(
            "helper",
            vec![
                Parameter {
                    parameter_type: Type::Float,
                    name: "left".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "right".into(),
                },
            ],
            vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![
                    Expression::Variable("left".into()),
                    Expression::Variable("right".into()),
                ],
            })],
        );
        let mut caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "condition".into(),
            }],
            vec![
                Statement::If {
                    condition: Expression::Variable("condition".into()),
                    then_body: vec![
                        Statement::Assign {
                            name: "left".into(),
                            value: Expression::FloatLiteral(1.0),
                        },
                        Statement::Assign {
                            name: "right".into(),
                            value: Expression::FloatLiteral(2.0),
                        },
                    ],
                    else_body: vec![
                        Statement::Assign {
                            name: "left".into(),
                            value: Expression::FloatLiteral(3.0),
                        },
                        Statement::Assign {
                            name: "right".into(),
                            value: Expression::FloatLiteral(4.0),
                        },
                    ],
                },
                Statement::Expression(Expression::Call {
                    name: "helper".into(),
                    arguments: vec![
                        Expression::Variable("left".into()),
                        Expression::Variable("right".into()),
                    ],
                }),
            ],
        );
        caller.locals = vec![
            LocalDeclaration {
                declared_type: Type::Float,
                name: "left".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            },
            LocalDeclaration {
                declared_type: Type::Float,
                name: "right".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            },
        ];

        let expanded = InlineBodySet::analyze_with_definitions(
            &[helper, caller.clone()],
            &[],
        )
        .expand_calls(&caller)
        .expect("a one-use forwarder should materialize changing arguments once");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded, &mut calls);
        assert!(!calls.contains_key("helper"));
        assert!(calls.contains_key("consume"));
        assert_eq!(expanded.locals.len(), 2);
    }

    #[test]
    fn composes_a_value_body_with_call_site_local_temporaries() {
        let mut helper = function(
            "turn",
            vec![Parameter {
                parameter_type: Type::Float,
                name: "speed".into(),
            }],
            vec![Statement::Expression(Expression::Call {
                name: "update".into(),
                arguments: vec![Expression::Variable("angle".into())],
            })],
        );
        helper.return_type = Type::Float;
        helper.locals = vec![local(
            "angle",
            Type::Float,
            Expression::Call {
                name: "measure".into(),
                arguments: vec![Expression::Variable("speed".into())],
            },
        )];
        helper.return_expression = Some(Expression::Variable("angle".into()));

        let mut caller = function(
            "caller",
            vec![Parameter {
                parameter_type: Type::Float,
                name: "speed".into(),
            }],
            Vec::new(),
        );
        caller.return_type = Type::Float;
        caller.return_expression = Some(Expression::Call {
            name: "turn".into(),
            arguments: vec![Expression::Variable("speed".into())],
        });

        let expanded = InlineBodySet::analyze(&[helper])
            .expand_calls(&caller)
            .expect("a sequenced value body should compose");
        assert_eq!(expanded.locals.len(), 1);
        assert!(expanded.locals[0].initializer.is_none());
        let temporary = &expanded.locals[0].name;
        assert!(temporary.starts_with("__mwcc_inline_turn_"));
        assert!(matches!(
            expanded.return_expression,
            Some(Expression::Comma { left, right })
                if matches!(left.as_ref(), Expression::Assign { target, .. }
                    if matches!(target.as_ref(), Expression::Variable(name) if name == temporary))
                && matches!(right.as_ref(), Expression::Comma { .. })
        ));
    }
}
