//! Pointer-alignment expressions lowered as mwcc's `addi; clrrwi` idiom.

use super::*;

/// Recognize `(pointer_type)(((word)source + alignment - 1) & -alignment)`.
///
/// The casts are semantically important: this is integer address alignment,
/// not scaled pointer arithmetic. Keep the recognizer narrow so unrelated
/// add-and-mask expressions continue through the general arithmetic paths.
fn round_up_parts(expression: &Expression) -> Option<(&Expression, u8)> {
    let Expression::Cast {
        target_type: Type::Pointer(_) | Type::StructPointer { .. },
        operand: masked,
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = masked.as_ref()
    else {
        return None;
    };

    let (sum, mask) = if let Some(mask) = constant_value(right) {
        (left.as_ref(), mask)
    } else {
        (right.as_ref(), constant_value(left)?)
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = sum
    else {
        return None;
    };
    let (source, bias) = if let Some(bias) = constant_value(right) {
        (left.as_ref(), bias)
    } else {
        (right.as_ref(), constant_value(left)?)
    };

    let alignment = bias.checked_add(1)?;
    if !(2..=32768).contains(&alignment) {
        return None;
    }
    let alignment = u32::try_from(alignment).ok()?;
    if !alignment.is_power_of_two() {
        return None;
    }
    if (mask as i32 as u32) != !(alignment - 1) {
        return None;
    }

    let source = match source {
        Expression::Cast {
            target_type: Type::Int | Type::UnsignedInt,
            operand,
        } => operand.as_ref(),
        _ => source,
    };
    if !matches!(source, Expression::Variable(_)) {
        return None;
    }

    Some((source, alignment.trailing_zeros() as u8))
}

impl Generator {
    /// Emit the canonical power-of-two address round-up. The biased value uses
    /// r0 even when source and destination coincide, preserving the live source
    /// until the final mask exactly as mwcc does.
    pub(crate) fn try_emit_pointer_round_up(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some((source, cleared_bits)) = round_up_parts(expression) else {
            return Ok(false);
        };
        let source = self.general_register_of_leaf(source)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: source,
            immediate: ((1_u32 << cleared_bits) - 1) as i16,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: destination,
                s: GENERAL_SCRATCH,
                begin: 0,
                end: 31 - cleared_bits,
            });
        Ok(true)
    }
}
