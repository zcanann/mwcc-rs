//! Semantics-preserving composition of retained inline function bodies.
//!
//! The frontend keeps skipped inline definitions out of object emission but
//! retains their parsed ASTs. This module owns the conservative subset that can
//! be spliced into a caller without changing argument evaluation: void bodies
//! with automatic scalar locals and no non-local control flow, called as
//! standalone statements with stable scalar arguments. Callee locals are
//! alpha-renamed and initialized at the call site rather than caller entry.

mod call_sites;
mod discarded_result;
mod frame_residue;
mod global_scalar_transaction;
mod ordinal_residue;
mod returns;
mod safety;
mod substitution;
mod value_body;
mod value_calls;
mod vector_transactions;

pub(crate) use call_sites::collect_function_calls;
use crate::inline_source_order::DefinitionOrder;
use mwcc_syntax_trees::{
    ArmBody, AsmItem, Expression, Function, InlineAsmBlock, Statement, SwitchArm, Type,
};
use returns::rewrite_inline_returns;
use safety::{
    automatic_composable_function, bounded_switch_transaction_callee, composable_function,
    materializable_arguments, multi_call_transaction_callee, parameter_requires_materialization,
    repeatable_guarded_call_callee, repeatable_terminal_wrapper_callee, stable_argument,
    stable_arguments, stable_local_values, terminal_scalar_arguments,
};
use std::collections::{HashMap, HashSet};
use substitution::{substitute_expression, substitute_statement};
use value_body::ValueInlineBody;

/// Independent recursion budgets used by MWCC's constructor and ordinary
/// inline-composition passes.
///
/// A constructor chain can exhaust its own budget and still enter small
/// ordinary helpers. Treating both categories as one stack incorrectly
/// materializes helpers reached after two base constructors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InlineNestingBudget {
    pub constructor: usize,
    pub ordinary: usize,
}

impl Default for InlineNestingBudget {
    fn default() -> Self {
        Self {
            constructor: 2,
            ordinary: 2,
        }
    }
}

impl InlineNestingBudget {
    fn permits(self, active: &HashSet<String>, callee: &str) -> bool {
        let constructor = is_constructor(callee);
        let active_depth = active
            .iter()
            .filter(|name| is_constructor(name) == constructor)
            .count();
        active_depth
            < if constructor {
                self.constructor
            } else {
                self.ordinary
            }
    }
}

fn is_constructor(name: &str) -> bool {
    name.starts_with("__ct__")
}

#[derive(Clone, Debug, Default)]
pub struct InlineBodySet {
    /// Read-only ordinary definitions available to semantic transaction
    /// lowerers. Generic automatic inlining still uses the narrower `bodies`
    /// maps below; retaining this view does not make arbitrary functions
    /// composable.
    definitions: HashMap<String, Function>,
    /// Total calls from emitted definitions, retained for exact whole-body
    /// planners whose measured automatic-inline threshold depends on whether a
    /// verified callee is single-use or repeated.
    definition_call_counts: HashMap<String, usize>,
    /// Read-only skipped-inline definitions. Transaction lowerers may inspect
    /// these even when generic AST composition correctly rejects their control
    /// flow; lookup alone never makes them callable or composable.
    retained_definitions: HashMap<String, Function>,
    bodies: HashMap<String, Function>,
    /// Ordinary small definitions that MWCC may expand selectively at hot
    /// structured call sites even when the TU calls them more than once.
    repeatable_bodies: HashMap<String, Function>,
    /// Repeated definitions available to a bounded later caller even when an
    /// earlier call preceded the definition and therefore remains out of line.
    bounded_caller_bodies: HashMap<String, Function>,
    /// Source positions for enforcing that asymmetric visibility at each
    /// bounded call site.
    definition_positions: HashMap<String, usize>,
    /// Tiny condition-plus-call transactions expanded at every visible call
    /// site even when the helper has multiple callers.
    repeatable_guarded_call_bodies: HashMap<String, Function>,
    /// Bounded scalar transactions whose loops and final local result require
    /// statement-level rather than expression-summary composition.
    statement_value_bodies: HashMap<String, Function>,
    /// Larger guarded transactions available only to terminal scratch wrappers.
    terminal_wrapper_bodies: HashMap<String, Function>,
    values: HashMap<String, ValueInlineBody>,
    /// Tiny global read/modify/write value helpers available to callers after
    /// their definition even when earlier calls keep the helper out of the
    /// translation-unit-wide automatic value set.
    repeatable_global_transaction_values: HashMap<String, ValueInlineBody>,
    /// Repeated guarded value transactions selected only for bounded callers.
    /// Larger callers need the general callee-saved allocator before their
    /// inlined callback state can be represented safely.
    guarded_transaction_values: HashMap<String, ValueInlineBody>,
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
    nesting_budget: InlineNestingBudget,
}

pub(crate) fn legacy_frame_residue_bytes(
    function: &Function,
    facts: mwcc_syntax_trees::InlineExpansionFacts,
) -> usize {
    frame_residue::legacy_frame_residue_bytes(function, facts)
}

pub(crate) fn legacy_statement_body_frame_residue_bytes(
    function: &Function,
    retained_substitutions: usize,
    mutating_substitutions: usize,
) -> usize {
    frame_residue::legacy_statement_body_frame_residue_bytes(
        function,
        retained_substitutions,
        mutating_substitutions,
    )
}

