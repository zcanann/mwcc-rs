//! Record-form contiguous mask tests used directly by branches.
//!
//! Register leaves and single-word memory loads share the same terminal
//! `rlwinm.` test. Memory values are first loaded into the integer scratch, so
//! the condition sets CR0 without materializing a masked value and comparing it
//! in a separate instruction.

use super::*;

impl Generator {
    pub(super) fn try_emit_record_mask_test(
        &mut self,
        value: &Expression,
        mask: &Expression,
    ) -> Compilation<bool> {
        let Some(mask) = constant_value(mask).and_then(|value| u32::try_from(value).ok()) else {
            return Ok(false);
        };
        let Some((begin, end)) = mask_to_run(mask) else {
            return Ok(false);
        };
        let source = if let Some(register) =
            leaf_name(value).and_then(|name| self.lookup_general(name))
        {
            register
        } else if self.is_word_load(value)
            || matches!(value, Expression::Variable(name)
                if self.frame_slots.get(name).is_some_and(|slot|
                    !slot.is_array && slot.class == ValueClass::General))
        {
            self.evaluate_general(value, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            return Ok(false);
        };
        self.output.instructions.push(Instruction::AndMaskRecord {
            a: GENERAL_SCRATCH,
            s: source,
            begin,
            end,
        });
        Ok(true)
    }
}
