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
    if initializer_values < 2 || !has_memory_mutation_before_surviving_call(&function.statements) {
        return 0;
    }

    // GC/1.2.5n's value graph assigns one eight-byte allocator lane to every
    // retained initializer result. The normal callee-saved frame lane remains
    // independently owned by the ABI policy.
    initializer_values * 8
}

pub(super) fn legacy_statement_body_frame_residue_bytes(
    function: &Function,
    substitutions: usize,
) -> usize {
    if substitutions == 0 || !has_top_level_memory_mutation_and_call(&function.statements) {
        return 0;
    }
    substitutions * 8
}

fn has_top_level_memory_mutation_and_call(statements: &[Statement]) -> bool {
    statements.iter().any(statement_contains_call)
        && statements.iter().any(|statement| match statement {
            Statement::Store { .. } => true,
            Statement::Expression(expression) => expression_contains_memory_mutation(expression),
            _ => false,
        })
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
    fn retains_one_lane_for_a_top_level_statement_body_after_a_surviving_call() {
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
    fn ignores_nested_mutation_from_an_unrelated_control_flow_call() {
        let function = function(vec![Statement::If {
            condition: Expression::Call {
                name: "external".into(),
                arguments: Vec::new(),
            },
            then_body: vec![Statement::Store {
                target: Expression::Variable("memory".into()),
                value: Expression::IntegerLiteral(0),
            }],
            else_body: Vec::new(),
        }]);
        assert_eq!(legacy_statement_body_frame_residue_bytes(&function, 1), 0);
    }

}
