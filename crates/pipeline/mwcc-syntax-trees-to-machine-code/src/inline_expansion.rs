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
use mwcc_syntax_trees::{Expression, Function, Statement};
use returns::rewrite_inline_returns;
use safety::{
    automatic_composable_function, composable_function, materializable_arguments,
    stable_argument, stable_arguments, stable_local_values,
};
use std::collections::{HashMap, HashSet};
use substitution::substitute_statement;
use value_body::ValueInlineBody;

#[derive(Clone, Debug, Default)]
pub struct InlineBodySet {
    bodies: HashMap<String, Function>,
    values: HashMap<String, ValueInlineBody>,
    required: HashSet<String>,
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
        let required: HashSet<String> = skipped
            .iter()
            .map(|function| function.name.clone())
            .collect();
        let mut call_counts = HashMap::<String, usize>::new();
        for function in definitions.iter().chain(skipped) {
            collect_function_calls(function, &mut call_counts);
        }
        let mut bodies = HashMap::new();
        for function in skipped
            .iter()
            .filter(|function| composable_function(function))
        {
            bodies.insert(function.name.clone(), function.clone());
        }
        for function in definitions.iter().filter(|function| {
            automatic_composable_function(function)
                && call_counts.get(&function.name).copied() == Some(1)
        }) {
            bodies
                .entry(function.name.clone())
                .or_insert_with(|| function.clone());
        }
        let mut values: HashMap<_, _> = skipped
            .iter()
            .filter_map(|function| {
                value_body::summarize(function).map(|body| (function.name.clone(), body))
            })
            .collect();
        for function in definitions {
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
            bodies,
            values,
            required,
        }
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
        function
            .locals
            .iter()
            .filter_map(|local| local.initializer.as_ref())
            .any(|expression| self.expression_contains_call(expression))
            || function.guards.iter().any(|guard| {
                self.expression_contains_call(&guard.condition)
                    || self.expression_contains_call(&guard.value)
            })
            || function
                .return_expression
                .as_ref()
                .is_some_and(|expression| self.expression_contains_call(expression))
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

    pub(crate) fn expand_calls_with_facts(&self, function: &Function) -> Option<ExpandedCalls> {
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
        expanded.statements = statements;
        let calls_remain = self.calls_any(&expanded);
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
    ) -> Vec<Statement> {
        let mut output = Vec::new();
        for statement in statements {
            match statement {
                Statement::Expression(Expression::Call { name, arguments })
                    if self.bodies.contains_key(name)
                        && !active.contains(name)
                        && (stable_arguments(
                            &self.bodies[name],
                            arguments,
                            stable_variables,
                        ) || materializable_arguments(
                            &self.bodies[name],
                            arguments,
                            stable_variables,
                        )) =>
                {
                    let callee = &self.bodies[name];
                    if callee.parameters.len() != arguments.len() {
                        output.push(statement.clone());
                        continue;
                    }
                    let callee_stable = stable_local_values(callee);
                    let mut nested_stable_variables = stable_variables.clone();
                    let materialize =
                        !stable_arguments(callee, arguments, stable_variables);
                    let mut replacements = HashMap::new();
                    let mut substituted = Vec::new();
                    for (parameter, argument) in callee.parameters.iter().zip(arguments) {
                        if !materialize || stable_argument(argument, stable_variables) {
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
                    if !callee_calls.is_empty() {
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
                    ),
                }),
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
            Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {
                false
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, LocalDeclaration, Parameter, Pointee, Type};

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
        let expanded = bodies
            .expand_calls(&caller)
            .expect("a sole ordinary call should be an automatic-inline candidate");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::If { then_body, .. },
            Statement::Label(boundary),
        ] if matches!(then_body.as_slice(), [Statement::Goto(target)] if target == boundary)));

        let required = InlineBodySet::analyze(&[helper.clone()]);
        assert!(required.calls_required(&caller));

        let mut second_caller = caller.clone();
        second_caller.name = "second_caller".into();
        let repeated =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone(), second_caller], &[]);
        assert!(repeated.expand_calls(&caller).is_none());
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
