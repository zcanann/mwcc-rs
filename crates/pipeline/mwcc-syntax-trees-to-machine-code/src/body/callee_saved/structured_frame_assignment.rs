//! SSA-style sinking for pure parameter reassignments in structured frames.
//!
//! MWCC keeps an incoming value in its saved home and materializes a pure
//! single-use rewrite directly into the eventual call-argument register. The
//! syntax tree records the source assignment earlier, so canonicalize that one
//! def-use edge before liveness and scheduling instead of teaching the emitter
//! to maintain a second mutable value environment.

#[allow(unused_imports)]
use super::*;

/// Sink a low-bit-clearing parameter assignment through uses which cannot
/// observe those bits. High right-shifts keep reading the original saved value;
/// a raw call argument receives the mask at that use. This is the SSA form MWCC
/// chooses on newer allocators for `x &= 0xfffff000` followed by command-byte
/// extraction and, optionally, one final raw argument.
pub(super) fn sink_low_mask_parameter_assignment(function: &Function) -> Option<Function> {
    for (assignment_index, statement) in function.statements.iter().enumerate() {
        let Statement::Assign {
            name,
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left,
                    right,
                },
        } = statement
        else {
            continue;
        };
        if !function.parameters.iter().any(|parameter| &parameter.name == name)
            || !matches!(left.as_ref(), Expression::Variable(read) if read == name)
        {
            continue;
        }
        let Some(mask_value) = constant_value(right) else {
            continue;
        };
        let mask = mask_value as u32;
        let cleared = mask.trailing_zeros();
        if cleared == 0 || mask != u32::MAX << cleared {
            continue;
        }

        let mut rewritten = function.clone();
        let mut supported = true;
        for later in rewritten.statements.iter_mut().skip(assignment_index + 1) {
            if !rewrite_low_mask_statement(later, name, mask_value, cleared) {
                supported = false;
                break;
            }
        }
        if !supported
            || rewritten
                .return_expression
                .as_ref()
                .is_some_and(|expression| expression_reads_name(expression, name))
        {
            continue;
        }
        rewritten.statements.remove(assignment_index);
        return Some(rewritten);
    }
    None
}

fn rewrite_low_mask_statement(
    statement: &mut Statement,
    name: &str,
    mask: i64,
    cleared: u32,
) -> bool {
    match statement {
        Statement::Store { target, value } => {
            !expression_reads_name(target, name) && high_shift_only(value, name, cleared)
        }
        Statement::Expression(Expression::Call { arguments, .. }) => {
            rewrite_low_mask_call_arguments(arguments, name, mask, cleared)
        }
        Statement::Assign {
            name: assigned,
            value,
        } => assigned != name && high_shift_only(value, name, cleared),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            !expression_reads_name(condition, name)
                && then_body
                    .iter_mut()
                    .all(|statement| rewrite_low_mask_statement(statement, name, mask, cleared))
                && else_body
                    .iter_mut()
                    .all(|statement| rewrite_low_mask_statement(statement, name, mask, cleared))
        }
        Statement::Return(Some(value)) => high_shift_only(value, name, cleared),
        Statement::Return(None)
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Break
        | Statement::Continue => true,
        Statement::Switch { .. } | Statement::Loop { .. } => false,
        Statement::Expression(value) => !expression_reads_name(value, name),
    }
}

fn rewrite_low_mask_call_arguments(
    arguments: &mut [Expression],
    name: &str,
    mask: i64,
    cleared: u32,
) -> bool {
    for argument in arguments {
        if matches!(argument, Expression::Variable(read) if read == name) {
            *argument = Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Variable(name.to_string())),
                right: Box::new(Expression::IntegerLiteral(mask)),
            };
        } else if !high_shift_only(argument, name, cleared) {
            return false;
        }
    }
    true
}

fn high_shift_only(expression: &Expression, name: &str, cleared: u32) -> bool {
    if !expression_reads_name(expression, name) {
        return true;
    }
    match expression {
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(read) if read == name) => {
            constant_value(right).is_some_and(|shift| shift >= i64::from(cleared))
        }
        Expression::Binary { left, right, .. } => {
            high_shift_only(left, name, cleared) && high_shift_only(right, name, cleared)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            high_shift_only(operand, name, cleared)
        }
        _ => false,
    }
}

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
