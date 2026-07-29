//! Legacy optimizer frame state left behind by value-returning inline calls.
//!
//! The parser substitutes safe single-return inlines immediately, so their
//! calls no longer exist in the function AST. This module combines retained
//! parser provenance with the surviving body shape and leaves ABI placement to
//! the frame-convention owner.

use mwcc_syntax_trees::{Expression, Function, InlineExpansionFacts, Statement};

pub(super) fn legacy_frame_residue_bytes(
    function: &Function,
    facts: InlineExpansionFacts,
) -> usize {
    let initializer_values = facts.leading_initializer_substitutions;
    let initializer_bytes = if initializer_values >= 2
        && has_memory_mutation_before_surviving_call(&function.statements)
    {
        // GC/1.2.5n's value graph assigns one eight-byte allocator lane to
        // every retained initializer result. The normal callee-saved frame
        // lane remains independently owned by the ABI policy.
        initializer_values * 8
    } else {
        0
    };
    let uninitialized_pointers = uninitialized_pointer_names(function);
    let body_address_lanes = count_derived_address_assignments_before_call(
        &function.statements,
        &uninitialized_pointers,
    )
    .min(facts.body_value_substitutions);
    initializer_bytes + body_address_lanes * 8
}

pub(super) fn legacy_statement_body_frame_residue_bytes(
    function: &Function,
    substitutions: usize,
) -> usize {
    if substitutions == 0 || !has_statement_body_frame_residue(&function.statements) {
        return 0;
    }
    substitutions * 8
}

pub(super) fn legacy_value_body_frame_residue_bytes(
    function: &Function,
    substitutions: usize,
) -> usize {
    if substitutions == 0 {
        return 0;
    }
    let uninitialized_pointers = uninitialized_pointer_names(function);
    let retained_addresses = count_derived_address_assignments_before_call(
        &function.statements,
        &uninitialized_pointers,
    );
    retained_addresses.min(substitutions) * 8
}

fn uninitialized_pointer_names(function: &Function) -> std::collections::HashSet<&str> {
    function
        .locals
        .iter()
        .filter(|local| {
            local.initializer.is_none()
                && matches!(
                    local.declared_type,
                    mwcc_syntax_trees::Type::Pointer(_)
                        | mwcc_syntax_trees::Type::StructPointer { .. }
                )
        })
        .map(|local| local.name.as_str())
        .collect()
}

fn count_derived_address_assignments_before_call(
    statements: &[Statement],
    uninitialized_pointers: &std::collections::HashSet<&str>,
) -> usize {
    let mut retained = 0;
    for (index, statement) in statements.iter().enumerate() {
        if let Statement::Assign { name, value } = statement {
            if uninitialized_pointers.contains(name.as_str())
                && is_derived_address(value)
                && statements[index + 1..].iter().any(statement_contains_call)
            {
                retained += 1;
            }
        }
        match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                retained += count_derived_address_assignments_before_call(
                    then_body,
                    uninitialized_pointers,
                );
                retained += count_derived_address_assignments_before_call(
                    else_body,
                    uninitialized_pointers,
                );
            }
            Statement::Loop { body, .. } => {
                retained +=
                    count_derived_address_assignments_before_call(body, uninitialized_pointers);
            }
            Statement::Switch {
                arms,
                default,
                ..
            } => {
                retained += arms
                    .iter()
                    .map(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Return(_) => 0,
                        mwcc_syntax_trees::ArmBody::Statements(body) => {
                            count_derived_address_assignments_before_call(
                                body,
                                uninitialized_pointers,
                            )
                        }
                    })
                    .sum::<usize>();
                retained += default.as_ref().map_or(0, |arm| match arm {
                    mwcc_syntax_trees::ArmBody::Return(_) => 0,
                    mwcc_syntax_trees::ArmBody::Statements(body) => {
                        count_derived_address_assignments_before_call(
                            body,
                            uninitialized_pointers,
                        )
                    }
                });
            }
            _ => {}
        }
    }
    retained
}

