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
                // signed /2^k (k>=2): arithmetic shift, then `addze` adds the carry
                // the shift sets for a negative dividend (round toward zero).
                let shift = divisor.trailing_zeros() as u8;
                let source = self.place_operand_or_scratch(left, GENERAL_SCRATCH)?;
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: source, shift });
                self.output.instructions.push(Instruction::AddToZeroExtended { d, a: GENERAL_SCRATCH });
                return Ok(());
            }
            // Signed division by a positive, non-power-of-two constant: the
            // magic-number multiply (Granlund–Montgomery).
            if signed && divisor >= 3 && i32::try_from(divisor).is_ok() {
                return self.emit_signed_magic_divide(left, divisor as i32, d);
            }
            return Err(Diagnostic::error("division by this constant needs magic-number lowering (roadmap)"));
        }

        // register divide: dividend (leaf stays, sub-expr -> scratch via the
        // allocator's virtual temporaries), then divisor.
        let dividend = self.place_operand_or_scratch(left, d)?;
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

    /// Signed division by a positive constant via the magic-number multiply.
    /// mwcc materializes the magic `M` (lis/addi), takes the high word of the
    /// signed product `mulhw`, applies the `M<0` correction (`add n`) and the
    /// post-shift (`srawi`), then rounds toward zero by adding the sign bit. The
    /// intermediate quotient lives in r0 whenever a correction or shift is present
    /// (the dividend must stay live); otherwise `mulhw` targets the result and the
    /// sign temporary uses r0. Restricted to the shapes where the dividend is a
    /// leaf and a scratch register is free, deferring otherwise.
    fn emit_signed_magic_divide(&mut self, dividend: &Expression, divisor: i32, destination: u8) -> Compilation<()> {
        let (magic, shift) = signed_magic(divisor);
        let Some(dividend_register) = leaf_name(dividend).and_then(|name| self.lookup_general(name)) else {
            return Err(Diagnostic::error("magic-number division needs a leaf dividend (roadmap)"));
        };
        // The lowest free general register holds the materialized magic's high
        // half. The destination counts as free here — its incoming value is dead
        // and the divide writes its result there only at the very end — but the
        // dividend is live through the multiply, so it is excluded.
        let Some(temp) = (3u8..=12).find(|r| *r != dividend_register && !self.reserved.contains(r)) else {
            return Err(Diagnostic::error("out of registers for magic-number division"));
        };
        // Materialize the 32-bit magic with lis + addi (the addi's low half is
        // sign-extended, so the high half is adjusted to compensate).
        let low = magic as i16;
        let high = (magic.wrapping_sub(low as i32) >> 16) as i16;
        self.output.instructions.push(Instruction::AddImmediateShifted { d: temp, a: 0, immediate: high });
        self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: temp, immediate: low });

        let correction = magic < 0;
        let (quotient, sign_temp) = if correction || shift > 0 {
            // The dividend is needed past the multiply, so the quotient uses r0.
            self.output.instructions.push(Instruction::MultiplyHighWord { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: dividend_register });
            if correction {
                self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: dividend_register });
            }
            if shift > 0 {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift });
            }
            (GENERAL_SCRATCH, destination)
        } else {
            // No correction or shift: the multiply lands straight in the result.
            self.output.instructions.push(Instruction::MultiplyHighWord { d: destination, a: GENERAL_SCRATCH, b: dividend_register });
            (destination, GENERAL_SCRATCH)
        };
        // Round toward zero: add the quotient's sign bit.
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: sign_temp, s: quotient, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: destination, a: quotient, b: sign_temp });
        Ok(())
    }

    /// Emit a remainder as `left - (left / right) * right` (leaf operands only for now).
    pub(crate) fn emit_modulo(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)? && self.signedness_of(right)?;

        // Unsigned modulo by a power of two is a low-bit mask: a % 2^k == a & (2^k - 1).
        if !signed {
            if let Expression::IntegerLiteral(divisor) = right {
                if *divisor >= 2 && (*divisor as u64).is_power_of_two() {
                    let source = self.place_operand_or_scratch(left, destination)?;
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

/// The signed magic number and post-shift for division by `d` (|d| >= 2), per
/// Granlund–Montgomery / Hacker's Delight. All intermediate arithmetic is on
/// unsigned 32-bit values; the result `M` is reinterpreted as signed.
fn signed_magic(d: i32) -> (i32, u8) {
    let two31: u32 = 0x8000_0000;
    let ad = (d as i64).unsigned_abs() as u32;
    let t = two31.wrapping_add((d as u32) >> 31);
    let anc = t - 1 - t % ad;
    let mut p = 31u32;
    let mut q1 = two31 / anc;
    let mut r1 = two31 - q1 * anc;
    let mut q2 = two31 / ad;
    let mut r2 = two31 - q2 * ad;
    loop {
        p += 1;
        q1 = q1.wrapping_mul(2);
        r1 = r1.wrapping_mul(2);
        if r1 >= anc {
            q1 += 1;
            r1 -= anc;
        }
        q2 = q2.wrapping_mul(2);
        r2 = r2.wrapping_mul(2);
        if r2 >= ad {
            q2 += 1;
            r2 -= ad;
        }
        let delta = ad - r2;
        if q1 >= delta && !(q1 == delta && r1 == 0) {
            break;
        }
    }
    let mut magic = (q2 + 1) as i32;
    if d < 0 {
        magic = magic.wrapping_neg();
    }
    (magic, (p - 32) as u8)
}
