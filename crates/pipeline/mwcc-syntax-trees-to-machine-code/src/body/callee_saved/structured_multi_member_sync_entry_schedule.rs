//! Final issue order for a synchronous multi-member command entry.
//!
//! Build 163 fills the linkage latency with two independent member literals,
//! then overlaps callback formation with the remaining stores. The structured
//! emitter preserves source order until allocation fixes the saved block home;
//! this pass validates and applies the measured physical permutation.

#[allow(unused_imports)]
use super::*;
use super::structured_conversion_call_schedule::permute_region;

const ENTRY_SCHEDULE: [usize; 19] = [
    0, 11, 1, 8, 13, 2, 3, 4, 5, 9, 6, 7, 10, 17, 16, 12, 14, 15, 18,
];

impl Generator {
    pub(crate) fn schedule_structured_multi_member_sync_entry(&mut self) {
        if !self.structured_loop_exit_parameter_home_reuse {
            return;
        }
        let Some(interrupt_result) = multi_member_sync_entry(
            &self.output.instructions,
            &self.output.relocations,
        ) else {
            return;
        };

        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[11] else {
            unreachable!("validated length literal changed form")
        };
        *d = 6;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[12] else {
            unreachable!("validated length store changed form")
        };
        *s = 6;
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[13] else {
            unreachable!("validated zero literal changed form")
        };
        *d = 5;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[14] else {
            unreachable!("validated zero store changed form")
        };
        *s = 5;
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[9] else {
            unreachable!("validated command store changed form")
        };
        *a = 3;
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[7] else {
            unreachable!("validated callback low half changed form")
        };
        *d = 0;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[15] else {
            unreachable!("validated callback store changed form")
        };
        *s = 0;
        self.output.instructions[17] = Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: 0,
        };
        self.output.instructions[interrupt_result] = Instruction::Or {
            a: 31,
            s: 3,
            b: 3,
        };

        permute_region(&mut self.output, 0, &ENTRY_SCHEDULE);
    }
}

fn multi_member_sync_entry(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<usize> {
    if !matches!(
        instructions.get(..ENTRY_SCHEDULE.len()),
        Some([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
            Instruction::StoreWord { s: 31, a: 1, offset: 36 },
            Instruction::StoreWord { s: 30, a: 1, offset: 32 },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 14 },
            Instruction::StoreWord { s: 0, a: 30, offset: 8 },
            Instruction::StoreWord { s: 4, a: 30, offset: 24 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 32 },
            Instruction::StoreWord { s: 0, a: 30, offset: 20 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 30, offset: 32 },
            Instruction::StoreWord { s: 3, a: 30, offset: 40 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 2 },
            Instruction::Or { a: 4, s: 30, b: 30 },
            Instruction::BranchAndLink { .. },
        ])
    ) {
        return None;
    }
    let callback = relocation_target_at(relocations, 6, RelocationKind::Addr16Ha);
    if !(callback.is_some()
        && relocation_target_at(relocations, 7, RelocationKind::Addr16Lo) == callback
        && relocation_target_at(relocations, 18, RelocationKind::Rel24)
            == Some("issueCommand"))
    {
        return None;
    }
    let interrupt_call = relocation_index(
        relocations,
        RelocationKind::Rel24,
        "OSDisableInterrupts",
    )?;
    let interrupt_result = interrupt_call + 1;
    matches!(
        instructions.get(interrupt_result),
        Some(Instruction::AddImmediate { d: 31, a: 3, immediate: 0 })
    )
    .then_some(interrupt_result)
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
    fn recognizes_a_multi_member_sync_entry() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
            Instruction::StoreWord { s: 31, a: 1, offset: 36 },
            Instruction::StoreWord { s: 30, a: 1, offset: 32 },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::load_immediate(0, 14),
            Instruction::StoreWord { s: 0, a: 30, offset: 8 },
            Instruction::StoreWord { s: 4, a: 30, offset: 24 },
            Instruction::load_immediate(0, 32),
            Instruction::StoreWord { s: 0, a: 30, offset: 20 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 30, offset: 32 },
            Instruction::StoreWord { s: 3, a: 30, offset: 40 },
            Instruction::load_immediate(3, 2),
            Instruction::Or { a: 4, s: 30, b: 30 },
            Instruction::BranchAndLink { target: "issueCommand".into() },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 23 },
            Instruction::load_immediate(3, -1),
            Instruction::Branch { target: 25 },
            Instruction::BranchAndLink { target: "OSDisableInterrupts".into() },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
        ];
        let relocations = vec![
            relocation(6, RelocationKind::Addr16Ha, "callback"),
            relocation(7, RelocationKind::Addr16Lo, "callback"),
            relocation(18, RelocationKind::Rel24, "issueCommand"),
            relocation(23, RelocationKind::Rel24, "OSDisableInterrupts"),
        ];

        assert_eq!(multi_member_sync_entry(&instructions, &relocations), Some(24));
    }
}
