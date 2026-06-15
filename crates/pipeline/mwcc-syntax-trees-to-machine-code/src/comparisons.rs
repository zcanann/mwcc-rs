//! Branchless comparison idioms.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    /// Emit a comparison as mwcc's branchless idiom. Currently handles `==` (and
    /// `== 0`) and signed `< 0`; the richer signed less/greater idioms are not
    /// implemented yet.
    pub(crate) fn emit_comparison(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let d = destination;
        let signed_left = self.signedness_of(left)?;
        match operator {
            BinaryOperator::Equal => {
                if is_zero_literal(right) || is_zero_literal(left) {
                    let value = if is_zero_literal(right) { left } else { right };
                    let source = self.place_operand_or_scratch(value, d)?;
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                } else if let Some(constant) = as_small_integer(right) {
                    // a == c : (c - a) leading zeros. A narrow operand is extended
                    // into the scratch first (extsb/clrlwi), then consumed there.
                    let value = match self.leaf_info(left) {
                        Ok((register, width, signed)) if width < 32 => {
                            self.emit_widen(GENERAL_SCRATCH, register, width, signed);
                            GENERAL_SCRATCH
                        }
                        _ => self.general_register_of_leaf(left)?,
                    };
                    self.output.instructions.push(Instruction::SubtractFromImmediate { d: GENERAL_SCRATCH, a: value, immediate: constant });
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                } else {
                    // a == b : leading zeros of (a - b). Narrow operands are
                    // extended first — the left in place, the right into the
                    // scratch (mwcc's placement for the equality idiom).
                    let (left_register, right_register) = self.place_compare_leaves(left, right)?;
                    self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: left_register, b: right_register });
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                }
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
                Ok(())
            }
            // x != 0 : sign bit of (-x | x)
            BinaryOperator::NotEqual if is_zero_literal(right) => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed x < 0 : the sign bit.
            BinaryOperator::Less if is_zero_literal(right) && signed_left => {
                let source = self.place_operand_or_scratch(left, d)?;
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: source, shift: 31 });
                Ok(())
            }
            // signed x > 0 : sign bit of (-x & ~x)
            BinaryOperator::Greater if is_zero_literal(right) && signed_left => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed x >= 0 : !(x < 0)
            BinaryOperator::GreaterEqual if is_zero_literal(right) && signed_left => {
                let source = self.place_operand_or_scratch(left, d)?;
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: source, shift: 31 });
                self.output.instructions.push(Instruction::XorImmediate { a: d, s: GENERAL_SCRATCH, immediate: 1 });
                Ok(())
            }
            // general signed branchless comparisons (leaf operands)
            BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::NotEqual
                if signed_left && leaf_name(left).is_some() && leaf_name(right).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                let scratch = GENERAL_SCRATCH;
                match operator {
                    // a < b : sign bit of (((a^b)>>1) - ((a^b)&b))
                    BinaryOperator::Less => {
                        self.output.instructions.push(Instruction::Xor { a: scratch, s: right_register, b: left_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: scratch, shift: 1 });
                        self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: right_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: d });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                    // a > b : sign bit of (((a^b)>>1) - ((a^b)&a)), reusing rB as a temp
                    BinaryOperator::Greater => {
                        self.output.instructions.push(Instruction::Xor { a: scratch, s: left_register, b: right_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: right_register, s: scratch, shift: 1 });
                        self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: left_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: right_register });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                    // a != b : sign bit of ((b - a) | (a - b)), with a second temp
                    _ => {
                        let temp = (3u8..=12).find(|r| ![left_register, right_register, scratch].contains(r)).ok_or_else(|| Diagnostic::error("out of registers"))?;
                        self.output.instructions.push(Instruction::SubtractFrom { d: temp, a: left_register, b: right_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: right_register, b: left_register });
                        self.output.instructions.push(Instruction::Or { a: scratch, s: temp, b: scratch });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                }
                Ok(())
            }
            // unsigned a < b / a > b : xor/cntlzw/slw/srwi.
            BinaryOperator::Less | BinaryOperator::Greater
                if !signed_left && leaf_name(left).is_some() && leaf_name(right).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                // a < b uses b as the high side; a > b is b < a.
                let high = if matches!(operator, BinaryOperator::Less) { right_register } else { left_register };
                let low = if matches!(operator, BinaryOperator::Less) { left_register } else { right_register };
                self.output.instructions.push(Instruction::Xor { a: GENERAL_SCRATCH, s: high, b: low });
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftLeftWord { a: GENERAL_SCRATCH, s: high, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // unsigned a <= b / a >= b : orc-based, dest + scratch.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if !signed_left && leaf_name(left).is_some() && leaf_name(right).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                // a<=b uses (low,high)=(a,b); a>=b is b<=a.
                let (low, high) = match operator {
                    BinaryOperator::LessEqual => (left_register, right_register),
                    _ => (right_register, left_register),
                };
                self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: low, b: high });
                self.output.instructions.push(Instruction::OrComplement { a: d, s: high, b: low });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 1 });
                self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed a <= b / a >= b : carry-based, with two temporaries.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if signed_left && leaf_name(left).is_some() && leaf_name(right).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                let mut free = (3u8..=12).filter(|r| ![left_register, right_register, GENERAL_SCRATCH].contains(r));
                let (Some(lower), Some(higher)) = (free.next(), free.next()) else {
                    return Err(Diagnostic::error("out of registers for comparison"));
                };
                // For a<=b: high = sign(b), low = sign(a), carry from (b - a).
                // For a>=b the operands swap.
                let (sign_high, sign_low, subtrahend, minuend) = match operator {
                    BinaryOperator::LessEqual => (right_register, left_register, left_register, right_register),
                    _ => (left_register, right_register, right_register, left_register),
                };
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: higher, s: sign_high, shift: 31 });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: lower, s: sign_low, shift: 31 });
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: GENERAL_SCRATCH, a: subtrahend, b: minuend });
                self.output.instructions.push(Instruction::AddExtended { d, a: higher, b: lower });
                Ok(())
            }
            _ => Err(Diagnostic::error("this comparison needs the branchless compare idioms (roadmap)")),
        }
    }

    /// Place two leaf operands for the equality idiom, extending narrow operands
    /// the way mwcc does: when both are narrow the left is extended in its home
    /// register and the right into the scratch; when only one is narrow it goes to
    /// the scratch and the wide operand stays in its home register. Build-aware via
    /// each leaf's signedness; transparent (home registers) for the all-int case.
    pub(crate) fn place_compare_leaves(&mut self, left: &Expression, right: &Expression) -> Compilation<(u8, u8)> {
        let (left_register, left_width, left_signed) = self.leaf_info(left)?;
        let (right_register, right_width, right_signed) = self.leaf_info(right)?;
        let left_narrow = left_width < 32;
        let right_narrow = right_width < 32;

        let (left_placed, right_placed) = if left_narrow && right_narrow {
            self.emit_widen(left_register, left_register, left_width, left_signed);
            self.emit_widen(GENERAL_SCRATCH, right_register, right_width, right_signed);
            (left_register, GENERAL_SCRATCH)
        } else if left_narrow {
            self.emit_widen(GENERAL_SCRATCH, left_register, left_width, left_signed);
            (GENERAL_SCRATCH, right_register)
        } else if right_narrow {
            self.emit_widen(GENERAL_SCRATCH, right_register, right_width, right_signed);
            (left_register, GENERAL_SCRATCH)
        } else {
            (left_register, right_register)
        };
        Ok((left_placed, right_placed))
    }
}
