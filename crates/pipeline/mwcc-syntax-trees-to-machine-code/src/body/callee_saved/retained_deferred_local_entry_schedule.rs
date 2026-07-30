//! Linkage-prefix scheduling for a state store before an early call guard.
//!
//! Build 163 fills the linkage latency slots with the incoming comparison and
//! stored constant before establishing the retained deferred-local frame.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_retained_deferred_local_entry(&mut self) {
        if self.legacy_callee_saved_frame_layout
            != LegacyCalleeSavedFrameLayout::RetainDeferredLocalLane
            || !matches!(
                self.output.instructions.get(0..9),
                Some([
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::StoreWord {
                        s: 0,
                        a: 1,
                        offset: 4,
                    },
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -24,
                    },
                    Instruction::StoreWord {
                        s: saved,
                        a: 1,
                        offset: 20,
                    },
                    Instruction::LoadWord {
                        d: loaded,
                        a: 0,
                        offset: 0,
                    },
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: -1,
                    },
                    Instruction::StoreWord {
                        s: 0,
                        a: stored_base,
                        ..
                    },
                    Instruction::CompareLogicalWordImmediate {
                        a: 3,
                        immediate: 16,
                    },
                    Instruction::BranchConditionalForward {
                        options: 4,
                        condition_bit: 2,
                        ..
                    },
                ]) if *saved >= 14 && loaded == stored_base
            )
        {
            return;
        }
        if !self.output.relocations.iter().any(|relocation| {
            relocation.instruction_index == 4 && relocation.kind == RelocationKind::EmbSda21
        }) {
            return;
        }

        crate::move_instruction_before_retargeting(self, 7, 1);
        crate::move_instruction_before_retargeting(self, 6, 3);
    }
}
