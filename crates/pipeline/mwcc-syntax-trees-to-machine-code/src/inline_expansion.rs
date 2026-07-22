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
use safety::{composable_function, stable_arguments, stable_local_values};
use std::collections::{HashMap, HashSet};
use substitution::substitute_statement;
use value_body::ValueInlineBody;

#[derive(Clone, Debug, Default)]
pub struct InlineBodySet {
    bodies: HashMap<String, Function>,
    values: HashMap<String, ValueInlineBody>,
}

pub(crate) fn legacy_frame_residue_bytes(
    function: &Function,
    facts: mwcc_syntax_trees::InlineExpansionFacts,
) -> usize {
    frame_residue::legacy_frame_residue_bytes(function, facts)
}

pub(crate) fn ordinal_residue(
    facts: mwcc_syntax_trees::InlineExpansionFacts,
    statement_body_substitutions: usize,
    value_body_substitutions: usize,
) -> u32 {
    ordinal_residue::ordinal_residue(
        facts,
        statement_body_substitutions,
        value_body_substitutions,
    )
}

pub(crate) struct ExpandedCalls {
    pub(crate) function: Function,
    pub(crate) statement_body_substitutions: usize,
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
            composable_function(function) && call_counts.get(&function.name).copied() == Some(1)
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
            }
        }
        Self { bodies, values }
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
        );
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
                )
            })
            .collect();
        let mut expanded = function.clone();
        expanded.locals = locals;
        for local in &mut expanded.locals {
            if let Some(initializer) = &local.initializer {
                local.initializer = Some(value_calls::expand_expression(
                    initializer,
                    &self.values,
                    &stable_variables,
                    &mut active,
                    &mut changed,
                    &mut value_body_substitutions,
                ));
            }
        }
        for guard in &mut expanded.guards {
            guard.condition = value_calls::expand_expression(
                &guard.condition,
                &self.values,
                &stable_variables,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
            );
            guard.value = value_calls::expand_expression(
                &guard.value,
                &self.values,
                &stable_variables,
                &mut active,
                &mut changed,
                &mut value_body_substitutions,
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
            ));
        }
        expanded.statements = statements;
        if !changed || self.calls_any(&expanded) {
            return None;
        }
        Some(ExpandedCalls {
            function: expanded,
            statement_body_substitutions,
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
    ) -> Vec<Statement> {
        let mut output = Vec::new();
        for statement in statements {
            match statement {
                Statement::Expression(Expression::Call { name, arguments })
                    if self.bodies.contains_key(name)
                        && !active.contains(name)
                        && stable_arguments(&self.bodies[name], arguments, stable_variables) =>
                {
                    let callee = &self.bodies[name];
                    if callee.parameters.len() != arguments.len() {
                        output.push(statement.clone());
                        continue;
                    }
                    let mut replacements: HashMap<String, Expression> = callee
                        .parameters
                        .iter()
                        .map(|parameter| parameter.name.as_str())
                        .zip(arguments)
                        .map(|(name, argument)| (name.to_owned(), argument.clone()))
                        .collect();
                    let callee_stable = stable_local_values(callee);
                    let mut nested_stable_variables = stable_variables.clone();
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
                    let mut substituted: Vec<_> = callee
                        .locals
                        .iter()
                        .map(|local| {
                            substitute_statement(
                                &Statement::Assign {
                                    name: local.name.clone(),
                                    value: local
                                        .initializer
                                        .clone()
                                        .expect("composable locals are initialized"),
                                },
                                &replacements,
                            )
                        })
                        .collect();
                    substituted.extend(
                        callee
                            .statements
                            .iter()
                            .map(|statement| substitute_statement(statement, &replacements)),
                    );
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
            Statement::If {
                condition: Expression::Binary { left, .. }, ..
            } if matches!(left.as_ref(), Expression::IntegerLiteral(1))
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
                target: Expression::Member { base, offset: 4, .. },
                ..
            }
        ] if matches!(base.as_ref(), Expression::MemberAddress { offset: 104, .. })));
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
    fn rejects_an_impure_argument_instead_of_duplicating_evaluation() {
        let write = function(
            "write",
            vec![Parameter {
                parameter_type: Type::Int,
                name: "value".into(),
            }],
            vec![Statement::Expression(Expression::Variable("value".into()))],
        );
        let caller = function(
            "caller",
            Vec::new(),
            vec![Statement::Expression(Expression::Call {
                name: "write".into(),
                arguments: vec![Expression::Call {
                    name: "side_effect".into(),
                    arguments: Vec::new(),
                }],
            })],
        );
        assert!(InlineBodySet::analyze(&[write])
            .expand_calls(&caller)
            .is_none());
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
    fn rejects_a_caller_value_that_can_change_during_the_inlined_body() {
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
        let expanded = bodies
            .expand_calls(&caller)
            .expect("a sole ordinary call should be an automatic-inline candidate");
        assert!(matches!(expanded.statements.as_slice(), [
            Statement::If { then_body, .. },
            Statement::Label(boundary),
        ] if matches!(then_body.as_slice(), [Statement::Goto(target)] if target == boundary)));

        let mut second_caller = caller.clone();
        second_caller.name = "second_caller".into();
        let repeated =
            InlineBodySet::analyze_with_definitions(&[helper, caller.clone(), second_caller], &[]);
        assert!(repeated.expand_calls(&caller).is_none());
    }
}
