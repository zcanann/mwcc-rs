//! Final issue order for a disk-state validation transaction.
//!
//! Build 163 starts global receiver loads while independent frame-buffer and
//! callback addresses are still being formed. Selection keeps source order;
//! this allocation-aware owner interleaves the packets after physical homes
//! and relocations are known.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Debug, PartialEq)]
struct StateValidationSchedule {
    compare_entry_branch: usize,
    compare_call: usize,
    copy_call: usize,
    invalidate_call: usize,
    publications: Vec<StatePublication>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct StatePublication {
    high: usize,
    low: usize,
    receiver: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_state_validation_transaction(&mut self) {
        let diagnostic = std::env::var_os("MWCC_DIAGNOSTIC_STATE_VALIDATION").is_some_and(
            |requested| {
                requested == "*" || requested == std::ffi::OsStr::new(&self.output.name)
            },
        );
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            if diagnostic {
                eprintln!(
                    "state validation {} rejected non-linkage-first frame",
                    self.output.name
                );
            }
            return;
        }
        let plan = state_validation_schedule(
            &self.output.instructions,
            &self.output.relocations,
        );
        if diagnostic {
            eprintln!(
                "state validation {} plan={plan:?} relocations={:?}",
                self.output.name,
                self.output.relocations
            );
        }
        let Some(plan) = plan else {
            return;
        };

        for publication in plan.publications.iter().rev() {
            crate::move_instruction_before_retargeting(
                self,
                publication.receiver,
                publication.low,
            );
            let Instruction::AddImmediateShifted { d, .. } =
                &mut self.output.instructions[publication.high]
            else {
                unreachable!("validated state callback high half changed form")
            };
            *d = 4;
            let Instruction::AddImmediate { d, a, .. } =
                &mut self.output.instructions[publication.receiver]
            else {
                unreachable!("validated state callback low half changed form")
            };
            *d = 0;
            *a = 4;
        }

        let copied_receiver = plan.copy_call + 1;
        let copied_store = plan.copy_call + 3;
        let copied_buffer = plan.copy_call + 4;
        let Instruction::LoadWord { d, .. } =
            &mut self.output.instructions[copied_receiver]
        else {
            unreachable!("validated copied-state receiver changed form")
        };
        *d = 4;
        crate::move_instruction_before_retargeting(self, copied_buffer, copied_store);
        let Instruction::StoreWord { a, .. } =
            &mut self.output.instructions[copied_buffer]
        else {
            unreachable!("validated copied-state publication changed form")
        };
        *a = 4;

        crate::move_instruction_before_retargeting(
            self,
            plan.compare_call - 2,
            plan.compare_call - 3,
        );
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[plan.compare_entry_branch]
        else {
            unreachable!("validated state comparison entry branch changed form")
        };
        *target = plan.compare_call - 3;
    }
}

fn state_validation_schedule(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<StateValidationSchedule> {
    let compare_call = relocation_index(relocations, RelocationKind::Rel24, "DVDCompareDiskID")?;
    let copy_call = relocation_index(relocations, RelocationKind::Rel24, "memcpy")?;
    let invalidate_call =
        relocation_index(relocations, RelocationKind::Rel24, "DCInvalidateRange")?;
    let compare_entry_branch = compare_call.checked_sub(5)?;
    let compare_packet = instructions.get(compare_call - 3..compare_call)?;
    let copied_packet = instructions.get(copy_call + 1..invalidate_call)?;
    if copy_call != compare_call + 6
        || invalidate_call != copy_call + 6
        || !matches!(
            instructions.get(compare_entry_branch),
            Some(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target,
            }) if *target == compare_call - 3
        )
        || relocation_target_at(relocations, compare_call - 2, RelocationKind::EmbSda21)
            != Some("executing")
        || relocation_target_at(relocations, copy_call + 1, RelocationKind::EmbSda21)
            != Some("executing")
        || !state_compare_packet(compare_packet)
        || !copied_state_packet(copied_packet)
    {
        return None;
    }

    let publications: Vec<_> = relocations
        .iter()
        .filter_map(|relocation| {
            (relocation.kind == RelocationKind::EmbSda21
                && matches!(
                    &relocation.target,
                    mwcc_machine_code::RelocationTarget::External(name) if name == "LastState"
                ))
            .then_some(relocation.instruction_index)
        })
        .map(|store| state_publication(instructions, relocations, store))
        .collect::<Option<_>>()?;
    if publications.len() != 2 {
        return None;
    }

    Some(StateValidationSchedule {
        compare_entry_branch,
        compare_call,
        copy_call,
        invalidate_call,
        publications,
    })
}

