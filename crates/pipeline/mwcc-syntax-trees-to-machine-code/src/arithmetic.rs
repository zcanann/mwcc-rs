//! Constant folding, immediate forms, complement fusion, and shifts.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    /// If one operand is `~leaf` and the other is a leaf, emit `andc`/`orc`.
    pub(crate) fn try_emit_complement_logical(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> bool {
        let (kept_expression, complemented_name) = if let Some(name) = complemented_leaf_name(right) {
            (left, name)
        } else if let Some(name) = complemented_leaf_name(left) {
            (right, name)
        } else {
            return false;
        };
        let (Some(kept_name), Some(complemented_register)) = (leaf_name(kept_expression), self.lookup_general(complemented_name)) else {
            return false;
        };
        let Some(kept_register) = self.lookup_general(kept_name) else {
            return false;
        };
        self.output.instructions.push(match operator {
            BinaryOperator::BitAnd => Instruction::AndComplement { a: destination, s: kept_register, b: complemented_register },
            _ => Instruction::OrComplement { a: destination, s: kept_register, b: complemented_register },
        });
        true
    }

    /// Emit a right shift, choosing arithmetic (signed) or logical (unsigned)
    /// from the type of the shifted value.
    pub(crate) fn emit_shift_right(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)?;
        let d = destination;

        if let Expression::IntegerLiteral(amount) = right {
            if (1..=31).contains(amount) {
                // An unsigned narrow value fuses extension and shift into one
                // rlwinm; a signed narrow value extends (extsb/extsh) then shifts.
                if let Ok((register, width, leaf_signed)) = self.leaf_info(left) {
                    if width < 32 && !leaf_signed {
                        if self.emit_narrow_unsigned_shift(d, register, width, false, *amount as u8) {
                            return Ok(());
                        }
                        return Err(Diagnostic::error("narrow unsigned shift out of the single-rlwinm range (roadmap)"));
                    }
                }
                // The shifted value: a leaf stays put, a sub-expression goes to scratch.
                let Some(source) = self.place_operand(left, d, false)? else {
                    return Err(Diagnostic::error("shift value needs the full register allocator (roadmap M1)"));
                };
                let shift = *amount as u8;
                self.output.instructions.push(if signed {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: source, shift }
                } else {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: source, shift }
                });
                return Ok(());
            }
        }

        // Register form: value into the destination, shift amount into a register.
        self.evaluate_general(left, d)?;
        let amount = if is_complex(right) {
            if !fits_single_scratch(right, true) {
                return Err(Diagnostic::error("shift amount needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(right)?
        };
        self.output.instructions.push(if signed {
            Instruction::ShiftRightAlgebraicWord { a: d, s: d, b: amount }
        } else {
            Instruction::ShiftRightWord { a: d, s: d, b: amount }
        });
        Ok(())
    }

    /// Fold a constant operand into an immediate instruction. Returns whether an
    /// instruction was emitted; if the constant does not qualify (out of range,
    /// non-mask), returns false so the caller can stop honestly.
    pub(crate) fn try_emit_general_with_constant(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        // variable op constant — subtraction becomes addition of the negation.
        if let Some(constant) = constant_value(right) {
            let (effective, value) = match operator {
                BinaryOperator::Subtract => (BinaryOperator::Add, -constant),
                other => (other, constant),
            };
            if self.emit_constant_form(effective, left, value, destination)? {
                return Ok(true);
            }
        }
        // constant op variable — only the commutative operators.
        if is_commutative(operator) {
            if let Some(constant) = constant_value(left) {
                if self.emit_constant_form(operator, right, constant, destination)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Apply `constant` to `variable` via the matching immediate instruction, if
    /// the constant qualifies. The operand is read from its own register (a leaf)
    /// or computed into `destination` (a sub-expression); the immediate then reads
    /// that source directly — `addi` must not take `r0` as its source, which would
    /// silently mean `li`.
    pub(crate) fn emit_constant_form(&mut self, operator: BinaryOperator, variable: &Expression, constant: i64, destination: u8) -> Compilation<bool> {
        // Identity and strength-reduction folds.
        match (operator, constant) {
            (BinaryOperator::Add, 0) => {
                self.evaluate_general(variable, destination)?;
                return Ok(true);
            }
            (BinaryOperator::Multiply, 0) => {
                self.load_integer_constant(destination, 0);
                return Ok(true);
            }
            (BinaryOperator::Multiply, 1) => {
                self.evaluate_general(variable, destination)?;
                return Ok(true);
            }
            (BinaryOperator::Multiply, -1) => {
                let Some(source) = self.place_operand(variable, destination, false)? else {
                    return Ok(false);
                };
                self.output.instructions.push(Instruction::Negate { d: destination, a: source });
                return Ok(true);
            }
            _ => {}
        }

        enum Immediate {
            Add,
            ShiftLeft(u8),
            Multiply,
            Or,
            Xor,
            Mask(u8, u8),
        }
        let kind = match operator {
            BinaryOperator::Add if fits_signed_16(constant) => Immediate::Add,
            BinaryOperator::Multiply if fits_signed_16(constant) => {
                if constant >= 2 && (constant as u64).is_power_of_two() {
                    Immediate::ShiftLeft(constant.trailing_zeros() as u8)
                } else {
                    Immediate::Multiply
                }
            }
            BinaryOperator::BitOr if fits_unsigned_16(constant) => Immediate::Or,
            BinaryOperator::BitXor if fits_unsigned_16(constant) => Immediate::Xor,
            BinaryOperator::BitAnd if contiguous_mask(constant).is_some() => {
                let (begin, end) = contiguous_mask(constant).unwrap();
                Immediate::Mask(begin, end)
            }
            BinaryOperator::ShiftLeft if (1..=31).contains(&constant) => Immediate::ShiftLeft(constant as u8),
            _ => return Ok(false),
        };

        // A narrow value times a power of two (or `<< n`): an unsigned narrow
        // operand fuses extension and shift into one rlwinm; a signed one extends
        // (extsb/extsh) then shifts via the normal path below.
        if let &Immediate::ShiftLeft(shift) = &kind {
            if let Ok((register, width, leaf_signed)) = self.leaf_info(variable) {
                if width < 32 && !leaf_signed {
                    return Ok(self.emit_narrow_unsigned_shift(destination, register, width, true, shift));
                }
            }
        }
        let prefer_destination = matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract);
        let Some(source) = self.place_operand(variable, destination, prefer_destination)? else {
            return Ok(false);
        };
        let d = destination;
        let instruction = match kind {
            Immediate::Add => Instruction::AddImmediate { d, a: source, immediate: constant as i16 },
            Immediate::ShiftLeft(shift) => Instruction::ShiftLeftImmediate { a: d, s: source, shift },
            Immediate::Multiply => Instruction::MultiplyImmediate { d, a: source, immediate: constant as i16 },
            Immediate::Or => Instruction::OrImmediate { a: d, s: source, immediate: constant as u16 },
            Immediate::Xor => Instruction::XorImmediate { a: d, s: source, immediate: constant as u16 },
            Immediate::Mask(begin, end) => Instruction::AndContiguousMask { a: d, s: source, begin, end },
        };
        self.output.instructions.push(instruction);
        Ok(true)
    }
}
