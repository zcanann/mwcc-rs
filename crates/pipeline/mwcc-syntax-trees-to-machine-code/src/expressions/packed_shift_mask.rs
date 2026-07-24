//! Normalization of macro-expanded packed integer fields.
//!
//! Display-list macros commonly leave several adjacent unsigned casts, masks,
//! and left shifts around one scalar. PowerPC can express the complete pipeline
//! with one `rlwinm`; recognizing only one parent/child pair leaves redundant
//! `slwi` and `clrlwi` instructions in high-pressure packet builders.

use super::*;

#[derive(Debug, Clone, Copy)]
struct PackedShiftMask<'a> {
    source: &'a Expression,
    shift: u8,
    mask: u32,
    possible_sign_fill: u32,
    operations: usize,
}

impl Generator {
    pub(crate) fn try_emit_packed_shift_mask(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(pipeline) = decompose(expression) else {
            return Ok(false);
        };
        // One parent/child pair is already owned by the ordinary rotate-mask
        // selector and by measured packet schedules built around it. This pass
        // owns only deeper macro residue that those selectors cannot collapse.
        if pipeline.operations < self.packed_shift_mask_min_operations
            || pipeline.possible_sign_fill != 0
            || constant_value(pipeline.source).is_some()
        {
            return Ok(false);
        }
        let Some((begin, end)) = mask_to_run(pipeline.mask) else {
            return Ok(false);
        };

        let source = if let Some(register) =
            leaf_name(pipeline.source).and_then(|name| self.lookup_general(name))
        {
            register
        } else {
            self.evaluate_general(pipeline.source, destination)?;
            destination
        };
        self.output.instructions.push(Instruction::RotateAndMask {
            a: destination,
            s: source,
            shift: pipeline.shift,
            begin,
            end,
        });
        Ok(true)
    }
}

fn decompose(expression: &Expression) -> Option<PackedShiftMask<'_>> {
    match expression {
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => decompose(operand),
        Expression::Cast {
            target_type: Type::UnsignedShort | Type::UnsignedChar,
            operand,
        } => {
            let mut pipeline = decompose(operand)?;
            let width = match expression {
                Expression::Cast { target_type, .. } => target_type.width(),
                _ => unreachable!("the unsigned narrow cast was matched"),
            };
            pipeline.mask &= (1u32 << width) - 1;
            pipeline.possible_sign_fill &= (1u32 << width) - 1;
            pipeline.operations += 1;
            Some(pipeline)
        }
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } => {
            let (inner, mask) = if let Some(mask) = constant_value(right) {
                (left.as_ref(), mask as u32)
            } else {
                (right.as_ref(), constant_value(left)? as u32)
            };
            let mut pipeline = decompose(inner)?;
            pipeline.mask &= mask;
            pipeline.possible_sign_fill &= mask;
            pipeline.operations += 1;
            Some(pipeline)
        }
        Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left,
            right,
        } => {
            let amount = u8::try_from(constant_value(right)?).ok()?;
            if amount == 0 {
                return decompose(left);
            }
            if amount >= 32 {
                return None;
            }
            apply_left_shift(decompose(left)?, amount)
        }
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } => {
            let (inner, factor) = if let Some(factor) = constant_value(right) {
                (left.as_ref(), u32::try_from(factor).ok()?)
            } else {
                (right.as_ref(), u32::try_from(constant_value(left)?).ok()?)
            };
            if !factor.is_power_of_two() {
                return None;
            }
            let amount = factor.trailing_zeros() as u8;
            if amount == 0 {
                return decompose(inner);
            }
            apply_left_shift(decompose(inner)?, amount)
        }
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left,
            right,
        } => {
            let amount = u8::try_from(constant_value(right)?).ok()?;
            if amount == 0 {
                return decompose(left);
            }
            if amount >= 32 {
                return None;
            }
            let mut pipeline = decompose(left)?;
            pipeline.shift = pipeline.shift.wrapping_add(32 - amount) % 32;
            pipeline.mask >>= amount;
            pipeline.possible_sign_fill =
                (pipeline.possible_sign_fill >> amount) | (u32::MAX << (32 - amount));
            pipeline.operations += 1;
            Some(pipeline)
        }
        source => Some(PackedShiftMask {
            source,
            shift: 0,
            mask: u32::MAX,
            possible_sign_fill: 0,
            operations: 0,
        }),
    }
}

