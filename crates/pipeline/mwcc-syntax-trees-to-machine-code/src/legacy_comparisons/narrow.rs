//! Narrow-operand placement for build-163 comparison values.
//!
//! A signed byte is loaded zero-extended by `lbz`, then widened in a register
//! selected around the carry chain. This schedule is distinct from both normal
//! arithmetic widening and the modern comparison idioms.

use crate::analysis::is_zero_literal;
use crate::generator::{Generator, GENERAL_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    pub(super) fn try_emit_legacy_signed_byte_zero_comparison(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        signed: bool,
    ) -> Compilation<bool> {
        if operator == BinaryOperator::NotEqual && is_zero_literal(right) {
            if let Some((source, width, source_signed)) = self
                .leaf_info(left)
                .ok()
                .filter(|&(_, width, _)| width < 32)
            {
                self.emit_widen(GENERAL_SCRATCH, source, width, source_signed);
                self.output.instructions.push(Instruction::Negate {
                    d: destination,
                    a: GENERAL_SCRATCH,
                });
                self.emit_legacy_not_equal_tail(destination, destination);
                return Ok(true);
            }
        }

        if !signed || !is_zero_literal(right) || !self.is_signed_byte_load(left)? {
            return Ok(false);
        }

        match operator {
            BinaryOperator::Less | BinaryOperator::Greater => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                self.load_integer_constant(destination, 0);
                let widened = self.fresh_virtual_general();
                self.emit_widen(widened, GENERAL_SCRATCH, 8, true);
                let (first, second) = if operator == BinaryOperator::Less {
                    (destination, widened)
                } else {
                    (widened, destination)
                };
                self.output.instructions.push(Instruction::Eqv {
                    a: GENERAL_SCRATCH,
                    s: first,
                    b: second,
                });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: destination,
                        a: first,
                        b: second,
                    });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::AddToZeroExtended {
                        d: destination,
                        a: GENERAL_SCRATCH,
                    });
                self.output
                    .instructions
                    .push(Instruction::ClearLeftImmediate {
                        a: destination,
                        s: destination,
                        clear: 31,
                    });
            }
            BinaryOperator::GreaterEqual => {
                self.evaluate_general(left, destination)?;
                self.load_integer_constant(GENERAL_SCRATCH, 0);
                let zero_sign = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: zero_sign,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                let widened = self.fresh_virtual_general();
                self.emit_widen(widened, destination, 8, true);
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: destination,
                        s: widened,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: GENERAL_SCRATCH,
                        a: GENERAL_SCRATCH,
                        b: widened,
                    });
                self.output.instructions.push(Instruction::AddExtended {
                    d: destination,
                    a: destination,
                    b: zero_sign,
                });
            }
            BinaryOperator::LessEqual => {
                let raw = self.fresh_virtual_general();
                self.evaluate_general(left, raw)?;
                self.load_integer_constant(GENERAL_SCRATCH, 0);
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: destination,
                        s: GENERAL_SCRATCH,
                        shift: 31,
                    });
                let widened = self.fresh_virtual_general();
                self.emit_widen(widened, raw, 8, true);
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: raw,
                        s: widened,
                        shift: 31,
                    });
                self.output
                    .instructions
                    .push(Instruction::SubtractFromCarrying {
                        d: GENERAL_SCRATCH,
                        a: widened,
                        b: GENERAL_SCRATCH,
                    });
                self.output.instructions.push(Instruction::AddExtended {
                    d: destination,
                    a: destination,
                    b: raw,
                });
            }
            BinaryOperator::NotEqual => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
                self.output.instructions.push(Instruction::Negate {
                    d: destination,
                    a: GENERAL_SCRATCH,
                });
                self.emit_legacy_not_equal_tail(destination, destination);
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
