//! Shared semantic probes for macro-expanded display-list packet builders.
//!
//! Nintendo's graphics macros leave deeply casted integer expressions and
//! pointer aliases in the syntax tree.  Schedulers should compare their values
//! and parameter dependencies without each owning a slightly different
//! constant folder.

use super::*;

pub(super) fn parameter_reads(
    expression: &Expression,
    parameters: &[&mwcc_syntax_trees::Parameter],
) -> Vec<usize> {
    parameters
        .iter()
        .map(|parameter| count_name_occurrences(expression, &parameter.name))
        .collect()
}

#[derive(Clone, Copy)]
enum PacketConstant {
    Integer(i64),
    Float(f64),
}

pub(super) fn constant_u32(expression: &Expression) -> Option<u32> {
    match constant(expression)? {
        PacketConstant::Integer(value) => Some(value as u32),
        PacketConstant::Float(_) => None,
    }
}

fn constant(expression: &Expression) -> Option<PacketConstant> {
    match expression {
        Expression::IntegerLiteral(value) => Some(PacketConstant::Integer(*value)),
        Expression::FloatLiteral(value) => Some(PacketConstant::Float(*value)),
        Expression::Cast {
            target_type,
            operand,
        } => {
            let value = constant(operand)?;
            match target_type {
                Type::Float => Some(PacketConstant::Float(match value {
                    PacketConstant::Integer(value) => (value as f32) as f64,
                    PacketConstant::Float(value) => (value as f32) as f64,
                })),
                Type::Double => Some(PacketConstant::Float(match value {
                    PacketConstant::Integer(value) => value as f64,
                    PacketConstant::Float(value) => value,
                })),
                Type::Int
                | Type::UnsignedInt
                | Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort => Some(PacketConstant::Integer(match value {
                    PacketConstant::Integer(value) => value,
                    PacketConstant::Float(value) => value.trunc() as i64,
                })),
                _ => None,
            }
        }
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let left = constant(left)?;
            let right = constant(right)?;
            match (left, right) {
                (PacketConstant::Integer(left), PacketConstant::Integer(right)) => {
                    let value = match operator {
                        BinaryOperator::Add => left.wrapping_add(right),
                        BinaryOperator::Subtract => left.wrapping_sub(right),
                        BinaryOperator::Multiply => left.wrapping_mul(right),
                        BinaryOperator::BitAnd => left & right,
                        BinaryOperator::BitOr => left | right,
                        BinaryOperator::ShiftLeft => left.wrapping_shl(u32::try_from(right).ok()?),
                        BinaryOperator::ShiftRight => left.wrapping_shr(u32::try_from(right).ok()?),
                        _ => return None,
                    };
                    Some(PacketConstant::Integer(value))
                }
                (left, right) => {
                    let number = |value| match value {
                        PacketConstant::Integer(value) => value as f64,
                        PacketConstant::Float(value) => value,
                    };
                    let left = number(left);
                    let right = number(right);
                    Some(PacketConstant::Float(match operator {
                        BinaryOperator::Add => left + right,
                        BinaryOperator::Subtract => left - right,
                        BinaryOperator::Multiply => left * right,
                        _ => return None,
                    }))
                }
            }
        }
        _ => None,
    }
}

pub(super) fn count_float_literal(expression: &Expression, expected: f64) -> usize {
    match expression {
        Expression::FloatLiteral(value) => usize::from(*value == expected),
        Expression::Cast { operand, .. } | Expression::Unary { operand, .. } => {
            count_float_literal(operand, expected)
        }
        Expression::Binary { left, right, .. } => {
            count_float_literal(left, expected) + count_float_literal(right, expected)
        }
        _ => 0,
    }
}

pub(super) fn integer_literals(expression: &Expression) -> Vec<i64> {
    let mut values = Vec::new();
    collect_integer_literals(expression, &mut values);
    values
}

fn collect_integer_literals(expression: &Expression, values: &mut Vec<i64>) {
    match expression {
        Expression::IntegerLiteral(value) => values.push(*value),
        Expression::Cast { operand, .. } | Expression::Unary { operand, .. } => {
            collect_integer_literals(operand, values);
        }
        Expression::Binary { left, right, .. } => {
            collect_integer_literals(left, values);
            collect_integer_literals(right, values);
        }
        _ => {}
    }
}
