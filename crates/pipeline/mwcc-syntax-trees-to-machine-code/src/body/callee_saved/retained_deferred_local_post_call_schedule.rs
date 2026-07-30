//! Publication scheduling for a deferred saved local created after a call.
//!
//! The absolute replacement pointer, saved-local load, boolean publication,
//! and pointer publication are independent once the producing call returns.
//! Build 163 interleaves them to cover address-materialization latency.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_retained_deferred_local_post_call(&mut self) {
        if self.legacy_callee_saved_frame_layout
            != LegacyCalleeSavedFrameLayout::RetainDeferredLocalLane
        {
            return;
        }
        let Some(start) = post_call_deferred_local_publication(&self.output) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, start + 3, start);
        crate::move_instruction_before_retargeting(self, start + 2, start + 1);
        crate::move_instruction_before_retargeting(self, start + 4, start + 2);
        crate::move_instruction_before_retargeting(self, start + 5, start + 4);

        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[start + 3] else {
            unreachable!("the post-call publication boolean was matched")
        };
        *d = 3;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 5] else {
            unreachable!("the post-call boolean store was matched")
        };
        *s = 3;
    }
}

fn post_call_deferred_local_publication(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<usize> {
    let start = output.instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 0,
                    a: 0,
                    immediate: 1,
                },
                Instruction::LoadWord {
                    d: saved,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediateShifted {
                    d: high,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: low,
                    a: low_base,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: stored,
                    a: 0,
                    offset: 0,
                },
            ] if *saved >= 14 && high == low_base && low == stored
        )
    })?;
    let relocations = &output.relocations;
    let constants = &output.constants;
    if !super::super::schedule_relocations::same_target_value(
        relocations,
        constants,
        start + 3,
        start + 4,
    ) || !super::super::schedule_relocations::same_relocated_value(
        relocations,
        constants,
        start + 1,
        start + 5,
    ) || super::super::schedule_relocations::same_target_value(
        relocations,
        constants,
        start + 1,
        start + 2,
    ) {
        return None;
    }
    Some(start)
}
