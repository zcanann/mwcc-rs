//! Branch comparisons between a promoted narrow register and a word register.

use super::*;

impl Generator {
    pub(super) fn try_emit_mixed_width_leaf_compare(
        &mut self,
        left: &Expression,
        right: &Expression,
        signed_compare: bool,
    ) -> Compilation<bool> {
        let (Ok((left, left_width, left_signed)), Ok((right, right_width, right_signed))) =
            (self.leaf_info(left), self.leaf_info(right))
        else {
            return Ok(false);
        };
        let (narrow, width, signed, wide, narrow_left) = match (left_width, right_width) {
            (1..=31, 32) => (left, left_width, left_signed, right, true),
            (32, 1..=31) => (right, right_width, right_signed, left, false),
            _ => return Ok(false),
        };
        let extended = if wide == GENERAL_SCRATCH {
            self.fresh_virtual_general_avoiding(vec![wide])
        } else {
            GENERAL_SCRATCH
        };
        self.emit_widen(extended, narrow, width, signed);
        let (left, right) = if narrow_left {
            (extended, wide)
        } else {
            (wide, extended)
        };
        self.output.instructions.push(if signed_compare {
            Instruction::CompareWord { a: left, b: right }
        } else {
            Instruction::CompareLogicalWord { a: left, b: right }
        });
        Ok(true)
    }
}
