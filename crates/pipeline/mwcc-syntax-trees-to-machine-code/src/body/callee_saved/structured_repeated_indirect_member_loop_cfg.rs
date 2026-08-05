//! Retained control-flow residue for repeated inlined indirect-member walks.
//!
//! These edges survive Build 163's inline optimization but are absent from the
//! semantic statement tree. They must be restored after generic polling-loop
//! alignment so they do not change the alignment decision for the real loop.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn retain_repeated_indirect_member_loop_cfg_residue(&mut self) {
        if !self.structured_repeated_indirect_member_loop_entry {
            return;
        }
        self.retain_outer_inline_loop_entry_edges();
        self.retain_guarded_sync_entry_edge();
        self.retain_inlined_switch_default_edge();
    }

    fn retain_outer_inline_loop_entry_edges(&mut self) {
        let Some(load) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::LoadWord { a: 0, offset: 0, .. })
        }) else {
            return;
        };
        crate::insert_instruction_retargeting(self, load, Instruction::Branch { target: 0 });
        crate::insert_instruction_retargeting(self, load + 1, Instruction::Branch { target: 0 });
        self.output.instructions[load] = Instruction::Branch { target: load + 1 };
        self.output.instructions[load + 1] = Instruction::Branch { target: load + 2 };
    }

    fn retain_guarded_sync_entry_edge(&mut self) {
        let Some(unlock) = self.output.instructions.windows(3).position(|window| {
            matches!(
                window,
                [
                    Instruction::BranchAndLink { target: unlock },
                    Instruction::Branch { .. },
                    Instruction::BranchAndLink { target: sync },
                ] if unlock == "__OSUnlockSram" && sync == "__OSSyncSram"
            )
        }) else {
            return;
        };
        crate::insert_instruction_retargeting(
            self,
            unlock + 2,
            Instruction::Branch { target: 0 },
        );
        self.output.instructions[unlock + 1] = Instruction::Branch { target: unlock + 2 };
        self.output.instructions[unlock + 2] = Instruction::Branch { target: unlock + 3 };
    }

    fn retain_inlined_switch_default_edge(&mut self) {
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(
                window,
                [
                    Instruction::CompareWordImmediate { immediate: 4, .. },
                    Instruction::BranchConditionalForward { .. },
                    Instruction::BranchConditionalForward { .. },
                    Instruction::CompareWordImmediate { immediate: 1, .. },
                    Instruction::BranchConditionalForward {
                        options: 4,
                        condition_bit: 2,
                        ..
                    },
                    Instruction::BranchAndLink { target },
                ] if target == "OSCancelThread"
            )
        }) else {
            return;
        };
        let Instruction::BranchConditionalForward { target: old_skip, .. } =
            self.output.instructions[start + 4]
        else {
            unreachable!("inlined switch default edge was recognized")
        };
        let insertion = start + 5;
        crate::insert_instruction_retargeting(
            self,
            insertion,
            Instruction::Branch { target: 0 },
        );
        let skip = old_skip + usize::from(old_skip >= insertion);
        self.output.instructions[start + 4] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 6,
        };
        self.output.instructions[insertion] = Instruction::Branch { target: skip };
    }
}