fn state_compare_packet(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::AddImmediate { d: 3, .. },
            Instruction::LoadWord { d: 4, a: 0, .. },
            Instruction::LoadWord {
                d: 4,
                a: 4,
                offset: 36,
            },
        ]
    )
}

fn copied_state_packet(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::LoadWord {
                d: receiver,
                a: 0,
                ..
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: 0,
                a: store_receiver,
                offset: 12,
            },
            Instruction::AddImmediate { d: 3, .. },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 32,
            },
        ] if receiver == store_receiver
    )
}

fn state_publication(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    store: usize,
) -> Option<StatePublication> {
    let high = store.checked_sub(3)?;
    let low = store.checked_sub(2)?;
    let receiver = store.checked_sub(1)?;
    let callback = relocation_target_at(relocations, high, RelocationKind::Addr16Ha)?;
    if relocation_target_at(relocations, low, RelocationKind::Addr16Lo) != Some(callback)
        || relocation_target_at(relocations, receiver, RelocationKind::EmbSda21)
            != Some("executing")
        || relocation_target_at(relocations, store + 1, RelocationKind::Rel24) != Some(callback)
        || !state_publication_packet(instructions.get(high..store + 2)?)
    {
        return None;
    }
    Some(StatePublication {
        high,
        low,
        receiver,
    })
}

fn state_publication_packet(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::AddImmediateShifted {
                d: callback_high,
                a: 0,
                ..
            },
            Instruction::AddImmediate {
                d: 0,
                a: callback_low_base,
                ..
            },
            Instruction::LoadWord { d: 3, a: 0, .. },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::BranchAndLink { .. },
        ] if callback_high == callback_low_base
    )
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
    fn recognizes_state_validation_packets() {
        let mut instructions = vec![Instruction::BranchToLinkRegister; 56];
        instructions[8] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 10,
        };
        instructions[10] = Instruction::AddImmediate { d: 3, a: 31, immediate: 0 };
        instructions[11] = Instruction::LoadWord { d: 4, a: 0, offset: 0 };
        instructions[12] = Instruction::LoadWord { d: 4, a: 4, offset: 36 };
        instructions[13] = Instruction::BranchAndLink { target: "DVDCompareDiskID".into() };
        instructions[19] = Instruction::BranchAndLink { target: "memcpy".into() };
        instructions[20] = Instruction::LoadWord { d: 3, a: 0, offset: 0 };
        instructions[21] = Instruction::load_immediate(0, 1);
        instructions[22] = Instruction::StoreWord { s: 0, a: 3, offset: 12 };
        instructions[23] = Instruction::AddImmediate { d: 3, a: 31, immediate: 0 };
        instructions[24] = Instruction::load_immediate(4, 32);
        instructions[25] = Instruction::BranchAndLink { target: "DCInvalidateRange".into() };
        for (high, callback) in [(26, "next_a"), (46, "next_b")] {
            instructions[high] = Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            };
            instructions[high + 1] = Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            };
            instructions[high + 2] = Instruction::LoadWord { d: 3, a: 0, offset: 0 };
            instructions[high + 3] = Instruction::StoreWord { s: 0, a: 0, offset: 0 };
            instructions[high + 4] = Instruction::BranchAndLink { target: callback.into() };
        }
        let relocations = vec![
            relocation(11, RelocationKind::EmbSda21, "executing"),
            relocation(13, RelocationKind::Rel24, "DVDCompareDiskID"),
            relocation(19, RelocationKind::Rel24, "memcpy"),
            relocation(20, RelocationKind::EmbSda21, "executing"),
            relocation(25, RelocationKind::Rel24, "DCInvalidateRange"),
            relocation(26, RelocationKind::Addr16Ha, "next_a"),
            relocation(27, RelocationKind::Addr16Lo, "next_a"),
            relocation(28, RelocationKind::EmbSda21, "executing"),
            relocation(29, RelocationKind::EmbSda21, "LastState"),
            relocation(30, RelocationKind::Rel24, "next_a"),
            relocation(46, RelocationKind::Addr16Ha, "next_b"),
            relocation(47, RelocationKind::Addr16Lo, "next_b"),
            relocation(48, RelocationKind::EmbSda21, "executing"),
            relocation(49, RelocationKind::EmbSda21, "LastState"),
            relocation(50, RelocationKind::Rel24, "next_b"),
        ];

        let plan = state_validation_schedule(&instructions, &relocations)
            .expect("the state-validation transaction should be recognized");
        assert_eq!(plan.compare_entry_branch, 8);
        assert_eq!(plan.compare_call, 13);
        assert_eq!(plan.copy_call, 19);
        assert_eq!(plan.invalidate_call, 25);
        assert_eq!(plan.publications.len(), 2);
    }
}