fn is_derived_address(expression: &Expression) -> bool {
    match expression {
        Expression::MemberAddress {
            index_stride: None,
            ..
        } => true,
        Expression::AddressOf { operand } => matches!(
            operand.as_ref(),
            Expression::Member {
                member_type: mwcc_syntax_trees::Type::Struct { .. },
                index_stride: None,
                ..
            }
        ),
        _ => false,
    }
}

fn has_statement_body_frame_residue(statements: &[Statement]) -> bool {
    has_top_level_memory_mutation_and_call(statements)
        || statements.iter().any(|statement| match statement {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                has_statement_body_frame_residue(then_body)
                    || has_statement_body_frame_residue(else_body)
            }
            Statement::Loop { body, .. } => has_statement_body_frame_residue(body),
            Statement::Switch {
                arms,
                default,
                ..
            } => {
                arms.iter().any(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(_) => false,
                    mwcc_syntax_trees::ArmBody::Statements(body) => {
                        has_statement_body_frame_residue(body)
                    }
                }) || default.as_ref().is_some_and(|body| match body {
                    mwcc_syntax_trees::ArmBody::Return(_) => false,
                    mwcc_syntax_trees::ArmBody::Statements(body) => {
                        has_statement_body_frame_residue(body)
                    }
                })
            }
            _ => false,
        })
}

fn has_top_level_memory_mutation_and_call(statements: &[Statement]) -> bool {
    has_memory_mutation_before_surviving_call(statements)
        || (statements.iter().any(statement_contains_call)
            && statements.iter().any(|statement| {
                matches!(statement, Statement::Expression(expression)
                    if expression_contains_memory_mutation(expression))
            }))
}

fn has_memory_mutation_before_surviving_call(statements: &[Statement]) -> bool {
    let mut saw_memory_mutation = false;
    for statement in statements {
        if statement_contains_call(statement) {
            return saw_memory_mutation;
        }
        saw_memory_mutation |= matches!(statement, Statement::Store { .. });
    }
    false
}

fn statement_contains_call(statement: &Statement) -> bool {
    match statement {
        Statement::InlineAsm(_) => crate::analysis::statement_has_call(statement),
        Statement::Store { target, value } => {
            expression_contains_call(target) || expression_contains_call(value)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            expression_contains_call(value)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_contains_call(condition)
                || then_body.iter().any(statement_contains_call)
                || else_body.iter().any(statement_contains_call)
        }
        Statement::Return(value) => value.as_ref().is_some_and(expression_contains_call),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_contains_call(scrutinee)
                || arms.iter().any(|arm| arm_contains_call(&arm.body))
                || default.as_ref().is_some_and(arm_contains_call)
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            [initializer, condition, step]
                .into_iter()
                .flatten()
                .any(expression_contains_call)
                || body.iter().any(statement_contains_call)
        }
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => false,
    }
}

fn arm_contains_call(arm: &mwcc_syntax_trees::ArmBody) -> bool {
    match arm {
        mwcc_syntax_trees::ArmBody::Return(value) => expression_contains_call(value),
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            statements.iter().any(statement_contains_call)
        }
    }
}