pub(crate) fn legacy_value_body_frame_residue_bytes(
    function: &Function,
    substitutions: usize,
) -> usize {
    frame_residue::legacy_value_body_frame_residue_bytes(function, substitutions)
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

pub(crate) fn legacy_mutating_value_body_ordinal_residue(
    function: &Function,
    value_body_substitutions: usize,
) -> u32 {
    ordinal_residue::legacy_mutating_value_body_ordinal_residue(
        function,
        value_body_substitutions,
    )
}

pub(crate) struct ExpandedCalls {
    pub(crate) function: Function,
    pub(crate) statement_body_substitutions: usize,
    pub(crate) statement_frame_residue_substitutions: usize,
    pub(crate) statement_mutating_body_substitutions: usize,
    pub(crate) value_body_substitutions: usize,
    /// Number of distinct source callees for which at least one call site was
    /// removed by this expansion transaction.
    pub(crate) distinct_substituted_callees: u32,
    /// Whether this mixed late-inline lane replays the caller's source control
    /// graph into the post-pool anonymous-ordinal stream.
    pub(crate) replays_source_hidden_ordinals: bool,
    /// Whether ordinary inline-analysis frame residue applies to this lane.
    pub(crate) retains_ordinary_residue: bool,
    /// Whether this lane consumes ordinary inline-analysis anonymous-symbol
    /// ordinals. Whole-file switch duplication retains an allocator lane but
    /// is selected after the ordinary anonymous-symbol pass.
    pub(crate) advances_ordinary_ordinals: bool,
    /// Expanded control-flow nodes absent from the pre-pool ordinal walk.
    pub(crate) pre_constant_ordinal_discount: u32,
    /// Optimizer residue selected after the function's strings/constants.
    /// This is distinct from ordinary inline residue, which precedes them.
    pub(crate) post_constant_ordinal_residue: u32,
    /// Whether structured labels introduced by this late-selected lane are
    /// absent from the translation unit's anonymous-symbol stream.
    pub(crate) discounts_structured_hidden_labels: bool,
    /// Whether allocator liveness must retain the source caller's call
    /// survivors. A composed guarded-value helper folds mutually exclusive
    /// call and fallthrough edges, while MWCC's pre-composition allocator
    /// conservatively sees the original value diamond.
    pub(crate) retains_source_call_survivors: bool,
    /// Mutable data symbols introduced solely by the selected inline body.
    /// Function-local dependency filtering may have omitted them from the
    /// caller's scalar-global map, so recursive lowering activates these exact
    /// targets from the addressable declaration map.
    pub(crate) introduced_mutable_globals: HashSet<String>,
    /// Compiler locals preserving the source return image and its caller-use
    /// image for a composed global scalar transaction. Structured lowering
    /// must see these before it chooses the unoptimized saved-home window.
    pub(crate) global_transaction_result_homes: Vec<String>,
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
        let bounded_caller_bodies = definitions
            .iter()
            .filter(|function| {
                (automatic_composable_function(function)
                    || bounded_switch_transaction_callee(function))
                    && call_counts.get(&function.name).copied().unwrap_or(0) > 1
                    && source_visible_call_counts
                        .get(&function.name)
                        .copied()
                        .unwrap_or(0)
                        > 0
            })
            .map(|function| (function.name.clone(), function.clone()))
            .collect();
        let repeatable_guarded_call_bodies = definitions
            .iter()
            .filter(|function| {
                repeatable_guarded_call_callee(function)
                    && call_counts.get(&function.name).copied().unwrap_or(0) > 1
                    && source_visible_call_counts
                        .get(&function.name)
                        .copied()
                        == call_counts.get(&function.name).copied()
            })
            .map(|function| (function.name.clone(), function.clone()))
            .collect();
        let statement_value_bodies: HashMap<String, Function> = definitions
            .iter()
            .filter(|function| {
                safety::automatic_statement_value_function(function)
                    && source_visible_call_counts
                        .get(&function.name)
                        .copied()
                        .unwrap_or(0)
                        == call_counts.get(&function.name).copied().unwrap_or(0)
            })
            .map(|function| (function.name.clone(), function.clone()))
            .collect();
        let terminal_wrapper_bodies = definitions
            .iter()
            .filter(|function| {
                repeatable_terminal_wrapper_callee(function)
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
            if let Some(body) = value_body::summarize_automatic(function)
                .or_else(|| vector_transactions::summarize_automatic(function))
            {
                values.entry(function.name.clone()).or_insert(body);
            } else {
                let call_count = call_counts.get(&function.name).copied();
                let straight_line = (call_count == Some(1))
                    .then(|| value_body::summarize_automatic_straight_line(function))
                    .flatten();
                let transaction = (function.is_static || call_count == Some(1))
                    .then(|| value_body::summarize_automatic_transaction(function))
                    .flatten();
                if let Some(body) = straight_line.or(transaction) {
                    values
                        .entry(function.name.clone())
                        .and_modify(|existing| existing.automatic_transaction = true)
                        .or_insert(body);
                } else if call_count == Some(1) {
                    if let Some(body) = value_body::summarize_automatic_void_forward(function) {
                        values.entry(function.name.clone()).or_insert(body);
                    }
                }
            }
        }
        let guarded_transaction_values = definitions
            .iter()
            .filter_map(|function| {
                value_body::summarize_automatic_guarded_transaction(function)
                    .map(|body| (function.name.clone(), body))
            })
            .collect();
        let repeatable_global_transaction_values = definitions
            .iter()
            .filter_map(|function| {
                value_body::summarize_repeatable_global_scalar_transaction(function)
                    .map(|body| (function.name.clone(), body))
            })
            .collect();
        if let Some(needle) = std::env::var_os("MWCC_CAPTURE_INLINE") {
            let needle = needle.to_string_lossy();
            for function in definitions
                .iter()
                .filter(|function| function.name.contains(needle.as_ref()))
            {
                eprintln!(
                    "automatic inline summary {}: eligible={} statement_value={} terminal_wrapper={} calls={:?} parameters={} locals={} statements={}",
                    function.name,
                    automatic_composable_function(function),
                    statement_value_bodies.contains_key(&function.name),
                    repeatable_terminal_wrapper_callee(function),
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
                eprintln!("retained inline AST: {function:#?}");
            }
        }
        Self {
            definitions: definitions
                .iter()
                .map(|function| (function.name.clone(), function.clone()))
                .collect(),
            definition_call_counts: call_counts,
            retained_definitions: skipped
                .iter()
                .map(|function| (function.name.clone(), function.clone()))
                .collect(),
            bodies,
            repeatable_bodies,
            bounded_caller_bodies,
            definition_positions: definitions
                .iter()
                .enumerate()
                .map(|(index, function)| (function.name.clone(), index))
                .collect(),
            repeatable_guarded_call_bodies,
            statement_value_bodies,
            terminal_wrapper_bodies,
            values,
            repeatable_global_transaction_values,
            guarded_transaction_values,
            required,
            asm_fragments,
            elide_overwritten_vptr_stores: false,
            nesting_budget: InlineNestingBudget::default(),
        }
    }

    /// Find retained-inline bodies reached beyond MWCC's independent
    /// constructor and ordinary nesting budgets. Each returned name is grouped
    /// after the emitted definition whose expansion first needs its weak
    /// callable fallback.
    pub fn depth_limited_fallbacks(
        definitions: &[Function],
        skipped: &[Function],
        budget: InlineNestingBudget,
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
                    [0, 0],
                    budget,
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

    pub fn with_nesting_budget(mut self, budget: InlineNestingBudget) -> Self {
        self.nesting_budget = budget;
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
            .or_else(|| self.repeatable_guarded_call_bodies.get(name))
    }

    /// An ordinary same-translation-unit definition, exposed only for
    /// lowerers that validate the complete callee shape before composing it.
    pub(crate) fn definition_body(&self, name: &str) -> Option<&Function> {
        self.definitions.get(name)
    }

    pub(crate) fn definition_call_count(&self, name: &str) -> usize {
        self.definition_call_counts.get(name).copied().unwrap_or(0)
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
        calls.keys().any(|name| {
            self.required.contains(name)
                && !self
                    .retained_definitions
                    .get(name)
                    .is_some_and(crate::inline_sqrtf::is_supported_retained_sqrtf)
        })
    }

    /// Whether a function references a retained body by its canonical AST
    /// identity. This supplements the frontend's legacy skipped-name set,
    /// whose C++ entries may still use an unmangled spelling.
    pub(crate) fn calls_any(&self, function: &Function) -> bool {
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        calls
            .keys()
            .any(|name| {
                self.bodies.contains_key(name)
                    || self.repeatable_guarded_call_bodies.contains_key(name)
                    || self.statement_value_bodies.contains_key(name)
                    || self.guarded_transaction_values.contains_key(name)
                    || self.repeatable_global_transaction_values.contains_key(name)
                    || self.values.contains_key(name)
            })
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
        let function_symbols = self
            .definitions
            .keys()
            .chain(self.retained_definitions.keys())
            .filter(|name| name.as_str() != result_name)
            .cloned()
            .collect();
        let mut changed = false;
        let mut substitutions = 0;
        let expanded = value_calls::expand_expression(
            &call,
            &self.values,
            &stable_variables,
            &function_symbols,
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

    /// Expand several repeatable small definitions inside one bounded caller.
    ///
    /// Whole-file IPA selectively duplicates ordinary helpers in compact,
    /// call-dense transactions while leaving the same helpers out of line in
    /// large state-copy functions. Two ordinary helper calls justify
    /// composition. A single helper also qualifies when its body is a
    /// multi-call transaction: MWCC duplicates those compact sequences in small
    /// callers while retaining their callable definition for larger sites.
    pub(crate) fn expand_repeatable_bounded_caller_calls(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        let caller_weight = safety::statement_weight(&function.statements);
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        let caller_position = *self.definition_positions.get(&function.name)?;
        let visible_bodies = self
            .bounded_caller_bodies
            .iter()
            .filter(|(name, _)| {
                self.definition_positions
                    .get(*name)
                    .is_some_and(|callee_position| *callee_position < caller_position)
            })
            .map(|(name, body)| (name.clone(), body.clone()))
            .collect::<HashMap<_, _>>();
        let repeatable_calls = calls
            .iter()
            .filter(|(name, _)| visible_bodies.contains_key(*name))
            .map(|(_, count)| *count)
            .sum::<usize>();
        let has_multi_call_transaction = calls.keys().any(|name| {
            visible_bodies
                .get(name)
                .is_some_and(multi_call_transaction_callee)
        });
        if repeatable_calls < 2 && !has_multi_call_transaction {
            return None;
        }
        let switch_transaction_calls = calls
            .iter()
            .filter(|(name, _)| {
                visible_bodies
                    .get(*name)
                    .is_some_and(bounded_switch_transaction_callee)
            })
            .map(|(_, count)| *count)
            .sum::<usize>();
        // A pair of source sites amortizes the larger transaction even in a
        // substantial dispatcher. A single site remains bounded so large
        // state-copy/error handlers keep the callable helper.
        if caller_weight > 24 && switch_transaction_calls < 2 {
            return None;
        }
        let mut expanded = self.clone();
        expanded.bodies.extend(visible_bodies);
        let mut result = expanded.expand_calls_with_facts_policy(function, true)?;
        if let Some(adjustment) = ordinal_residue::context_snapshot_clear_adjustment(
            &calls,
            result.statement_body_substitutions,
            result.value_body_substitutions,
        ) {
            // Preserve the semantic adjustment separately from version policy.
            // Body lowering enables it only for analyzers whose inline nodes
            // are assigned after function-owned strings.
            result.pre_constant_ordinal_discount = adjustment.pre_constant_discount;
            result.post_constant_ordinal_residue = adjustment.post_constant_residue;
        }
        if switch_transaction_calls != 0 {
            result.statement_frame_residue_substitutions =
                usize::from(switch_transaction_calls == 1);
            result.advances_ordinary_ordinals = false;
            result.discounts_structured_hidden_labels = true;
        }
        Some(result)
    }

    /// Expand a small condition-plus-call helper at each ordinary call site.
    /// Unlike the loop-only repeatable lane, this category is profitable from
    /// the removed helper call itself and is selected for every visible use.
    pub(crate) fn expand_repeatable_guarded_calls(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        if !calls
            .keys()
            .any(|name| self.repeatable_guarded_call_bodies.contains_key(name))
        {
            return None;
        }
        let mut expanded = self.clone();
        expanded
            .bodies
            .extend(self.repeatable_guarded_call_bodies.clone());
        expanded.expand_calls_with_facts_policy(function, false)
    }

    /// Expand a repeated guarded value transaction inside a bounded caller.
    ///
    /// The helper itself is profitable at every site, but a large caller can
    /// introduce several overlapping callback survivors. Keep selection
    /// separate from value summarization so those callers remain callable
    /// until the general saved-register allocator can represent them.
    pub(crate) fn expand_bounded_guarded_value_transactions(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        if safety::statement_weight(&function.statements) > 16 {
            return None;
        }
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        if !calls
            .keys()
            .any(|name| self.guarded_transaction_values.contains_key(name))
        {
            return None;
        }
        let mut expanded = self.clone();
        expanded
            .values
            .extend(self.guarded_transaction_values.clone());
        let mut result = expanded.expand_calls_with_facts_policy(function, false)?;
        // This repeated IPA transaction is selected after ordinary inline
        // analysis and does not consume that pass's anonymous-symbol or frame
        // residue budget. Its alpha-renamed survivor is planned directly by
        // structured callee-saved lowering.
        result.retains_ordinary_residue = false;
        result.advances_ordinary_ordinals = false;
        result.discounts_structured_hidden_labels = true;
        Some(result)
    }

    /// Expand each tiny global scalar transaction after its definition.
    ///
    /// Source visibility is asymmetric in a single translation unit: an
    /// earlier call remains out of line, but that must not suppress the same
    /// helper at later call sites. The ordinary all-or-nothing `values` map
    /// cannot express that distinction, so this lane selects per caller.
    pub(crate) fn expand_visible_global_scalar_transactions(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        let caller_position = *self.definition_positions.get(&function.name)?;
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        let visible_values = self
            .repeatable_global_transaction_values
            .iter()
            .filter(|(name, _)| {
                calls.contains_key(*name)
                    && self
                        .definition_positions
                        .get(*name)
                        .is_some_and(|position| *position < caller_position)
            })
            .map(|(name, body)| (name.clone(), body.clone()))
            .collect::<HashMap<_, _>>();
        if visible_values.is_empty() {
            return None;
        }
        let introduced_mutable_globals = visible_values
            .values()
            .flat_map(ValueInlineBody::stored_global_names)
            .collect();
        let mut expanded = self.clone();
        expanded.values.extend(visible_values);
        let mut result = expanded.expand_calls_with_facts_policy(function, false)?;
        let (linearized, result_homes) = global_scalar_transaction::linearize(
            &result.function,
            &introduced_mutable_globals,
        );
        result.function = linearized;
        result.introduced_mutable_globals = introduced_mutable_globals;
        result.global_transaction_result_homes = result_homes;
        Some(result)
    }

    /// Expand a guarded value transaction together with several repeated
    /// statement helpers in one large, call-dense state-machine callback.
    ///
    /// Treating either family independently is the wrong profitability model:
    /// the guarded transaction exposes the caller's early exit, while the
    /// repeated error helpers share the resulting saved values and epilogue.
    /// Require both families and source visibility so ordinary large callers
    /// keep the conservative out-of-line policy.
    pub(crate) fn expand_mixed_bounded_transactions(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        if safety::statement_weight(&function.statements) <= 16 {
            return None;
        }
        let caller_position = *self.definition_positions.get(&function.name)?;
        let mut calls = HashMap::new();
        collect_function_calls(function, &mut calls);
        let visible_bodies = self
            .bounded_caller_bodies
            .iter()
            .filter(|(name, _)| {
                self.definition_positions
                    .get(*name)
                    .is_some_and(|position| *position < caller_position)
            })
            .map(|(name, body)| (name.clone(), body.clone()))
            .collect::<HashMap<_, _>>();
        let visible_values = self
            .guarded_transaction_values
            .iter()
            .filter(|(name, _)| {
                self.definition_positions
                    .get(*name)
                    .is_some_and(|position| *position < caller_position)
            })
            .map(|(name, body)| (name.clone(), body.clone()))
            .collect::<HashMap<_, _>>();
        let statement_calls = calls
            .iter()
            .filter(|(name, _)| visible_bodies.contains_key(*name))
            .map(|(_, count)| *count)
            .sum::<usize>();
        let guarded_calls = calls
            .iter()
            .filter(|(name, _)| visible_values.contains_key(*name))
            .map(|(_, count)| *count)
            .sum::<usize>();
        if statement_calls < 3 || guarded_calls == 0 {
            return None;
        }
        if std::env::var_os("MWCC_DIAGNOSTIC_ANONYMOUS_ORDINALS").is_some() {
            let mut statement_sites = calls
                .iter()
                .filter_map(|(name, count)| {
                    let body = visible_bodies.get(name)?;
                    Some(format!(
                        "{name}:{count}:w{}:l{}:g{}",
                        safety::statement_weight(&body.statements),
                        body.locals.len(),
                        body.guards.len(),
                    ))
                })
                .collect::<Vec<_>>();
            let mut value_sites = calls
                .iter()
                .filter_map(|(name, count)| {
                    let body = visible_values.get(name)?;
                    Some(format!(
                        "{name}:{count}:w{}:l{}:g{}",
                        safety::statement_weight(&body.source.statements),
                        body.source.locals.len(),
                        body.source.guards.len(),
                    ))
                })
                .collect::<Vec<_>>();
            statement_sites.sort();
            value_sites.sort();
            eprintln!(
                "inline-ordinal-sites {}: statement=[{}] value=[{}]",
                function.name,
                statement_sites.join(","),
                value_sites.join(","),
            );
        }

        let mut expanded = self.clone();
        expanded.bodies.extend(visible_bodies);
        expanded.values.extend(visible_values);
        let mut result = expanded.expand_calls_with_facts_policy(function, true)?;
        // This mixed lane is selected by late whole-file profitability after
        // the ordinary anonymous-label pass. Its structured branches therefore
        // affect frame planning but not later static-local ordinals.
        result.advances_ordinary_ordinals = false;
        result.discounts_structured_hidden_labels = true;
        result.retains_source_call_survivors = true;
        // The build-163 replay transaction is the measured large callback
        // topology: five statement sites, six value sites, and seven distinct
        // helper bodies. Smaller mixed lanes are selected after ordinal
        // analysis and retain no replay block.
        result.replays_source_hidden_ordinals = mixed_inline_replays_source_hidden_ordinals(
            result.statement_body_substitutions,
            result.value_body_substitutions,
            result.distinct_substituted_callees,
        );
        Some(result)
    }

    /// Expand a repeatable definition at a terminal wrapper call. Source-level
    /// scratch padding may precede the call, but no executable wrapper work can
    /// surround it. This is the non-loop counterpart to repeated loop-site
    /// inlining and keeps multi-use helpers unavailable to ordinary callers.
    pub(crate) fn expand_repeatable_terminal_wrapper_call(
        &self,
        function: &Function,
    ) -> Option<ExpandedCalls> {
        let (terminal, prefix) = function.statements.split_last()?;
        let Statement::Expression(Expression::Call { name, .. }) = terminal else {
            return None;
        };
        if !(self.repeatable_bodies.contains_key(name)
            || self.terminal_wrapper_bodies.contains_key(name))
            || !prefix.iter().all(is_empty_padding_loop)
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return None;
        }
        let mut expanded = self.clone();
        expanded.bodies.extend(self.repeatable_bodies.clone());
        expanded.bodies.extend(self.terminal_wrapper_bodies.clone());
        let caller_locals = function
            .locals
            .iter()
            .map(|local| local.name.as_str())
            .collect::<HashSet<_>>();
        let mut result = expanded.expand_calls_with_facts_policy(function, false)?;
        // Build 163 keeps the wrapper's source scratch reservation but discards
        // the repeated callee's unused padding declaration. Its nested mutating
        // helper is absorbed into the enclosing repeated transaction and does
        // not leave an independent frame-residue lane.
        result.function.locals.retain(|local| {
            local.array_length.is_none() || caller_locals.contains(local.name.as_str())
        });
        result.statement_frame_residue_substitutions = 0;
        result.statement_mutating_body_substitutions = 0;
        Some(result)
    }

    /// Select the context-sensitive repeated-body lane used by body lowering.
    ///
    /// Frame and section-anchor planning call this same owner before emission,
    /// so they analyze the exact AST that the lowering driver will later use.
    pub(crate) fn expand_selective_calls(&self, function: &Function) -> Option<ExpandedCalls> {
        self.expand_visible_global_scalar_transactions(function)
            .or_else(|| self.expand_mixed_bounded_transactions(function))
            .or_else(|| self.expand_bounded_guarded_value_transactions(function))
            .or_else(|| self.expand_repeatable_guarded_calls(function))
            .or_else(|| self.expand_repeatable_bounded_caller_calls(function))
            .or_else(|| self.expand_repeatable_loop_calls(function))
            .or_else(|| self.expand_repeatable_terminal_wrapper_call(function))
    }

    /// Effective expanded source for read-only planning passes.
    pub(crate) fn expanded_function_for_planning(&self, function: &Function) -> Option<Function> {
        self.expand_selective_calls(function)
            .map(|expanded| expanded.function)
            .or_else(|| self.expand_calls(function))
    }

    /// Expand only call-free expression helpers for source-liveness planning.
    ///
    /// Mixed late-composition lanes deliberately retain the source allocator's
    /// view of guarded transactions. Ordinary predicate helpers are different:
    /// their calls disappear before allocation and must not make unrelated
    /// caller values look live across a call. Keeping this projection here
    /// gives allocation the same value-body ownership as actual composition
    /// without exposing statement helpers or call-bearing transactions.
    pub(crate) fn expand_call_free_values_for_liveness(
        &self,
        function: &Function,
    ) -> Option<Function> {
        let mut values_only = self.clone();
        values_only.bodies.clear();
        values_only.statement_value_bodies.clear();
        values_only.values.retain(|_, body| {
            !crate::analysis::expression_has_call(&body.expression)
        });
        values_only
            .expand_calls_with_facts_policy(function, false)
            .map(|expanded| expanded.function)
    }

    pub(crate) fn expand_calls_with_facts(&self, function: &Function) -> Option<ExpandedCalls> {
        self.expand_calls_with_facts_policy(function, false)
    }

    fn expand_calls_with_facts_policy(
        &self,
        function: &Function,
        allow_changing_scalar_arguments: bool,
    ) -> Option<ExpandedCalls> {
        let has_inline_residue = function
            .locals
            .iter()
            .any(|local| local.name.starts_with("__mwcc_inline_"));
        let mut source_calls = HashMap::new();
        collect_function_calls(function, &mut source_calls);
        let values = self
            .values
            .iter()
            .filter(|(name, body)| {
                let automatic_transaction = body.automatic_transaction
                    || value_body::summarize_automatic_transaction(&body.source).is_some();
                !automatic_transaction
                    || (!has_inline_residue && source_calls.contains_key(name.as_str()))
            })
            .map(|(name, body)| (name.clone(), body.clone()))
            .collect::<HashMap<_, _>>();
        let mut changed = false;
        let mut statement_body_substitutions = 0;
        let mut statement_frame_residue_substitutions = 0;
        let mut statement_mutating_body_substitutions = 0;
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
        let function_symbols: HashSet<String> = self
            .definitions
            .keys()
            .chain(self.retained_definitions.keys())
            .filter(|name| !occupied_names.contains(name.as_str()))
            .cloned()
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
            &mut statement_mutating_body_substitutions,
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
                    &values,
                    &stable_variables,
                    &function_symbols,
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
                &values,
                &stable_variables,
                &function_symbols,
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
                &values,
                &stable_variables,
                &function_symbols,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
                &mut allocator,
            );
            guard.value = value_calls::expand_expression(
                &guard.value,
                &values,
                &stable_variables,
                &function_symbols,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
                &mut allocator,
            );
        }
        if let Some(return_expression) = &expanded.return_expression {
            expanded.return_expression = Some(value_calls::expand_expression(
                return_expression,
                &values,
                &stable_variables,
                &function_symbols,
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
        let mut required_scope = self.clone();
        required_scope.values = values;
        let calls_remain = required_scope.calls_required(&expanded);
        let mut remaining_calls = HashMap::new();
        collect_function_calls(&expanded, &mut remaining_calls);
        let distinct_substituted_callees = source_calls
            .iter()
            .filter(|(name, count)| {
                remaining_calls.get(*name).copied().unwrap_or(0) < **count
            })
            .count() as u32;
        if calls_remain
            && std::env::var_os("MWCC_CAPTURE_FUNCTION")
                .is_some_and(|name| name == std::ffi::OsStr::new(&function.name))
        {
            let mut calls = HashMap::new();
            collect_function_calls(&expanded, &mut calls);
            let mut retained = calls
                .into_keys()
                .filter(|name| required_scope.required.contains(name))
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
            statement_mutating_body_substitutions,
            value_body_substitutions,
            distinct_substituted_callees,
            replays_source_hidden_ordinals: false,
            retains_ordinary_residue: true,
            advances_ordinary_ordinals: true,
            pre_constant_ordinal_discount: 0,
            post_constant_ordinal_residue: 0,
            discounts_structured_hidden_labels: false,
            retains_source_call_survivors: false,
            introduced_mutable_globals: HashSet::new(),
            global_transaction_result_homes: Vec::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_statement_call(
        &self,
        statement: &Statement,
        statement_index: usize,
        statement_count: usize,
        stable_variables: &HashSet<String>,
        active: &mut HashSet<String>,
        changed: &mut bool,
        locals: &mut Vec<mwcc_syntax_trees::LocalDeclaration>,
        occupied_names: &mut HashSet<String>,
        next_local_id: &mut usize,
        statement_body_substitutions: &mut usize,
        statement_frame_residue_substitutions: &mut usize,
        statement_mutating_body_substitutions: &mut usize,
        allow_terminal_local_reuse: bool,
        allow_changing_scalar_arguments: bool,
    ) -> Option<Vec<Statement>> {
        let (destination, callee_name, arguments, callee) = match statement {
            Statement::Expression(Expression::Call { name, arguments }) => (
                None,
                name,
                arguments,
                self.bodies
                    .get(name)
                    .or_else(|| self.statement_value_bodies.get(name))?,
            ),
            Statement::Assign {
                name: destination,
                value: Expression::Call { name, arguments },
            } => (
                Some(destination.as_str()),
                name,
                arguments,
                self.statement_value_bodies.get(name)?,
            ),
            _ => return None,
        };
        if active.contains(callee_name)
            || !self.nesting_budget.permits(active, callee_name)
            || callee.parameters.len() != arguments.len()
        {
            return None;
        }
        let terminal_direct = destination.is_none()
            && allow_terminal_local_reuse
            && statement_index + 1 == statement_count
            && terminal_scalar_arguments(callee, arguments, stable_variables);
        let known_function_designator = |argument: &Expression| {
            matches!(
                argument,
                Expression::Variable(name)
                    if self.definitions.contains_key(name)
                        || self.retained_definitions.contains_key(name)
            )
        };
        let direct_arguments = arguments.iter().all(|argument| {
            stable_argument(argument, stable_variables)
                || known_function_designator(argument)
        });
        if !stable_arguments(callee, arguments, stable_variables)
            && !materializable_arguments(
                callee,
                arguments,
                stable_variables,
                allow_changing_scalar_arguments,
            )
            && !terminal_direct
            && !direct_arguments
        {
            return None;
        }

        let callee_stable = stable_local_values(callee);
        let mut nested_stable_variables = stable_variables.clone();
        let materialize =
            !terminal_direct && !stable_arguments(callee, arguments, stable_variables);
        let mut replacements = HashMap::new();
        let mut substituted = Vec::new();
        for (parameter, argument) in callee.parameters.iter().zip(arguments) {
            let parameter_is_mutable =
                parameter_requires_materialization(callee, &parameter.name);
            if (!parameter_is_mutable || terminal_direct)
                && (!materialize
                    || stable_argument(argument, stable_variables)
                    || known_function_designator(argument))
            {
                replacements.insert(parameter.name.clone(), argument.clone());
                continue;
            }
            let unique_name = loop {
                let candidate = format!(
                    "__mwcc_inline_{}_{}_{}",
                    callee_name, *next_local_id, parameter.name
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
                attribute_alignment: None,
                row_bytes: None,
            });
            substituted.push(Statement::Assign {
                name: unique_name,
                value: argument.clone(),
            });
        }
        for local in &callee.locals {
            let unique_name = loop {
                let candidate = format!(
                    "__mwcc_inline_{}_{}_{}",
                    callee_name, *next_local_id, local.name
                );
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
        let write_only_result = destination
            .is_none()
            .then(|| discarded_result::write_only_result_local(callee))
            .flatten();
        let guarded_accumulator = destination
            .is_none()
            .then(|| discarded_result::guarded_accumulator_local(callee))
            .flatten();
        let discarded_result = write_only_result.or(guarded_accumulator);
        if let Some(result_name) = write_only_result {
            if let Some(Expression::Variable(substituted_result)) = replacements.get(result_name) {
                substituted =
                    discarded_result::remove_assignments(substituted, substituted_result);
            }
        } else if let Some(result_name) = guarded_accumulator {
            if let Some(Expression::Variable(substituted_result)) = replacements.get(result_name) {
                substituted = discarded_result::remove_straight_line_accumulator_updates(
                    substituted,
                    substituted_result,
                );
            }
        }
        // Guards are trailing early returns in the callee's executable body.
        // Rebind them to a private forward boundary and, for value-position
        // composition, publish the selected return value before leaving this
        // inline instance.
        let return_boundary =
            format!("__mwcc_inline_return_{}_{}", callee_name, *next_local_id);
        *next_local_id += 1;
        let mut has_early_exit = false;
        for guard in &callee.guards {
            if discarded_result.is_some() {
                continue;
            }
            let mut then_body = Vec::new();
            if let Some(destination) = destination {
                then_body.push(Statement::Assign {
                    name: destination.to_owned(),
                    value: substitute_expression(&guard.value, &replacements),
                });
            }
            then_body.push(Statement::Goto(return_boundary.clone()));
            substituted.push(Statement::If {
                condition: substitute_expression(&guard.condition, &replacements),
                then_body,
                else_body: Vec::new(),
            });
            has_early_exit = true;
        }
        if let Some(destination) = destination {
            let returned = substitute_expression(callee.return_expression.as_ref()?, &replacements);
            substituted.push(Statement::Assign {
                name: destination.to_owned(),
                value: returned,
            });
        }
        substituted = fold_constant_inline_branches(substituted);
        // A return exits the callee instance, not its caller. Give every
        // expansion a private forward boundary before recursive composition.
        has_early_exit |= rewrite_inline_returns(&mut substituted, &return_boundary);
        if has_early_exit {
            substituted.push(Statement::Label(return_boundary));
        }
        *changed = true;
        *statement_body_substitutions += 1;
        let mutates_memory = statements_mutate_memory(&callee.statements);
        if mutates_memory {
            *statement_mutating_body_substitutions += 1;
        }
        let mut callee_calls = HashMap::new();
        collect_function_calls(callee, &mut callee_calls);
        if self.required.contains(callee_name)
            && (!callee_calls.is_empty() || mutates_memory)
        {
            *statement_frame_residue_substitutions += 1;
        }
        active.insert(callee_name.clone());
        let output = self.expand_statements(
            &substituted,
            &nested_stable_variables,
            active,
            changed,
            locals,
            occupied_names,
            next_local_id,
            statement_body_substitutions,
            statement_frame_residue_substitutions,
            statement_mutating_body_substitutions,
            false,
            allow_changing_scalar_arguments,
        );
        active.remove(callee_name);
        Some(output)
    }

    #[allow(clippy::too_many_arguments)]
    fn expand_pretest_loop_condition_call(
        &self,
        statement: &Statement,
        stable_variables: &HashSet<String>,
        active: &mut HashSet<String>,
        changed: &mut bool,
        locals: &mut Vec<mwcc_syntax_trees::LocalDeclaration>,
        occupied_names: &mut HashSet<String>,
        next_local_id: &mut usize,
        statement_body_substitutions: &mut usize,
        statement_frame_residue_substitutions: &mut usize,
        statement_mutating_body_substitutions: &mut usize,
        allow_changing_scalar_arguments: bool,
    ) -> Option<Statement> {
        let Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        } = statement
        else {
            return None;
        };
        let (call, negated) = match condition {
            Expression::Call { .. } => (condition, false),
            Expression::Unary {
                operator: mwcc_syntax_trees::UnaryOperator::LogicalNot,
                operand,
            } if matches!(operand.as_ref(), Expression::Call { .. }) => (operand.as_ref(), true),
            _ => return None,
        };
        let Expression::Call {
            name: callee_name,
            arguments,
        } = call
        else {
            return None;
        };
        let callee = self.statement_value_bodies.get(callee_name)?;
        let result_name = loop {
            let candidate = format!(
                "__mwcc_inline_{}_{}_condition",
                callee_name, *next_local_id
            );
            *next_local_id += 1;
            if occupied_names.insert(candidate.clone()) {
                break candidate;
            }
        };
        locals.push(mwcc_syntax_trees::LocalDeclaration {
            declared_type: callee.return_type,
            name: result_name.clone(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        let assignment = Statement::Assign {
            name: result_name.clone(),
            value: Expression::Call {
                name: callee_name.clone(),
                arguments: arguments.clone(),
            },
        };
        let Some(mut expanded_body) = self.expand_statement_call(
            &assignment,
            0,
            1,
            stable_variables,
            active,
            changed,
            locals,
            occupied_names,
            next_local_id,
            statement_body_substitutions,
            statement_frame_residue_substitutions,
            statement_mutating_body_substitutions,
            false,
            allow_changing_scalar_arguments,
        ) else {
            occupied_names.remove(&result_name);
            locals.pop();
            return None;
        };
        let exit_condition = if negated {
            Expression::Variable(result_name)
        } else {
            Expression::Unary {
                operator: mwcc_syntax_trees::UnaryOperator::LogicalNot,
                operand: Box::new(Expression::Variable(result_name)),
            }
        };
        expanded_body.push(Statement::If {
            condition: exit_condition,
            then_body: vec![Statement::Break],
            else_body: Vec::new(),
        });
        expanded_body.extend(self.expand_statements(
            body,
            stable_variables,
            active,
            changed,
            locals,
            occupied_names,
            next_local_id,
            statement_body_substitutions,
            statement_frame_residue_substitutions,
            statement_mutating_body_substitutions,
            false,
            allow_changing_scalar_arguments,
        ));
        Some(Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: expanded_body,
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
        statement_mutating_body_substitutions: &mut usize,
        allow_terminal_local_reuse: bool,
        allow_changing_scalar_arguments: bool,
    ) -> Vec<Statement> {
        let mut output = Vec::new();
        for (statement_index, statement) in statements.iter().enumerate() {
            if let Some(expanded) = self.expand_pretest_loop_condition_call(
                statement,
                stable_variables,
                active,
                changed,
                locals,
                occupied_names,
                next_local_id,
                statement_body_substitutions,
                statement_frame_residue_substitutions,
                statement_mutating_body_substitutions,
                allow_changing_scalar_arguments,
            ) {
                output.push(expanded);
                continue;
            }
            if let Some(expanded) = self.expand_statement_call(
                statement,
                statement_index,
                statements.len(),
                stable_variables,
                active,
                changed,
                locals,
                occupied_names,
                next_local_id,
                statement_body_substitutions,
                statement_frame_residue_substitutions,
                statement_mutating_body_substitutions,
                allow_terminal_local_reuse,
                allow_changing_scalar_arguments,
            ) {
                output.extend(expanded);
                continue;
            }
            match statement {
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
                        statement_mutating_body_substitutions,
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
                        statement_mutating_body_substitutions,
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
                        statement_mutating_body_substitutions,
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
                                    statement_mutating_body_substitutions,
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
                                statement_mutating_body_substitutions,
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
                    || self.statement_value_bodies.contains_key(name)
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

fn statements_mutate_memory(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Store { .. } => true,
        Statement::Expression(Expression::Assign { target, .. }) => {
            !matches!(target.as_ref(), Expression::Variable(_))
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => statements_mutate_memory(then_body) || statements_mutate_memory(else_body),
        Statement::Loop { body, .. } => statements_mutate_memory(body),
        Statement::Switch {
            arms,
            default,
            ..
        } => {
            arms.iter().any(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Return(_) => false,
                mwcc_syntax_trees::ArmBody::Statements(body) => statements_mutate_memory(body),
            }) || default.as_ref().is_some_and(|arm| match arm {
                mwcc_syntax_trees::ArmBody::Return(_) => false,
                mwcc_syntax_trees::ArmBody::Statements(body) => statements_mutate_memory(body),
            })
        }
        _ => false,
    })
}

fn is_empty_padding_loop(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Loop {
            kind: mwcc_syntax_trees::LoopKind::DoWhile,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(0)),
            step: None,
            body,
        } if body.is_empty()
    )
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
    depth: [usize; 2],
    budget: InlineNestingBudget,
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
        let lane = usize::from(!is_constructor(&name));
        let maximum_depth = if lane == 0 {
            budget.constructor
        } else {
            budget.ordinary
        };
        if depth[lane] >= maximum_depth {
            if emitted.insert(name.clone()) {
                output.push(name.clone());
                // A materialized fallback is a fresh compilation root. Its
                // own automatic-inlining budget starts over.
                collect_depth_limited_fallbacks(
                    body,
                    [0, 0],
                    budget,
                    bodies,
                    emitted,
                    &mut HashSet::new(),
                    output,
                );
            }
            continue;
        }
        if active.insert(name.clone()) {
            let mut nested_depth = depth;
            nested_depth[lane] += 1;
            collect_depth_limited_fallbacks(
                body,
                nested_depth,
                budget,
                bodies,
                emitted,
                active,
                output,
            );
            active.remove(&name);
        }
    }
}

fn mixed_inline_replays_source_hidden_ordinals(
    statement_substitutions: usize,
    value_substitutions: usize,
    distinct_substituted_callees: u32,
) -> bool {
    statement_substitutions == 5
        && value_substitutions == 6
        && distinct_substituted_callees == 7
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{
        AsmInstruction, AsmItem, AsmOperand, BinaryOperator, InlineAsmBlock,
        GuardedReturn, LocalDeclaration, LoopKind, Parameter, Pointee, Type,
    };

    #[test]
    fn only_the_large_mixed_transaction_replays_source_ordinals() {
        assert!(mixed_inline_replays_source_hidden_ordinals(5, 6, 7));
        assert!(!mixed_inline_replays_source_hidden_ordinals(4, 6, 7));
        assert!(!mixed_inline_replays_source_hidden_ordinals(5, 5, 7));
        assert!(!mixed_inline_replays_source_hidden_ordinals(5, 6, 6));
    }

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
            attribute_alignment: None,
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
    fn frame_residue_counts_retained_calling_and_mutating_statement_bodies() {
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
        assert_eq!(expanded.statement_frame_residue_substitutions, 2);
        assert_eq!(expanded.statement_mutating_body_substitutions, 1);
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
        assert_eq!(expanded.statement_mutating_body_substitutions, 0);
    }

    #[test]
    fn records_an_ordinary_mutating_body_separately_from_retained_frame_residue() {
        let helper = function(
            "helper",
            Vec::new(),
            vec![Statement::Store {
                target: Expression::Variable("memory".into()),
                value: Expression::IntegerLiteral(0),
            }],
        );
        let caller = function(
            "caller",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: Vec::new(),
            })],
        );

        let expanded =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone()], &[])
                .expand_calls_with_facts(&caller)
                .expect("the ordinary mutating body should compose");
        assert_eq!(expanded.statement_body_substitutions, 1);
        assert_eq!(expanded.statement_frame_residue_substitutions, 0);
        assert_eq!(expanded.statement_mutating_body_substitutions, 1);
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
            attribute_alignment: None,
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
            attribute_alignment: None,
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
    fn materializes_a_scalar_call_result_before_statement_body_expansion() {
        let helper = function(
            "helper",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            ["first", "second"]
                .into_iter()
                .map(|name| {
                    Statement::Expression(Expression::Call {
                        name: name.into(),
                        arguments: vec![Expression::Variable("value".into())],
                    })
                })
                .collect(),
        );
        let caller = function(
            "caller",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: vec![Expression::Call {
                    name: "produce".into(),
                    arguments: Vec::new(),
                }],
            })],
        );

        let expanded = InlineBodySet::analyze(&[helper])
            .expand_calls(&caller)
            .expect("the scalar call result should be captured exactly once");
        let captured = &expanded.locals[0].name;
        assert!(matches!(
            expanded.statements.as_slice(),
            [
                Statement::Assign {
                    name,
                    value: Expression::Call { name: producer, .. },
                },
                Statement::Expression(Expression::Call {
                    arguments: first,
                    ..
                }),
                Statement::Expression(Expression::Call {
                    arguments: second,
                    ..
                }),
            ] if name == captured
                && producer == "produce"
                && matches!(first.as_slice(), [Expression::Variable(argument)] if argument == captured)
                && matches!(second.as_slice(), [Expression::Variable(argument)] if argument == captured)
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
                InlineNestingBudget::default(),
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
    fn constructor_depth_does_not_consume_the_ordinary_inline_budget() {
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

        let root = caller("root", "__ct__first");
        let first = caller("__ct__first", "__ct__second");
        let second = caller("__ct__second", "initialize");
        let initialize = caller("initialize", "set_name");
        let set_name = function(
            "set_name",
            Vec::new(),
            vec![Statement::Store {
                target: Expression::Variable("sink".into()),
                value: Expression::IntegerLiteral(1),
            }],
        );
        let skipped = [first, second, initialize, set_name];

        assert_eq!(
            InlineBodySet::depth_limited_fallbacks(
                std::slice::from_ref(&root),
                &skipped,
                InlineNestingBudget::default(),
            ),
            Vec::<Vec<String>>::from([Vec::new()])
        );

        let expanded = InlineBodySet::analyze(&skipped)
            .expand_calls(&root)
            .expect("constructor and ordinary nesting lanes should compose independently");
        assert!(matches!(
            expanded.statements.as_slice(),
            [Statement::Store {
                target: Expression::Variable(name),
                value: Expression::IntegerLiteral(1),
            }] if name == "sink"
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
    fn keeps_a_known_function_designator_symbolic_in_a_diagnostic_transaction() {
        let callback = function("callback", Vec::new(), Vec::new());
        let mut transaction = function(
            "transaction",
            vec![Parameter {
                parameter_type: Type::Pointer(Pointee::Int),
                name: "callback_argument".into(),
            }],
            (0..5)
                .map(|index| Statement::Store {
                    target: Expression::Variable(format!("published_{index}")),
                    value: Expression::IntegerLiteral(index),
                })
                .chain([
                    Statement::Store {
                        target: Expression::Variable("published_callback".into()),
                        value: Expression::Variable("callback_argument".into()),
                    },
                    Statement::If {
                        condition: Expression::IntegerLiteral(1),
                        then_body: vec![Statement::Expression(Expression::Call {
                            name: "diagnose".into(),
                            arguments: Vec::new(),
                        })],
                        else_body: Vec::new(),
                    },
                    Statement::Assign {
                        name: "idle".into(),
                        value: Expression::Call {
                            name: "issue".into(),
                            arguments: Vec::new(),
                        },
                    },
                ])
                .collect(),
        );
        transaction.return_type = Type::Int;
        transaction.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "idle".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        transaction.return_expression = Some(Expression::Variable("idle".into()));
        let mut caller = function("caller", Vec::new(), Vec::new());
        caller.return_type = Type::Int;
        caller.return_expression = Some(Expression::Call {
            name: "transaction".into(),
            arguments: vec![Expression::Variable("callback".into())],
        });

        let expanded = InlineBodySet::analyze_with_definitions(
            &[callback, transaction, caller.clone()],
            &[],
        )
        .expand_calls(&caller)
        .expect("the visible bounded transaction should compose");

        assert!(!expanded
            .locals
            .iter()
            .any(|local| local.name.ends_with("_callback_argument")));
    }

    #[test]
    fn drops_a_pure_unused_value_inline_argument() {
        let mut constant = function(
            "constant",
            vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 4 },
                name: "this".into(),
            }],
            Vec::new(),
        );
        constant.return_type = Type::Int;
        constant.return_expression = Some(Expression::IntegerLiteral(7));
        let mut caller = function("caller", Vec::new(), Vec::new());
        caller.return_type = Type::Int;
        caller.return_expression = Some(Expression::Call {
            name: "constant".into(),
            arguments: vec![Expression::Variable("global".into())],
        });

        let expanded = InlineBodySet::analyze(&[constant])
            .expand_calls(&caller)
            .expect("the unused pure argument should disappear");
        assert!(expanded.locals.is_empty());
        assert!(matches!(
            expanded.return_expression,
            Some(Expression::IntegerLiteral(7))
        ));
    }

    #[test]
    fn evaluates_an_impure_unused_value_inline_argument_without_a_temporary() {
        let mut constant = function(
            "constant",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "unused".into(),
            }],
            Vec::new(),
        );
        constant.return_type = Type::Int;
        constant.return_expression = Some(Expression::IntegerLiteral(7));
        let mut caller = function("caller", Vec::new(), Vec::new());
        caller.return_type = Type::Int;
        caller.return_expression = Some(Expression::Call {
            name: "constant".into(),
            arguments: vec![Expression::Call {
                name: "side_effect".into(),
                arguments: Vec::new(),
            }],
        });

        let expanded = InlineBodySet::analyze(&[constant])
            .expand_calls(&caller)
            .expect("the unused impure argument should remain sequenced");
        assert!(expanded.locals.is_empty());
        assert!(matches!(
            expanded.return_expression,
            Some(Expression::Comma { left, right })
                if matches!(left.as_ref(), Expression::Call { name, .. } if name == "side_effect")
                    && matches!(right.as_ref(), Expression::IntegerLiteral(7))
        ));
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
    fn composes_a_one_call_straight_line_scalar_helper() {
        let float_parameter = |name: &str| Parameter {
            parameter_type: Type::Float,
            name: name.into(),
        };
        let float_local = |name: &str| LocalDeclaration {
            declared_type: Type::Float,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        };
        let mut helper = function(
            "blend",
            vec![
                float_parameter("weight"),
                float_parameter("left"),
                float_parameter("middle"),
                float_parameter("right"),
            ],
            vec![
                Statement::Assign {
                    name: "inverse".into(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Subtract,
                        left: Box::new(Expression::FloatLiteral(1.0)),
                        right: Box::new(Expression::Variable("weight".into())),
                    },
                },
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable("left".into())),
                        right: Box::new(Expression::Variable("inverse".into())),
                    },
                },
            ],
        );
        helper.return_type = Type::Float;
        helper.is_static = false;
        helper.locals = vec![float_local("inverse"), float_local("result")];
        helper.return_expression = Some(Expression::Variable("result".into()));

        let mut caller = function(
            "caller",
            vec![
                float_parameter("weight"),
                float_parameter("left"),
                float_parameter("middle"),
                float_parameter("right"),
            ],
            vec![Statement::Expression(Expression::Call {
                name: "blend".into(),
                arguments: vec![
                    Expression::Variable("weight".into()),
                    Expression::Variable("left".into()),
                    Expression::Variable("middle".into()),
                    Expression::Variable("right".into()),
                ],
            })],
        );
        caller.is_static = false;

        let bodies =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone()], &[]);
        let expanded = bodies
            .expand_calls(&caller)
            .expect("a one-use pure scalar helper should compose");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded, &mut calls);
        assert!(!calls.contains_key("blend"));
        assert_eq!(expanded.locals.len(), 2);
    }

    #[test]
    fn composes_a_repeated_global_scalar_transaction() {
        let global = || Expression::Variable("random_state".into());
        let mut helper = function(
            "next_random",
            Vec::new(),
            vec![
                Statement::Store {
                    target: global(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Multiply,
                        left: Box::new(global()),
                        right: Box::new(Expression::IntegerLiteral(1103515245)),
                    },
                },
                Statement::Store {
                    target: global(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(global()),
                        right: Box::new(Expression::IntegerLiteral(12345)),
                    },
                },
            ],
        );
        helper.return_type = Type::Int;
        helper.return_expression = Some(Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: Box::new(global()),
            right: Box::new(Expression::IntegerLiteral(16)),
        });
        let mut caller = function("shuffle", Vec::new(), Vec::new());
        caller.return_type = Type::Int;
        caller.return_expression = Some(Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Call {
                name: "next_random".into(),
                arguments: Vec::new(),
            }),
            right: Box::new(Expression::Call {
                name: "next_random".into(),
                arguments: Vec::new(),
            }),
        });

        let mut early_caller = caller.clone();
        early_caller.name = "early_shuffle".into();
        let bodies = InlineBodySet::analyze_with_definitions(
            &[early_caller.clone(), helper, caller.clone()],
            &[],
        );
        assert!(
            bodies.expand_visible_global_scalar_transactions(&early_caller).is_none(),
            "a call before the helper definition remains out of line"
        );
        let expanded = bodies
            .expand_visible_global_scalar_transactions(&caller)
            .expect("the repeatable transaction should replace both calls");
        assert_eq!(expanded.value_body_substitutions, 2);
        assert!(!bodies.calls_any(&expanded.function));
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
    fn bounded_caller_inlines_only_the_post_definition_repeated_calls() {
        let helper = |name: &str| {
            function(
                name,
                vec![Parameter {
                    parameter_type: Type::Int,
                    name: "value".into(),
                }],
                vec![Statement::Expression(Expression::Call {
                    name: format!("consume_{name}"),
                    arguments: vec![Expression::Variable("value".into())],
                })],
            )
        };
        let caller = |name: &str| {
            function(
                name,
                vec![Parameter {
                    parameter_type: Type::Int,
                    name: "value".into(),
                }],
                ["first", "second"]
                    .into_iter()
                    .map(|callee| {
                        Statement::Expression(Expression::Call {
                            name: callee.into(),
                            arguments: vec![Expression::Variable("value".into())],
                        })
                    })
                    .collect(),
            )
        };
        let early = caller("early");
        let late = caller("late");
        let bodies = InlineBodySet::analyze_with_definitions(
            &[
                early.clone(),
                helper("first"),
                helper("second"),
                late.clone(),
            ],
            &[],
        );

        assert!(
            bodies
                .expand_repeatable_bounded_caller_calls(&early)
                .is_none(),
            "a later definition is unavailable to an earlier caller"
        );
        let expanded = bodies
            .expand_repeatable_bounded_caller_calls(&late)
            .expect("the bounded later caller should compose both helpers");
        assert!(matches!(
            expanded.function.statements.as_slice(),
            [
                Statement::Expression(Expression::Call { name: first, .. }),
                Statement::Expression(Expression::Call { name: second, .. }),
            ] if first == "consume_first" && second == "consume_second"
        ));
    }

    #[test]
    fn bounded_caller_inlines_one_repeated_multi_call_transaction() {
        let transaction = function(
            "transaction",
            Vec::new(),
            ["first", "second", "third"]
                .into_iter()
                .map(|name| {
                    Statement::Expression(Expression::Call {
                        name: name.into(),
                        arguments: Vec::new(),
                    })
                })
                .collect(),
        );
        let caller = |name: &str| {
            function(
                name,
                Vec::new(),
                vec![
                    Statement::Expression(Expression::Call {
                        name: "observe_before".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Expression(Expression::Call {
                        name: "transaction".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Expression(Expression::Call {
                        name: "observe_after".into(),
                        arguments: Vec::new(),
                    }),
                ],
            )
        };
        let first = caller("first_caller");
        let second = caller("second_caller");
        let bodies =
            InlineBodySet::analyze_with_definitions(&[transaction, first.clone(), second], &[]);

        let expanded = bodies
            .expand_repeatable_bounded_caller_calls(&first)
            .expect("a repeated multi-call transaction should compose in a bounded caller");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded.function, &mut calls);
        assert!(!calls.contains_key("transaction"));
        assert_eq!(calls.get("first"), Some(&1));
        assert_eq!(calls.get("second"), Some(&1));
        assert_eq!(calls.get("third"), Some(&1));
    }

    #[test]
    fn bounded_caller_materializes_a_changing_scalar_helper_argument() {
        let error = function(
            "error",
            vec![Parameter {
                parameter_type: Type::UnsignedInt,
                name: "code".into(),
            }],
            vec![
                Statement::Expression(Expression::Call {
                    name: "record".into(),
                    arguments: vec![Expression::Variable("code".into())],
                }),
                Statement::Expression(Expression::Call {
                    name: "stop".into(),
                    arguments: Vec::new(),
                }),
            ],
        );
        let transaction = function(
            "transaction",
            Vec::new(),
            ["first", "second", "third"]
                .into_iter()
                .map(|name| {
                    Statement::Expression(Expression::Call {
                        name: name.into(),
                        arguments: Vec::new(),
                    })
                })
                .collect(),
        );
        let caller = function(
            "caller",
            Vec::new(),
            vec![
                Statement::Expression(Expression::Call {
                    name: "transaction".into(),
                    arguments: Vec::new(),
                }),
                Statement::Expression(Expression::Call {
                    name: "error".into(),
                    arguments: vec![Expression::Variable(
                        "changing_global".into(),
                    )],
                }),
                Statement::Expression(Expression::Call {
                    name: "transaction".into(),
                    arguments: Vec::new(),
                }),
            ],
        );
        let second_error_caller = function(
            "second_error_caller",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "error".into(),
                arguments: vec![Expression::IntegerLiteral(4)],
            })],
        );
        let bodies = InlineBodySet::analyze_with_definitions(
            &[
                error,
                transaction,
                caller.clone(),
                second_error_caller,
            ],
            &[],
        );

        let expanded = bodies
            .expand_repeatable_bounded_caller_calls(&caller)
            .expect("the bounded caller should capture the changing argument");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded.function, &mut calls);

        assert!(!calls.contains_key("error"));
        assert!(!calls.contains_key("transaction"));
        assert_eq!(calls.get("record"), Some(&1));
        assert!(expanded.function.statements.iter().any(|statement| {
            matches!(
                statement,
                Statement::Assign {
                    name,
                    value: Expression::Variable(source),
                } if name.starts_with("__mwcc_inline_error_")
                    && source == "changing_global"
            )
        }));
    }

    #[test]
    fn mixed_bounded_lane_retains_source_call_survivor_policy() {
        let helper = function(
            "helper",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: Vec::new(),
            })],
        );
        let mut guarded = function(
            "guarded",
            Vec::new(),
            vec![Statement::If {
                condition: Expression::Variable("requested".into()),
                then_body: vec![
                    Statement::Expression(Expression::Call {
                        name: "publish".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Expression(Expression::Call {
                        name: "finish".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Return(Some(Expression::IntegerLiteral(1))),
                ],
                else_body: Vec::new(),
            }],
        );
        guarded.return_type = Type::Int;
        guarded.return_expression = Some(Expression::IntegerLiteral(0));
        let guarded_body =
            value_body::summarize_automatic_guarded_transaction(&guarded)
                .expect("the guarded helper should have a value summary");

        let mut statements = (0..17)
            .map(|value| {
                Statement::Expression(Expression::IntegerLiteral(value))
            })
            .collect::<Vec<_>>();
        statements.extend((0..3).map(|_| {
            Statement::Expression(Expression::Call {
                name: "helper".into(),
                arguments: Vec::new(),
            })
        }));
        statements.push(Statement::If {
            condition: Expression::Call {
                name: "guarded".into(),
                arguments: Vec::new(),
            },
            then_body: vec![Statement::Return(None)],
            else_body: Vec::new(),
        });
        let caller = function("caller", Vec::new(), statements);

        let mut bodies = InlineBodySet::default();
        bodies
            .bounded_caller_bodies
            .insert(helper.name.clone(), helper);
        bodies
            .guarded_transaction_values
            .insert(guarded.name.clone(), guarded_body);
        bodies.definition_positions.insert("helper".into(), 0);
        bodies.definition_positions.insert("guarded".into(), 1);
        bodies.definition_positions.insert("caller".into(), 2);

        let expanded = bodies
            .expand_mixed_bounded_transactions(&caller)
            .expect("the large mixed caller should compose both helper families");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded.function, &mut calls);
        assert!(!calls.contains_key("helper"));
        assert!(!calls.contains_key("guarded"));
        assert!(expanded.retains_source_call_survivors);
        assert!(!expanded.advances_ordinary_ordinals);
        assert!(expanded.discounts_structured_hidden_labels);
    }

    #[test]
    fn source_liveness_expands_only_call_free_value_helpers() {
        let mut predicate = function("predicate", Vec::new(), Vec::new());
        predicate.return_type = Type::Int;
        predicate.return_expression =
            Some(Expression::Variable("predicate_value".into()));
        let mut guarded = function("guarded", Vec::new(), Vec::new());
        guarded.return_type = Type::Int;
        guarded.return_expression = Some(Expression::Call {
            name: "read_guard".into(),
            arguments: Vec::new(),
        });
        let mut bodies = InlineBodySet::default();
        bodies.values.insert(
            predicate.name.clone(),
            value_body::ValueInlineBody {
                source: predicate,
                expression: Expression::Variable("predicate_value".into()),
                automatic_transaction: false,
            },
        );
        bodies.values.insert(
            guarded.name.clone(),
            value_body::ValueInlineBody {
                source: guarded,
                expression: Expression::Call {
                    name: "read_guard".into(),
                    arguments: Vec::new(),
                },
                automatic_transaction: false,
            },
        );
        let caller = function(
            "caller",
            Vec::new(),
            vec![
                Statement::If {
                    condition: Expression::Call {
                        name: "predicate".into(),
                        arguments: Vec::new(),
                    },
                    then_body: Vec::new(),
                    else_body: Vec::new(),
                },
                Statement::If {
                    condition: Expression::Call {
                        name: "guarded".into(),
                        arguments: Vec::new(),
                    },
                    then_body: Vec::new(),
                    else_body: Vec::new(),
                },
            ],
        );

        let projected = bodies
            .expand_call_free_values_for_liveness(&caller)
            .expect("the call-free predicate should expand");

        assert!(matches!(
            &projected.statements[0],
            Statement::If {
                condition: Expression::Variable(name),
                ..
            } if name == "predicate_value"
        ));
        assert!(matches!(
            &projected.statements[1],
            Statement::If {
                condition: Expression::Call { name, .. },
                ..
            } if name == "guarded"
        ));
    }

    #[test]
    fn composes_a_repeated_guarded_call_transaction_at_an_ordinary_site() {
        let helper = function(
            "guarded",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::If {
                condition: Expression::Call {
                    name: "enabled".into(),
                    arguments: vec![Expression::Variable("value".into())],
                },
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("value".into())],
                })],
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
                name: "guarded".into(),
                arguments: vec![Expression::Variable("value".into())],
            })],
        );
        let mut sibling = caller.clone();
        sibling.name = "sibling".into();
        let mut early_return = caller.clone();
        early_return.name = "early_return".into();
        early_return.statements.insert(
            0,
            Statement::If {
                condition: Expression::Variable("value".into()),
                then_body: vec![Statement::Return(None)],
                else_body: Vec::new(),
            },
        );

        let bodies = InlineBodySet::analyze_with_definitions(
            &[helper, caller.clone(), sibling, early_return.clone()],
            &[],
        );
        assert!(bodies.expand_calls(&caller).is_none());
        let expanded = bodies
            .expand_repeatable_guarded_calls(&caller)
            .expect("the guarded transaction should inline at every ordinary site");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded.function, &mut calls);
        assert_eq!(calls.get("enabled"), Some(&1));
        assert_eq!(calls.get("consume"), Some(&1));
        assert!(!calls.contains_key("guarded"));
        assert!(bodies
            .expand_repeatable_guarded_calls(&early_return)
            .is_some());
    }

    #[test]
    fn composes_a_repeated_definition_in_a_terminal_scratch_wrapper() {
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
        let wrapper = function(
            "wrapper",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![
                Statement::Loop {
                    kind: LoopKind::DoWhile,
                    initializer: None,
                    condition: Some(Expression::IntegerLiteral(0)),
                    step: None,
                    body: Vec::new(),
                },
                Statement::Expression(Expression::Call {
                    name: "helper".into(),
                    arguments: vec![Expression::Variable("value".into())],
                }),
            ],
        );
        let sibling = function(
            "sibling",
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
            &[helper, wrapper.clone(), sibling],
            &[],
        );
        assert!(bodies.expand_calls(&wrapper).is_none());
        let expanded = bodies
            .expand_repeatable_terminal_wrapper_call(&wrapper)
            .expect("the terminal wrapper call should be eligible");
        assert!(matches!(
            expanded.function.statements.as_slice(),
            [
                Statement::Loop { body, .. },
                Statement::Expression(Expression::Call { name, arguments }),
            ] if body.is_empty()
                && name == "consume"
                && matches!(
                    arguments.as_slice(),
                    [Expression::Variable(value)] if value == "value"
                )
        ));
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
                attribute_alignment: None,
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
                attribute_alignment: None,
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

    #[test]
    fn composes_a_queue_draining_value_body_at_discarded_and_assigned_calls() {
        let callback = Parameter {
            parameter_type: Type::Pointer(Pointee::Pointer),
            name: "callback".into(),
        };
        let mut transaction = function(
            "drain",
            vec![callback.clone()],
            vec![
                Statement::Assign {
                    name: "enabled".into(),
                    value: Expression::Call {
                        name: "disable".into(),
                        arguments: Vec::new(),
                    },
                },
                Statement::Loop {
                    kind: LoopKind::While,
                    initializer: None,
                    condition: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("item".into())),
                        value: Box::new(Expression::Call {
                            name: "pop".into(),
                            arguments: Vec::new(),
                        }),
                    }),
                    step: None,
                    body: vec![Statement::Expression(Expression::Call {
                        name: "cancel".into(),
                        arguments: vec![Expression::Variable("item".into())],
                    })],
                },
                Statement::Assign {
                    name: "result".into(),
                    value: Expression::IntegerLiteral(1),
                },
            ],
        );
        transaction.return_type = Type::Int;
        transaction.locals = ["enabled", "item", "result"]
            .into_iter()
            .map(|name| {
                let mut declaration =
                    local(name, Type::Int, Expression::IntegerLiteral(0));
                declaration.initializer = None;
                declaration
            })
            .collect();
        transaction.return_expression = Some(Expression::Variable("result".into()));

        let discarded = function(
            "discarded",
            vec![callback.clone()],
            vec![Statement::Expression(Expression::Call {
                name: "drain".into(),
                arguments: vec![Expression::Variable("callback".into())],
            })],
        );
        let mut assigned = function(
            "assigned",
            vec![callback],
            vec![Statement::Assign {
                name: "outer".into(),
                value: Expression::Call {
                    name: "drain".into(),
                    arguments: vec![Expression::Variable("callback".into())],
                },
            }],
        );
        assigned.return_type = Type::Int;
        assigned.locals = vec![local(
            "outer",
            Type::Int,
            Expression::IntegerLiteral(0),
        )];
        assigned.return_expression = Some(Expression::Variable("outer".into()));

        let bodies = InlineBodySet::analyze_with_definitions(
            &[transaction, discarded.clone(), assigned.clone()],
            &[],
        );
        let discarded = bodies
            .expand_calls(&discarded)
            .expect("the discarded statement-valued call should compose");
        let assigned = bodies
            .expand_calls(&assigned)
            .expect("the assigned statement-valued call should compose");
        let mut discarded_calls = HashMap::new();
        collect_function_calls(&discarded, &mut discarded_calls);
        let mut assigned_calls = HashMap::new();
        collect_function_calls(&assigned, &mut assigned_calls);
        assert!(!discarded_calls.contains_key("drain"));
        assert!(!assigned_calls.contains_key("drain"));
        assert!(!discarded.statements.iter().any(|statement| {
            matches!(
                statement,
                Statement::Assign { name, .. }
                    if name.starts_with("__mwcc_inline_drain_") && name.ends_with("_result")
            )
        }));
        assert!(assigned.statements.iter().any(|statement| {
            matches!(
                statement,
                Statement::Assign {
                    name,
                    value: Expression::Variable(value),
                } if name == "outer" && value.starts_with("__mwcc_inline_drain_")
            )
        }));
    }

    #[test]
    fn composes_a_guarded_accumulator_into_a_pretest_loop_condition() {
        let accumulate = |call: &str| Statement::Assign {
            name: "failed".into(),
            value: Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: Box::new(Expression::Variable("failed".into())),
                right: Box::new(Expression::Call {
                    name: call.into(),
                    arguments: Vec::new(),
                }),
            },
        };
        let mut transaction = function(
            "transaction",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "final_pass".into(),
            }],
            vec![
                Statement::Loop {
                    kind: LoopKind::For,
                    initializer: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("iterator".into())),
                        value: Box::new(Expression::Variable("head".into())),
                    }),
                    condition: Some(Expression::Binary {
                        operator: BinaryOperator::NotEqual,
                        left: Box::new(Expression::Variable("iterator".into())),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    }),
                    step: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("iterator".into())),
                        value: Box::new(Expression::Variable("next".into())),
                    }),
                    body: vec![accumulate("visit")],
                },
                accumulate("finish"),
            ],
        );
        transaction.return_type = Type::Int;
        transaction.locals = vec![
            LocalDeclaration {
                declared_type: Type::Pointer(Pointee::Int),
                name: "iterator".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            },
            local("failed", Type::Int, Expression::IntegerLiteral(0)),
        ];
        transaction.guards = vec![GuardedReturn {
            condition: Expression::Variable("failed".into()),
            value: Expression::IntegerLiteral(0),
        }];
        transaction.return_expression = Some(Expression::IntegerLiteral(1));

        let caller = function(
            "caller",
            Vec::new(),
            vec![
                Statement::Loop {
                    kind: LoopKind::While,
                    initializer: None,
                    condition: Some(Expression::Unary {
                        operator: mwcc_syntax_trees::UnaryOperator::LogicalNot,
                        operand: Box::new(Expression::Call {
                            name: "transaction".into(),
                            arguments: vec![Expression::IntegerLiteral(0)],
                        }),
                    }),
                    step: None,
                    body: Vec::new(),
                },
                Statement::Expression(Expression::Call {
                    name: "transaction".into(),
                    arguments: vec![Expression::IntegerLiteral(1)],
                }),
            ],
        );

        let expanded = InlineBodySet::analyze_with_definitions(
            &[transaction, caller.clone()],
            &[],
        )
        .expand_calls(&caller)
        .expect("the guarded reduction should compose at both call sites");
        let mut calls = HashMap::new();
        collect_function_calls(&expanded, &mut calls);
        assert!(!calls.contains_key("transaction"));
        assert!(matches!(
            expanded.statements.first(),
            Some(Statement::Loop {
                condition: Some(Expression::IntegerLiteral(1)),
                body,
                ..
            }) if body.iter().any(|statement| matches!(
                statement,
                Statement::If {
                    then_body,
                    ..
                } if matches!(then_body.as_slice(), [Statement::Break])
            ))
        ));
    }
}
