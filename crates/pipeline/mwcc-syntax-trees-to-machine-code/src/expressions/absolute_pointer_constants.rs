//! Fold literal addresses while retaining the pointee stride through arithmetic.
//!
//! The untyped integer folder cannot erase pointer casts: `(int*)4 + 1`
//! advances four bytes, whereas `(unsigned)(int*)4 + 1` advances one.

use mwcc_syntax_trees::{BinaryOperator, Expression, Type};

pub(super) fn value(expression: &Expression) -> Option<i64> {
    let (bits, _, contains_pointer) = typed_value(expression)?;
    contains_pointer.then_some(bits as i32 as i64)
}

fn typed_value(expression: &Expression) -> Option<(u32, Option<u32>, bool)> {
    match expression {
        Expression::IntegerLiteral(value) => Some((*value as u32, None, false)),
        Expression::Cast {
            target_type,
            operand,
        } => {
            let (bits, _, contains_pointer) = typed_value(operand)?;
            match target_type {
                Type::Pointer(pointee) => Some((bits, Some(u32::from(pointee.size())), true)),
                Type::StructPointer { element_size } if *element_size != 0 => {
                    Some((bits, Some(*element_size), true))
                }
                _ => Some((
                    crate::analysis::convert_integer_constant(bits as i64, *target_type)? as u32,
                    None,
                    contains_pointer,
                )),
            }
        }
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let (left, left_stride, left_pointer) = typed_value(left)?;
            let (right, right_stride, right_pointer) = typed_value(right)?;
            let (bits, stride) = match (operator, left_stride, right_stride) {
                (BinaryOperator::Add, Some(stride), None) => {
                    (left.wrapping_add(right.wrapping_mul(stride)), Some(stride))
                }
                (BinaryOperator::Add, None, Some(stride)) => {
                    (right.wrapping_add(left.wrapping_mul(stride)), Some(stride))
                }
                (BinaryOperator::Subtract, Some(stride), None) => {
                    (left.wrapping_sub(right.wrapping_mul(stride)), Some(stride))
                }
                (BinaryOperator::Subtract, Some(left_stride), Some(right_stride))
                    if left_stride == right_stride && left_stride != 0 =>
                {
                    (
                        ((left.wrapping_sub(right) as i32 as i64) / left_stride as i64) as u32,
                        None,
                    )
                }
                (BinaryOperator::Add, None, None) => (left.wrapping_add(right), None),
                (BinaryOperator::Subtract, None, None) => (left.wrapping_sub(right), None),
                _ => return None,
            };
            Some((bits, stride, left_pointer || right_pointer))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Pointee;

    fn cast(target_type: Type, operand: Expression) -> Expression {
        Expression::Cast {
            target_type,
            operand: Box::new(operand),
        }
    }

    #[test]
    fn retains_stride_until_an_integer_cast() {
        let pointer = cast(
            Type::Pointer(Pointee::Int),
            Expression::IntegerLiteral(0xfffffff8),
        );
        let add = |left| Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(left),
            right: Box::new(Expression::IntegerLiteral(3)),
        };
        assert_eq!(value(&add(pointer.clone())), Some(4));
        assert_eq!(value(&add(cast(Type::UnsignedInt, pointer))), Some(-5));
        assert_eq!(value(&add(Expression::IntegerLiteral(4))), None);
        assert_eq!(
            value(&add(cast(
                Type::Pointer(Pointee::Int),
                Expression::Call {
                    name: "address".into(),
                    arguments: Vec::new(),
                }
            ))),
            None
        );
    }
}
