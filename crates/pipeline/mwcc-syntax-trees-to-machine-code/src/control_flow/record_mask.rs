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
        let register_leaf = leaf_name(value).and_then(|name| self.lookup_general(name));
        let memory_value = self.is_word_load(value)
            || matches!(value, Expression::Variable(name)
                if self.frame_slots.get(name).is_some_and(|slot|
                    !slot.is_array && slot.class == ValueClass::General));
        if register_leaf.is_none() && !memory_value {
            return Ok(false);
        }
        if let Some((begin, end)) = mask_to_run(mask) {
            let source = if let Some(register) = register_leaf {
                register
            } else {
                self.evaluate_general(value, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            };
            self.output.instructions.push(Instruction::AndMaskRecord {
                a: GENERAL_SCRATCH,
                s: source,
                begin,
                end,
            });
        } else {
            // A discontiguous wide mask cannot use rlwinm. MWCC forms it in
            // r0, keeps the loaded value in the next available register, and
            // lets `and.` both consume the mask and set CR0 for the branch.
            let low = mask as u16 as i16;
            let high = ((i64::from(mask) - i64::from(low)) >> 16) as i16;
            let high_register = self.fresh_virtual_general();
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(high_register, high));
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high_register,
                immediate: low,
            });
            let source = if let Some(register) = register_leaf {
                register
            } else {
                let register = self.fresh_virtual_general_preferring(5);
                self.evaluate_general(value, register)?;
                register
            };
            self.output.instructions.push(Instruction::AndRecord {
                a: GENERAL_SCRATCH,
                s: source,
                b: GENERAL_SCRATCH,
            });
        }
        Ok(true)
    }
}
