//! Comparisons between a narrow register leaf and a memory-backed member.
//!
//! The member occupies r0 while the register leaf still needs extension, so
//! the widened leaf joins allocation instead of overwriting the loaded value.

use super::*;
use mwcc_syntax_trees::Type;

impl Generator {
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
