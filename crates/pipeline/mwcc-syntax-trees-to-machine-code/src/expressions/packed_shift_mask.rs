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
            let mut pipeline = decompose(left)?;
            let total_shift = u16::from(pipeline.shift) + u16::from(amount);
            if total_shift >= 32 {
                return None;
            }
            pipeline.shift = total_shift as u8;
            pipeline.mask <<= amount;
            pipeline.operations += 1;
            Some(pipeline)
        }
        source => Some(PackedShiftMask {
            source,
            shift: 0,
            mask: u32::MAX,
            operations: 0,
        }),
    }
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
}
