//! Integer division and modulo.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::Expression;
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    /// Emit a division, choosing signed/unsigned and handling power-of-two
    /// constant divisors; non-power-of-two constants (magic-number lowering) and
    /// signed division by powers of two beyond 2 are deferred.
    pub(crate) fn emit_divide(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)? && self.signedness_of(right)?;
        let d = destination;

        if let Expression::IntegerLiteral(divisor) = right {
            let divisor = *divisor;
            if divisor >= 2 && (divisor as u64).is_power_of_two() {
                if !signed {
                    let shift = divisor.trailing_zeros() as u8;
                    // Unsigned `/2^k` is a logical right shift; a narrow operand
                    // fuses the extension and shift into one rlwinm like `>>`.
                    if let Ok((register, width, _)) = self.leaf_info(left) {
                        if width < 32 {
                            if self.emit_narrow_unsigned_shift(d, register, width, false, shift) {
                                return Ok(());
                            }
                            return Err(Diagnostic::error("narrow unsigned divide out of the single-rlwinm range (roadmap)"));
                        }
                    }
                    self.evaluate_general(left, d)?;
                    self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: d, shift });
                    return Ok(());
                }
                if divisor == 2 {
                    // signed /2 rounds toward zero: add the sign bit, then arithmetic shift.
                    self.evaluate_general(left, d)?;
                    self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: d, shift: 31 });
                    self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: d });
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: GENERAL_SCRATCH, shift: 1 });
                    return Ok(());
                }
            }
            return Err(Diagnostic::error("division by this constant needs magic-number lowering (roadmap)"));
        }

        // register divide: dividend (leaf stays, sub-expr -> scratch), then divisor.
        let Some(dividend) = self.place_operand(left, d, false)? else {
            return Err(Diagnostic::error("dividend needs the full register allocator (roadmap M1)"));
        };
        let divisor = if let Some(register) = leaf_name(right).and_then(|name| self.lookup_general(name)) {
            register
        } else {
            // a sub-expression divisor needs the scratch, which the dividend may occupy.
            if dividend == GENERAL_SCRATCH {
                return Err(Diagnostic::error("divisor and dividend both need scratch (roadmap M1)"));
            }
            if !fits_single_scratch(right, true) {
                return Err(Diagnostic::error("divisor needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        };
        self.output.instructions.push(if signed {
            Instruction::DivideWord { d, a: dividend, b: divisor }
        } else {
            Instruction::DivideWordUnsigned { d, a: dividend, b: divisor }
        });
        Ok(())
    }

    /// Emit a remainder as `left - (left / right) * right` (leaf operands only for now).
    pub(crate) fn emit_modulo(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)? && self.signedness_of(right)?;

        // Unsigned modulo by a power of two is a low-bit mask: a % 2^k == a & (2^k - 1).
        if !signed {
            if let Expression::IntegerLiteral(divisor) = right {
                if *divisor >= 2 && (*divisor as u64).is_power_of_two() {
                    let Some(source) = self.place_operand(left, destination, false)? else {
                        return Err(Diagnostic::error("modulo value needs the full register allocator (roadmap M1)"));
                    };
                    let clear = 32 - divisor.trailing_zeros() as u8;
                    self.output.instructions.push(Instruction::ClearLeftImmediate { a: destination, s: source, clear });
                    return Ok(());
                }
            }
        }

        let left_register = self.general_register_of_leaf(left)?;
        let right_register = self.general_register_of_leaf(right)?;
        self.output.instructions.push(if signed {
            Instruction::DivideWord { d: GENERAL_SCRATCH, a: left_register, b: right_register }
        } else {
            Instruction::DivideWordUnsigned { d: GENERAL_SCRATCH, a: left_register, b: right_register }
        });
        self.output.instructions.push(Instruction::MultiplyLow { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: right_register });
        self.output.instructions.push(Instruction::SubtractFrom { d: destination, a: GENERAL_SCRATCH, b: left_register });
        Ok(())
    }
}
