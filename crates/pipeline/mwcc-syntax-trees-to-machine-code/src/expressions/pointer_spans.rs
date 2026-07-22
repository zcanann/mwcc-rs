//! Unsigned pointer-span arithmetic used to index fixed-size memory blocks.

use super::*;

struct PointerSpanScale<'a> {
    current: &'a Expression,
    origin_base: &'a Expression,
    origin_offset: u32,
    origin_type: Type,
    scale_base: &'a Expression,
    scale_offset: u32,
    scale_type: Type,
    shift: u8,
}

fn unsigned_word_operand(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand,
        } => Some(operand),
        _ => None,
    }
}

/// Recognize `((u32)current - (u32)owner->origin) / BLOCK * owner->scale`.
///
/// The unsigned casts are required: they select `srwi`, and make the pointer
/// subtraction raw address arithmetic rather than C pointer-element arithmetic.
/// Keeping the recognizer semantic and narrow avoids changing the scheduling of
/// ordinary divide/multiply trees.
fn pointer_span_scale_parts(expression: &Expression) -> Option<PointerSpanScale<'_>> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: quotient,
        right: scale,
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: difference,
        right: block_size,
    } = quotient.as_ref()
    else {
        return None;
    };
    let block_size = u32::try_from(constant_value(block_size)?).ok()?;
    if block_size < 2 || !block_size.is_power_of_two() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: current,
        right: origin,
    } = difference.as_ref()
    else {
        return None;
    };
    let current = unsigned_word_operand(current)?;
    let origin = unsigned_word_operand(origin)?;
    if !matches!(current, Expression::Variable(_)) {
        return None;
    }
    let (origin_base, origin_offset, origin_type) = as_member(origin)?;
    if !matches!(origin_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let (scale_base, scale_offset, scale_type) = as_member(scale)?;
    if !matches!(scale_type, Type::Int | Type::UnsignedInt) {
        return None;
    }

    Some(PointerSpanScale {
        current,
        origin_base,
        origin_offset,
        origin_type,
        scale_base,
        scale_offset,
        scale_type,
        shift: block_size.trailing_zeros() as u8,
    })
}

impl Generator {
    /// Emit mwcc's load-first schedule for a fixed-size block offset. Both
    /// members are loaded before the arithmetic, preserving the shared owner
    /// and current pointer until the allocator assigns the scale temporary.
    pub(crate) fn try_emit_pointer_span_scale(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(parts) = pointer_span_scale_parts(expression) else {
            return Ok(false);
        };

        self.emit_member_load(
            parts.origin_base,
            parts.origin_offset,
            parts.origin_type,
            None,
            GENERAL_SCRATCH,
        )?;
        let scale = self.fresh_virtual_general();
        self.emit_member_load(
            parts.scale_base,
            parts.scale_offset,
            parts.scale_type,
            None,
            scale,
        )?;
        let current = self.general_register_of_leaf(parts.current)?;
        self.output.instructions.push(Instruction::SubtractFrom {
            d: GENERAL_SCRATCH,
            a: GENERAL_SCRATCH,
            b: current,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: GENERAL_SCRATCH,
                s: GENERAL_SCRATCH,
                shift: parts.shift,
            });
        self.output.instructions.push(Instruction::MultiplyLow {
            d: destination,
            a: scale,
            b: GENERAL_SCRATCH,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(name.into())),
            offset,
            member_type,
            index_stride: None,
        }
    }

    fn cast_unsigned(expression: Expression) -> Expression {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(expression),
        }
    }

    fn span_expression(block_size: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Divide,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::Subtract,
                    left: Box::new(cast_unsigned(Expression::Variable("current".into()))),
                    right: Box::new(cast_unsigned(member(
                        "owner",
                        128,
                        Type::StructPointer { element_size: 8192 },
                    ))),
                }),
                right: Box::new(Expression::IntegerLiteral(block_size)),
            }),
            right: Box::new(member("owner", 12, Type::UnsignedInt)),
        }
    }

    #[test]
    fn recognizes_an_unsigned_power_of_two_pointer_span() {
        let expression = span_expression(8192);
        let parts = pointer_span_scale_parts(&expression).expect("pointer span");
        assert_eq!(parts.origin_offset, 128);
        assert_eq!(parts.scale_offset, 12);
        assert_eq!(parts.shift, 13);
    }

    #[test]
    fn rejects_non_power_of_two_and_signed_pointer_spans() {
        assert!(pointer_span_scale_parts(&span_expression(6000)).is_none());

        let mut signed = span_expression(8192);
        let Expression::Binary { left, .. } = &mut signed else { unreachable!() };
        let Expression::Binary { left, .. } = left.as_mut() else { unreachable!() };
        let Expression::Binary { left, .. } = left.as_mut() else { unreachable!() };
        let Expression::Cast { target_type, .. } = left.as_mut() else { unreachable!() };
        *target_type = Type::Int;
        assert!(pointer_span_scale_parts(&signed).is_none());
    }
}
