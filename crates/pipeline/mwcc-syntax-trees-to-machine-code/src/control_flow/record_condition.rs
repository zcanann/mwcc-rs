//! Arithmetic conditions that set CR0 on their final value instruction.
//!
//! mwcc turns a computed truth test into the record form of the expression's
//! final operation when PowerPC provides one. The branch can then consume CR0
//! directly, avoiding a separate `cmpwi` and avoiding materializing 0/1.

use super::*;

impl Generator {
    pub(super) fn try_emit_computed_record_condition(
        &mut self,
        condition: &Expression,
    ) -> Compilation<bool> {
        if !matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                ..
            }
        ) {
            return Ok(false);
        }

        self.evaluate_general(condition, GENERAL_SCRATCH)?;
        let Some(last) = self.output.instructions.last_mut() else {
            return Ok(false);
        };
        let replacement = match *last {
            Instruction::MultiplyLow { d, a, b } => {
                Some(Instruction::MultiplyLowRecord { d, a, b })
            }
            _ => None,
        };
        if let Some(record) = replacement {
            *last = record;
            Ok(true)
        } else {
            Err(Diagnostic::error(
                "a computed multiply condition did not end in a recordable multiply",
            ))
        }
    }
}
