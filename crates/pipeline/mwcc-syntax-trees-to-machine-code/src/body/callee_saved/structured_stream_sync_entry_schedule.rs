//! Final issue order for synchronous stream-command entry packets.
//!
//! Build 163 hoists the command literal across the linkage prologue, overlaps
//! the command store with callback address formation, and uses `r0` for the
//! callback low half. Selection and allocation own the semantic lifetimes;
//! this late owner applies the physical issue order after the frame is final.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StreamSyncEntrySchedule {
    callback_high: usize,
    callback_low: usize,
    command_literal: usize,
    command_store: usize,
    callback_store: usize,
    block_argument: usize,
    interrupt_result: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_stream_sync_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_callee_saved_frame_layout
                != LegacyCalleeSavedFrameLayout::RetainEntryParameterTableAndDeferredLocalLane
        {
            return;
        }
        let Some(plan) = stream_sync_entry_schedule(
            &self.output.instructions,
            &self.output.relocations,
        ) else {
            return;
        };

        crate::move_instruction_before_retargeting(self, plan.command_literal, 2);
        crate::move_instruction_before_retargeting(
            self,
            plan.command_store,
            plan.command_store - 1,
        );
        crate::move_instruction_before_retargeting(
            self,
            plan.block_argument,
            plan.callback_store,
        );
        self.output.instructions[plan.callback_store] = Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: 0,
        };

        let Instruction::AddImmediate { d, .. } =
            &mut self.output.instructions[plan.callback_low + 2]
        else {
            unreachable!("validated stream callback low half changed form")
        };
        *d = 0;
        let Instruction::StoreWord { s, .. } =
            &mut self.output.instructions[plan.callback_store + 1]
        else {
            unreachable!("validated stream callback store changed form")
        };
        *s = 0;
        self.output.instructions[plan.interrupt_result] = Instruction::Or {
            a: 31,
            s: 3,
            b: 3,
        };
    }
}

fn stream_sync_entry_schedule(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<StreamSyncEntrySchedule> {
    let issue_call = relocation_index(relocations, RelocationKind::Rel24, "issueCommand")?;
    let callback_high = issue_call.checked_sub(7)?;
    let callback_low = issue_call - 6;
    let command_literal = issue_call - 5;
    let command_store = issue_call - 4;
    let callback_store = issue_call - 3;
    let block_argument = issue_call - 1;
    let interrupt_call = relocation_index(
        relocations,
        RelocationKind::Rel24,
        "OSDisableInterrupts",
    )?;
    let interrupt_result = interrupt_call + 1;
    if callback_high != 6
        || !matches!(
            instructions.get(..issue_call + 1),
            Some([
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
                Instruction::StoreWord { s: 31, a: 1, offset: 28 },
                Instruction::StoreWord { s: 30, a: 1, offset: 24 },
                Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate { d: 3, a: 3, .. },
                Instruction::AddImmediate { d: 0, a: 0, .. },
                Instruction::StoreWord { s: 0, a: 30, offset: 8 },
                Instruction::StoreWord { s: 3, a: 30, offset: 40 },
                Instruction::AddImmediate { d: 3, a: 0, immediate: 1 },
                Instruction::Or { a: 4, s: 30, b: 30 },
                Instruction::BranchAndLink { .. },
            ])
        )
    {
        return None;
    }
    if interrupt_call != issue_call + 5
        || !matches!(
            instructions.get(interrupt_result),
            Some(Instruction::AddImmediate { d: 31, a: 3, immediate: 0 })
        )
    {
        return None;
    }
    let callback = relocation_target_at(relocations, callback_high, RelocationKind::Addr16Ha)?;
    if relocation_target_at(relocations, callback_low, RelocationKind::Addr16Lo) != Some(callback)
    {
        return None;
    }
    Some(StreamSyncEntrySchedule {
        callback_high,
        callback_low,
        command_literal,
        command_store,
        callback_store,
        block_argument,
        interrupt_result,
    })
}

fn relocation_index(
    relocations: &[mwcc_machine_code::Relocation],
    kind: RelocationKind,
    target: &str,
) -> Option<usize> {
    relocations.iter().find_map(|relocation| {
        (relocation.kind == kind
            && matches!(
                &relocation.target,
                mwcc_machine_code::RelocationTarget::External(name) if name == target
            ))
        .then_some(relocation.instruction_index)
    })
}

fn relocation_target_at(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index && relocation.kind == kind)
            .then(|| match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(name) => Some(name.as_str()),
                _ => None,
            })
            .flatten()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn recognizes_a_stream_sync_entry_packet() {
        let mut instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreWord { s: 31, a: 1, offset: 28 },
            Instruction::StoreWord { s: 30, a: 1, offset: 24 },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::load_immediate(0, 7),
            Instruction::StoreWord { s: 0, a: 30, offset: 8 },
            Instruction::StoreWord { s: 3, a: 30, offset: 40 },
            Instruction::load_immediate(3, 1),
            Instruction::Or { a: 4, s: 30, b: 30 },
            Instruction::BranchAndLink { target: "issueCommand".into() },
        ];
        instructions.extend([
            Instruction::BranchToLinkRegister,
            Instruction::BranchToLinkRegister,
            Instruction::BranchToLinkRegister,
            Instruction::BranchToLinkRegister,
            Instruction::BranchAndLink { target: "OSDisableInterrupts".into() },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
        ]);
        let relocations = vec![
            relocation(6, RelocationKind::Addr16Ha, "callback"),
            relocation(7, RelocationKind::Addr16Lo, "callback"),
            relocation(13, RelocationKind::Rel24, "issueCommand"),
            relocation(18, RelocationKind::Rel24, "OSDisableInterrupts"),
        ];

        assert_eq!(
            stream_sync_entry_schedule(&instructions, &relocations),
            Some(StreamSyncEntrySchedule {
                callback_high: 6,
                callback_low: 7,
                command_literal: 8,
                command_store: 9,
                callback_store: 10,
                block_argument: 12,
                interrupt_result: 19,
            })
        );
    }
}
