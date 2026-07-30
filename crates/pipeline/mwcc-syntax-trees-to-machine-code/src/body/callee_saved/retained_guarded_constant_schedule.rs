//! Final issue order for a guarded constant retained across a call.
//!
//! The semantic normalizer and structured allocator own the value lifetime,
//! saved home, and frame. This pass only applies Build-163's measured issue
//! order after physical allocation has made the complete region explicit.

use super::*;

impl Generator {
    pub(crate) fn schedule_retained_guarded_constant(&mut self) {
        if self.legacy_callee_saved_frame_layout
            != LegacyCalleeSavedFrameLayout::RetainDeferredLocalLane
        {
            return;
        }

        if matches!(
            self.output.instructions.get(0..5),
            Some([
                Instruction::MoveFromLinkRegister { .. },
                Instruction::StoreWord { s: 0, a: 1, .. },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::StoreWord { s: 31, a: 1, .. },
                Instruction::CompareLogicalWordImmediate { .. }
                    | Instruction::CompareWordImmediate { .. },
            ])
        ) {
            crate::move_instruction_before_retargeting(self, 4, 1);
        }

        let Some(load) = self.output.instructions.windows(3).position(|window| {
            matches!(
                window,
                [
                    Instruction::BranchAndLink { .. },
                    Instruction::LoadWord { d: 4, a: 0, .. },
                    Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, load + 2, load + 1);
    }
}
