//! Shifted register values combined with a structure member and masked.
//!
//! CARD response code uses `(value << k) ^ record->state` before clearing the
//! low bits. MWCC loads the member first, shifts the register leaf into the
//! destination, then combines through r0. That is distinct from the ordinary
//! commutative-operand policy and is owned explicitly here.

use super::*;

impl Generator {
    pub(crate) fn try_emit_shifted_member_high_mask(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: combined,
            right: mask,
        } = expression
        else {
            return Ok(false);
        };
        let Some(mask) = constant_value(mask).map(|value| value as i32 as u32) else {
            return Ok(false);
        };
        let cleared_bits = mask.trailing_zeros();
        if cleared_bits == 0 || cleared_bits >= 32 || mask != u32::MAX << cleared_bits {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitXor,
            left: shifted,
            right: member,
        } = combined.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: variable,
            right: shift,
        } = shifted.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(variable) = variable.as_ref() else {
            return Ok(false);
        };
        let Some(variable) = self.lookup_general(variable) else {
            return Ok(false);
        };
        let Some(shift) = constant_value(shift).and_then(|value| u8::try_from(value).ok()) else {
            return Ok(false);
        };
        if shift == 0 || shift >= 32 {
            return Ok(false);
        }
        let Expression::Member {
            member_type: member_type @ (Type::Int | Type::UnsignedInt),
            index_stride: None,
            ..
        } = member.as_ref()
        else {
            return Ok(false);
        };

        self.evaluate(member, *member_type, GENERAL_SCRATCH)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: destination,
                s: variable,
                shift,
            });
        self.output.instructions.push(Instruction::Xor {
            a: GENERAL_SCRATCH,
            s: destination,
            b: GENERAL_SCRATCH,
        });
        self.output
            .instructions
            .push(Instruction::AndContiguousMask {
                a: destination,
                s: GENERAL_SCRATCH,
                begin: 0,
                end: 31 - cleared_bits as u8,
            });
        Ok(true)
    }
}
