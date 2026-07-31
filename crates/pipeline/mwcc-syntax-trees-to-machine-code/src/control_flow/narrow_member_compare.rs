//! Comparisons whose narrow operands compete for the r0 load scratch.
//!
//! Preserve and promote one operand in an allocator-backed register before a
//! memory-backed sibling overwrites r0.

use super::*;
use mwcc_syntax_trees::Type;

impl Generator {
    /// Compare two narrow memory values when both naturally load through r0.
    ///
    /// Narrow loads already produce a promoted value except for signed bytes:
    /// `lha` sign-extends and `lhz`/`lbz` zero-extend. Preserve the left load in
    /// an allocator-backed register (sign-extending a signed byte on the way),
    /// then let the right load reuse r0. This is MWCC's measured
    /// `lbz r0; extsb r4,r0; lha r0; cmpw r4,r0` schedule.
    pub(crate) fn try_emit_narrow_memory_compare(
        &mut self,
        left: &Expression,
        right: &Expression,
        left_register: u8,
        signed_compare: bool,
    ) -> Compilation<bool> {
        let narrow_memory = |generator: &Self, expression: &Expression| {
            (generator.is_byte_load(expression) || generator.is_halfword_load(expression))
                .then_some(())
        };
        if left_register != GENERAL_SCRATCH
            || narrow_memory(self, left).is_none()
            || narrow_memory(self, right).is_none()
        {
            return Ok(false);
        }

        let preserved_left = self.fresh_virtual_general_preferring(4);
        if self.is_signed_byte_load(left)? {
            self.emit_widen(preserved_left, left_register, 8, true);
        } else {
            self.output.instructions.push(Instruction::move_register(
                preserved_left,
                left_register,
            ));
        }

        let right_register = self.condition_operand_register(right)?;
        // Direct member/dereference/index byte loads are raw `lbz` values. A
        // signed-char global is already extended by `emit_global_load`.
        if self.is_signed_byte_load(right)? && !matches!(right, Expression::Variable(_)) {
            self.emit_widen(right_register, right_register, 8, true);
        }

        if signed_compare {
            self.output.instructions.push(Instruction::CompareWord {
                a: preserved_left,
                b: right_register,
            });
        } else {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWord {
                    a: preserved_left,
                    b: right_register,
                });
        }
        Ok(true)
    }

    pub(crate) fn try_emit_narrow_leaf_member_compare(
        &mut self,
        left: &Expression,
        right: &Expression,
        left_register: u8,
        signed_compare: bool,
    ) -> Compilation<bool> {
        let Ok((leaf_register, leaf_width, leaf_signed)) = self.leaf_info(left) else {
            return Ok(false);
        };
        if leaf_register != left_register || leaf_width >= 32 || as_member(right).is_none() {
            return Ok(false);
        }

        // MWCC evaluates the source-right member first, then widens the
        // source-left leaf into the lowest available register.  This preserves
        // the r0 member value and, when the leaf has a later use, its raw home.
        let member_register = self.condition_operand_register(right)?;
        let widened_leaf = self.fresh_virtual_general();
        self.emit_widen(widened_leaf, leaf_register, leaf_width, leaf_signed);

        let member_type = as_member(right)
            .map(|(_, _, member_type)| member_type)
            .expect("the shape check above established a member");
        if member_type == Type::Char {
            self.emit_widen(member_register, member_register, 8, true);
        }

        if signed_compare {
            self.output.instructions.push(Instruction::CompareWord {
                a: widened_leaf,
                b: member_register,
            });
        } else {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWord {
                    a: widened_leaf,
                    b: member_register,
                });
        }
        Ok(true)
    }
}