fn apply_left_shift(mut pipeline: PackedShiftMask<'_>, amount: u8) -> Option<PackedShiftMask<'_>> {
    pipeline.shift = pipeline.shift.wrapping_add(amount) % 32;
    pipeline.mask <<= amount;
    pipeline.possible_sign_fill <<= amount;
    pipeline.operations += 1;
    Some(pipeline)
}

pub(crate) fn is_shallow_packed_shift_mask_expression(expression: &Expression) -> bool {
    decompose(expression).is_some_and(|pipeline| {
        pipeline.operations == 2
            && pipeline.possible_sign_fill == 0
            && constant_value(pipeline.source).is_none()
            && mask_to_run(pipeline.mask).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shift(left: Expression, amount: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: Box::new(left),
            right: Box::new(Expression::IntegerLiteral(amount)),
        }
    }

    fn mask(left: Expression, value: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(left),
            right: Box::new(Expression::IntegerLiteral(value)),
        }
    }

    fn shift_right(left: Expression, amount: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: Box::new(left),
            right: Box::new(Expression::IntegerLiteral(amount)),
        }
    }

    fn multiply(left: Expression, factor: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(left),
            right: Box::new(Expression::IntegerLiteral(factor)),
        }
    }

    #[test]
    fn combines_a_macro_expanded_shift_mask_pipeline() {
        let expression = shift(
            mask(
                shift(shift(Expression::Variable("uls".into()), 1), 2),
                0xfff,
            ),
            12,
        );

        let pipeline = decompose(&expression).expect("packed pipeline");
        assert!(matches!(pipeline.source, Expression::Variable(name) if name == "uls"));
        assert_eq!(
            (pipeline.shift, pipeline.mask, pipeline.operations),
            (15, 0x00ff_8000, 4)
        );
    }

    #[test]
    fn unsigned_narrowing_participates_in_the_final_mask() {
        let expression = shift(
            Expression::Cast {
                target_type: Type::UnsignedShort,
                operand: Box::new(Expression::Variable("width".into())),
            },
            1,
        );
        let pipeline = decompose(&expression).expect("packed pipeline");

        assert_eq!((pipeline.shift, pipeline.mask), (1, 0x0001_fffe));
        assert_eq!(pipeline.operations, 2);
    }

    #[test]
    fn combines_a_masked_right_shift_with_a_following_left_shift() {
        let expression = shift(
            mask(shift_right(Expression::Variable("value".into()), 3), 0x1ff),
            9,
        );
        let pipeline = decompose(&expression).expect("packed pipeline");

        assert_eq!(
            (
                pipeline.shift,
                pipeline.mask,
                pipeline.possible_sign_fill,
                pipeline.operations,
            ),
            (6, 0x0003_fe00, 0, 3)
        );
        assert_eq!(mask_to_run(pipeline.mask), Some((14, 22)));
    }

    #[test]
    fn rejects_an_unmasked_possible_arithmetic_right_shift() {
        let expression = shift(shift_right(Expression::Variable("value".into()), 3), 1);
        let pipeline = decompose(&expression).expect("right-shift pipeline");

        assert_ne!(pipeline.possible_sign_fill, 0);
    }

    #[test]
    fn folds_a_power_of_two_multiply_into_the_final_shift() {
        let expression = shift(
            mask(multiply(Expression::Variable("value".into()), 4), 0xfff),
            12,
        );
        let pipeline = decompose(&expression).expect("power-of-two pipeline");

        assert_eq!(
            (pipeline.shift, pipeline.mask, pipeline.operations),
            (14, 0x00ff_c000, 3)
        );
    }
}
