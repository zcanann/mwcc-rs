//! Final issue order for a pair of inlined interrupt transactions.
//!
//! Build 163 composes a guarded pause transaction and a guarded resume
//! transaction around a queue-draining loop. After allocation it forwards the
//! queue call result directly into the loop test, retains the loaded executing
//! pointer for the following cancel call, and reuses the expired callback home
//! for the second interrupt token.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_inlined_interrupt_transaction(&mut self) {
        if self.inline_statement_body_substitutions < 2
            || !is_inlined_interrupt_transaction(
                &self.output.instructions,
                &self.output.relocations,
            )
        {
            return;
        }

        // The queue result is already in r3. Remove the temporary r0 round trip
        // and compare the ABI result directly.
        crate::remove_instruction_retargeting_to_next(self, 28);
        crate::remove_instruction_retargeting_to_next(self, 22);
        crate::remove_instruction_retargeting_to_next(self, 21);
        let Instruction::CompareLogicalWordImmediate { a, .. } = &mut self.output.instructions[21]
        else {
            unreachable!("validated queue-result compare changed form")
        };
        *a = 3;

        // Keep the executing pointer loaded in r3 through its null test and
        // into DVDCancelAsync rather than issuing a second global load.
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[23] else {
            unreachable!("validated executing load changed form")
        };
        *d = 3;
        let Instruction::CompareLogicalWordImmediate { a, .. } = &mut self.output.instructions[24]
        else {
            unreachable!("validated executing compare changed form")
        };
        *a = 3;

        // MWCC records saved call results with register moves, then schedules
        // the fallback constant and indirect-call linkage around their uses.
        self.output.instructions[8] = Instruction::Or { a: 30, s: 3, b: 3 };
        self.output.instructions[28] = Instruction::Or { a: 29, s: 3, b: 3 };
        crate::move_instruction_before_retargeting(self, 31, 30);
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[25]
        else {
            unreachable!("validated executing-null branch changed form")
        };
        *target = 30;
        crate::move_instruction_before_retargeting(self, 36, 34);
        crate::move_instruction_before_retargeting(self, 52, 51);
    }
}

fn is_inlined_interrupt_transaction(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> bool {
    instructions.len() == 62
        && call_target(relocations, 7) == Some("OSDisableInterrupts")
        && call_target(relocations, 9) == Some("OSDisableInterrupts")
        && call_target(relocations, 16) == Some("OSRestoreInterrupts")
        && call_target(relocations, 19) == Some("DVDCancelAsync")
        && call_target(relocations, 20) == Some("__DVDPopWaitingQueue")
        && call_target(relocations, 30) == Some("DVDCancelAsync")
        && call_target(relocations, 41) == Some("OSDisableInterrupts")
        && call_target(relocations, 49) == Some("stateReady")
        && call_target(relocations, 51) == Some("OSRestoreInterrupts")
        && call_target(relocations, 53) == Some("OSRestoreInterrupts")
        && matches!(
            instructions.get(21..25),
            Some([
                Instruction::AddImmediate {
                    d: 0,
                    a: 3,
                    immediate: 0,
                },
                Instruction::Or { a: 3, s: 0, b: 0 },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
            ])
        )
        && matches!(
            instructions.get(25..31),
            Some([
                Instruction::LoadWord { d: 0, .. },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadWord { d: 3, .. },
                Instruction::Or { a: 4, s: 31, b: 31 },
                Instruction::BranchAndLink { .. },
            ])
        )
}

fn call_target(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index
            || relocation.kind != RelocationKind::Rel24
        {
            return None;
        }
        let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
            return None;
        };
        Some(target.as_str())
    })
}
