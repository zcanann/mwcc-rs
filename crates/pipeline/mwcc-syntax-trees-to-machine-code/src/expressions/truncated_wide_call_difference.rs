//! A word-sized observation of a wide call-result difference.
//!
//! The low subtraction is sufficient after a 32-bit cast. A volatile global
//! still requires both word reads even though its high word is discarded.

use super::*;

impl Generator {
    pub(crate) fn try_emit_truncated_wide_call_difference(
        &mut self,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } = operand
        else {
            return Ok(false);
        };
        let Expression::Call { name, arguments } = left.as_ref() else {
            return Ok(false);
        };
        let Expression::Variable(global) = right.as_ref() else {
            return Ok(false);
        };
        if !matches!(
            self.call_return_types.get(name),
            Some(Type::LongLong | Type::UnsignedLongLong)
        ) || !matches!(
            self.globals.get(global),
            Some(Type::LongLong | Type::UnsignedLongLong)
        ) || self.known_locals.contains(global)
            || self.locations.contains_key(global)
        {
            return Ok(false);
        }
        self.emit_call(name, arguments, None, false)?;
        if self.volatile_globals.contains(global) {
            self.record_relocation(RelocationKind::EmbSda21, global);
            self.output.instructions.push(Instruction::LoadWord {
                d: 5,
                a: 0,
                offset: 0,
            });
        }
        self.record_relocation_with_addend(RelocationKind::EmbSda21, global, 4);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFromCarrying {
                d: destination,
                a: 0,
                b: 4,
            });
        Ok(true)
    }
}
