//! Scalar temporaries materialized while analyzing discarded C++ inlines.
//!
//! Legacy mwcceppc assigns anonymous ordinals to rvalues bound to scalar
//! `const T&` parameters, even when the containing inline body is later
//! discarded.  The parser retains both resolved call identities and their
//! source reference masks; this pass counts the semantic binding sites without
//! mixing compiler-version weights into parsing.

use mwcc_syntax_trees::{
    ArmBody, Expression, Function, GuardedReturn, LocalDeclaration, Statement, TranslationUnit,
};
use std::collections::{HashMap, HashSet};

pub fn count(unit: &TranslationUnit) -> usize {
    let mut seen = HashSet::new();
    unit.skipped_inline_definitions
        .iter()
        .filter(|function| seen.insert(function.name.as_str()))
        .map(|function| {
            count_function(
                function,
                &unit.cxx_const_reference_parameter_positions,
            )
        })
        .sum()
}

fn count_function(function: &Function, bindings: &HashMap<String, Vec<bool>>) -> usize {
    function
        .locals
        .iter()
        .map(|local| count_local(local, bindings))
        .sum::<usize>()
        + function
            .statements
            .iter()
            .map(|statement| count_statement(statement, bindings))
            .sum::<usize>()
        + function
            .guards
            .iter()
            .map(|guard| count_guard(guard, bindings))
            .sum::<usize>()
        + function
            .return_expression
            .as_ref()
            .map_or(0, |expression| count_expression(expression, bindings))
}

fn count_local(local: &LocalDeclaration, bindings: &HashMap<String, Vec<bool>>) -> usize {
    local
        .initializer
        .as_ref()
        .map_or(0, |expression| count_expression(expression, bindings))
}

fn count_guard(guard: &GuardedReturn, bindings: &HashMap<String, Vec<bool>>) -> usize {
    count_expression(&guard.condition, bindings) + count_expression(&guard.value, bindings)
}

fn count_arm(arm: &ArmBody, bindings: &HashMap<String, Vec<bool>>) -> usize {
    match arm {
        ArmBody::Return(expression) => count_expression(expression, bindings),
        ArmBody::Statements(statements) => statements
            .iter()
            .map(|statement| count_statement(statement, bindings))
            .sum(),
    }
}

fn count_statement(statement: &Statement, bindings: &HashMap<String, Vec<bool>>) -> usize {
    match statement {
        Statement::Store { target, value } => {
            count_expression(target, bindings) + count_expression(value, bindings)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            count_expression(value, bindings)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            count_expression(condition, bindings)
                + then_body
                    .iter()
                    .map(|statement| count_statement(statement, bindings))
                    .sum::<usize>()
                + else_body
                    .iter()
                    .map(|statement| count_statement(statement, bindings))
                    .sum::<usize>()
        }
        Statement::Return(expression) => expression
            .as_ref()
            .map_or(0, |expression| count_expression(expression, bindings)),
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            count_expression(scrutinee, bindings)
                + arms
                    .iter()
                    .map(|arm| count_arm(&arm.body, bindings))
                    .sum::<usize>()
                + default
                    .as_ref()
                    .map_or(0, |arm| count_arm(arm, bindings))
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
                .map_or(0, |expression| count_expression(expression, bindings))
                + condition
                    .as_ref()
                    .map_or(0, |expression| count_expression(expression, bindings))
                + step
                    .as_ref()
                    .map_or(0, |expression| count_expression(expression, bindings))
                + body
                    .iter()
                    .map(|statement| count_statement(statement, bindings))
                    .sum::<usize>()
        }
        Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => 0,
    }
}

fn count_call_bindings(
    name: &str,
    arguments: &[Expression],
    bindings: &HashMap<String, Vec<bool>>,
) -> usize {
    let Some(mask) = bindings.get(name) else {
        return 0;
    };
    let mask = if mask.len() == arguments.len() + 1 && mask.first() == Some(&false) {
        &mask[1..]
    } else {
        mask.as_slice()
    };
    arguments
        .iter()
        .zip(mask)
        .filter(|(argument, binds)| **binds && !is_lvalue(argument))
        .count()
}

fn is_lvalue(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Variable(_)
            | Expression::Dereference { .. }
            | Expression::Index { .. }
            | Expression::Member { .. }
    )
}

fn count_expression(expression: &Expression, bindings: &HashMap<String, Vec<bool>>) -> usize {
    match expression {
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => 0,
        Expression::AggregateLiteral(elements) => elements
            .iter()
            .map(|element| count_expression(element, bindings))
            .sum(),
        Expression::Binary { left, right, .. }
        | Expression::Assign {
            target: left,
            value: right,
        }
        | Expression::Comma { left, right } => {
            count_expression(left, bindings) + count_expression(right, bindings)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::PostStep {
            target: operand, ..
        } => count_expression(operand, bindings),
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            count_expression(condition, bindings)
                + count_expression(when_true, bindings)
                + count_expression(when_false, bindings)
        }
        Expression::BitFieldRead {
            extracted, storage, ..
        } => count_expression(extracted, bindings) + count_expression(storage, bindings),
        Expression::Index { base, index } => {
            count_expression(base, bindings) + count_expression(index, bindings)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            count_expression(base, bindings)
        }
        Expression::CallThrough { target, arguments } => {
            count_expression(target, bindings)
                + arguments
                    .iter()
                    .map(|argument| count_expression(argument, bindings))
                    .sum::<usize>()
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            count_expression(object, bindings)
                + arguments
                    .iter()
                    .map(|argument| count_expression(argument, bindings))
                    .sum::<usize>()
        }
        Expression::ConstructedNew {
            allocation,
            constructor,
            arguments,
            ..
        } => {
            count_expression(allocation, bindings)
                + count_call_bindings(constructor, arguments, bindings)
                + arguments
                    .iter()
                    .map(|argument| count_expression(argument, bindings))
                    .sum::<usize>()
        }
        Expression::Call { name, arguments } => {
            count_call_bindings(name, arguments, bindings)
                + arguments
                    .iter()
                    .map(|argument| count_expression(argument, bindings))
                    .sum::<usize>()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::BinaryOperator;

    #[test]
    fn counts_only_rvalues_at_const_reference_positions() {
        let bindings = HashMap::from([(
            "set__1VFRCfRCf".to_string(),
            vec![false, true, true],
        )]);
        let expression = Expression::Call {
            name: "set__1VFRCfRCf".to_string(),
            arguments: vec![
                Expression::Variable("this".to_string()),
                Expression::Variable("plain".to_string()),
                Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("left".to_string())),
                    right: Box::new(Expression::Variable("right".to_string())),
                },
            ],
        };

        assert_eq!(count_expression(&expression, &bindings), 1);
    }
}
