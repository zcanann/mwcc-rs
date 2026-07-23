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
        // A member address used for truth (`if (&p->member)`) folds address
        // formation and the CR0 test into `addic.`. Assertion macros expose this
        // after preprocessing as the condition of a discarded ternary.
        let member_address = match condition {
            Expression::AddressOf { operand } => match operand.as_ref() {
                Expression::Member {
                    base,
                    offset,
                    index_stride: None,
                    ..
                } => Some((base.as_ref(), *offset)),
                _ => None,
            },
            Expression::MemberAddress {
                base,
                offset,
                index_stride: None,
                ..
            } => Some((base.as_ref(), *offset)),
            _ => None,
        };
        if let Some((base, offset)) = member_address {
            if let (Some(base), Ok(immediate)) = (
                leaf_name(base).and_then(|name| self.lookup_general(name)),
                i16::try_from(offset as i64),
            ) {
                self.output
                    .instructions
                    .push(Instruction::AddImmediateCarryingRecord {
                        d: GENERAL_SCRATCH,
                        a: base,
                        immediate,
                    });
                return Ok(true);
            }
        }
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
