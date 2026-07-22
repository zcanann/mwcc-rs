//! SSA-style sinking for pure parameter reassignments in structured frames.
//!
//! MWCC keeps an incoming value in its saved home and materializes a pure
//! single-use rewrite directly into the eventual call-argument register. The
//! syntax tree records the source assignment earlier, so canonicalize that one
//! def-use edge before liveness and scheduling instead of teaching the emitter
//! to maintain a second mutable value environment.

#[allow(unused_imports)]
use super::*;

pub(super) fn sink_single_use_parameter_assignment(function: &Function) -> Option<Function> {
    for (assignment_index, statement) in function.statements.iter().enumerate() {
        let Statement::Assign { name, value } = statement else {
            continue;
        };
        if !function.parameters.iter().any(|parameter| &parameter.name == name)
            || !is_pure_parameter_rewrite(value, name)
            || function
                .return_expression
                .as_ref()
                .is_some_and(|expression| expression_reads_name(expression, name))
        {
            continue;
        }

        let mut use_site = None;
        let mut rejected = false;
        for (later_index, later) in function
            .statements
            .iter()
            .enumerate()
            .skip(assignment_index + 1)
        {
            if !statement_reads_name(later, name) {
                continue;
            }
            let Statement::Expression(Expression::Call { arguments, .. }) = later else {
                rejected = true;
                break;
            };
            let matching: Vec<usize> = arguments
                .iter()
                .enumerate()
                .filter_map(|(argument_index, argument)| {
                    matches!(argument, Expression::Variable(argument) if argument == name)
                        .then_some(argument_index)
                })
                .collect();
            let [argument_index] = matching.as_slice() else {
                rejected = true;
                break;
            };
            if use_site
                .replace((later_index, *argument_index))
                .is_some()
            {
                rejected = true;
                break;
            }
        }
        if rejected {
            continue;
        }
        let Some((call_index, argument_index)) = use_site else {
            continue;
        };

        let mut rewritten = function.clone();
        let Statement::Expression(Expression::Call { arguments, .. }) =
            &mut rewritten.statements[call_index]
        else {
            unreachable!("use site was classified as a call")
        };
        arguments[argument_index] = value.clone();
        rewritten.statements.remove(assignment_index);
        return Some(rewritten);
    }
    None
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign { value, .. }
        | Statement::Expression(value)
        | Statement::Return(Some(value)) => expression_reads_name(value, name),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || then_body.iter().any(|statement| statement_reads_name(statement, name))
                || else_body.iter().any(|statement| statement_reads_name(statement, name))
        }
        Statement::Switch { .. } | Statement::Loop { .. } => true,
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

fn is_pure_parameter_rewrite(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Variable(variable) => variable == name,
        Expression::IntegerLiteral(_) => true,
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            is_pure_parameter_rewrite(operand, name)
        }
        Expression::Binary { left, right, .. } => {
            is_pure_parameter_rewrite(left, name)
                && is_pure_parameter_rewrite(right, name)
                && expression_reads_name(expression, name)
        }
        _ => false,
    }
}