fn expression_contains_call(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. } => true,
        Expression::AggregateLiteral(elements) => elements.iter().any(expression_contains_call),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            expression_contains_call(left) || expression_contains_call(right)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_contains_call(condition)
                || expression_contains_call(when_true)
                || expression_contains_call(when_false)
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
        }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => expression_contains_call(operand),
        Expression::Index { base, index } => {
            expression_contains_call(base) || expression_contains_call(index)
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

fn expression_contains_memory_mutation(expression: &Expression) -> bool {
    match expression {
        Expression::Assign { target, value } => {
            !matches!(target.as_ref(), Expression::Variable(_))
                || expression_contains_memory_mutation(value)
        }
        Expression::Comma { left, right } | Expression::Binary { left, right, .. } => {
            expression_contains_memory_mutation(left)
                || expression_contains_memory_mutation(right)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_contains_memory_mutation(condition)
                || expression_contains_memory_mutation(when_true)
                || expression_contains_memory_mutation(when_false)
        }
        Expression::AggregateLiteral(elements) => {
            elements.iter().any(expression_contains_memory_mutation)
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
        }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => {
            expression_contains_memory_mutation(operand)
        }
        Expression::Index { base, index } => {
            expression_contains_memory_mutation(base)
                || expression_contains_memory_mutation(index)
        }
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. }
        | Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
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

    fn call(name: &str) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.into(),
            arguments: Vec::new(),
        })
    }

    fn two_initializers() -> InlineExpansionFacts {
        InlineExpansionFacts {
            leading_initializer_substitutions: 2,
            body_value_substitutions: 0,
        }
    }

    #[test]
    fn retains_one_lane_per_initializer_across_a_pre_call_store() {
        let function = function(vec![
            Statement::Store {
                target: Expression::Variable("memory".into()),
                value: Expression::IntegerLiteral(0),
            },
            call("external"),
        ]);
        assert_eq!(
            legacy_frame_residue_bytes(&function, two_initializers()),
            16
        );
    }

    #[test]
    fn does_not_retain_initializer_lanes_without_the_intervening_store() {
        let function = function(vec![call("external")]);
        assert_eq!(legacy_frame_residue_bytes(&function, two_initializers()), 0);
    }

    #[test]
    fn retains_one_lane_for_a_statement_body_before_a_surviving_call() {
        let function = function(vec![
            Statement::Store {
                target: Expression::Variable("memory".into()),
                value: Expression::IntegerLiteral(0),
            },
            call("external"),
        ]);
        assert_eq!(legacy_statement_body_frame_residue_bytes(&function, 1), 8);
    }

    #[test]
    fn retains_one_lane_for_an_inlined_address_accessor_before_a_call() {
        let pointer = Type::StructPointer { element_size: 64 };
        let mut function = function(vec![Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![
                Statement::Assign {
                    name: "alias".into(),
                    value: Expression::MemberAddress {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 16,
                        element: mwcc_syntax_trees::Pointee::UnsignedInt,
                        index_stride: None,
                    },
                },
                call("external"),
            ],
            else_body: Vec::new(),
        }]);
        function.locals.push(mwcc_syntax_trees::LocalDeclaration {
            declared_type: pointer,
            name: "alias".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        assert_eq!(legacy_value_body_frame_residue_bytes(&function, 1), 8);
    }

    #[test]
    fn retains_one_lane_for_a_top_level_assignment_after_a_surviving_call() {
        let function = function(vec![
            call("external"),
            Statement::Expression(Expression::Assign {
                target: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("memory".into())),
                    offset: 0,
                    member_type: Type::Int,
                    index_stride: None,
                }),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
        ]);
        assert_eq!(legacy_statement_body_frame_residue_bytes(&function, 1), 8);
    }

    #[test]
    fn retains_statement_lanes_for_mutation_before_a_call_in_a_nested_arm() {
        let function = function(vec![Statement::If {
            condition: Expression::Variable("condition".into()),
            then_body: vec![
                Statement::Store {
                    target: Expression::Variable("memory".into()),
                    value: Expression::IntegerLiteral(0),
                },
                call("external"),
            ],
            else_body: Vec::new(),
        }]);
        assert_eq!(legacy_statement_body_frame_residue_bytes(&function, 2), 16);
    }

    #[test]
    fn ignores_a_top_level_store_after_an_unrelated_call() {
        let function = function(vec![
            call("external"),
            Statement::Store {
                target: Expression::Variable("memory".into()),
                value: Expression::IntegerLiteral(0),
            },
        ]);
        assert_eq!(legacy_statement_body_frame_residue_bytes(&function, 1), 0);
    }

}
