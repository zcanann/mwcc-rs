//! Final call-packet order for a retained pre-composition value graph.
//!
//! Once allocation has fixed the caller homes, build 163 fills the address
//! latency slots of the recovery seek and loads the indirect fallback target
//! before its receiver.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct PrecompositionTailSchedule {
    seek_call: usize,
    last_state: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_precomposition_tail(&mut self) {
        if self.inline_source_call_survivors.len() < 2 {
            return;
        }
        let Some(plan) =
            precomposition_tail_schedule(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };

        let seek = plan.seek_call;
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[seek - 4] else {
            unreachable!("validated executing load changed form")
        };
        *d = 5;
        let Instruction::LoadWord { d, a, .. } = &mut self.output.instructions[seek - 3] else {
            unreachable!("validated seek member load changed form")
        };
        *d = 3;
        *a = 5;
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[seek - 2]
        else {
            unreachable!("validated callback high half changed form")
        };
        *d = 3;
        let Instruction::AddImmediate { d, a, .. } = &mut self.output.instructions[seek - 1] else {
            unreachable!("validated callback low half changed form")
        };
        *d = 4;
        *a = 3;
        crate::move_instruction_before_retargeting(self, seek - 2, seek - 3);
        crate::move_instruction_before_retargeting(self, seek - 1, seek - 2);
        crate::move_instruction_before_retargeting(self, plan.last_state, plan.last_state - 1);
        let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[seek - 5]
        else {
            unreachable!("validated recovery fallback branch changed form")
        };
        *target = plan.last_state - 1;
    }
}

fn precomposition_tail_schedule(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<PrecompositionTailSchedule> {
    let seek_call = relocation_index(relocations, RelocationKind::Rel24, "DVDLowSeek")?;
    let last_state = relocation_index(relocations, RelocationKind::EmbSda21, "LastState")?;
    if seek_call < 5
        || last_state == 0
        || !matches!(
            instructions.get(seek_call - 5),
            Some(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target,
            }) if *target == last_state - 1
        )
        || relocation_target_at(relocations, seek_call - 4, RelocationKind::EmbSda21)
            != Some("executing")
        || relocation_target_at(relocations, seek_call - 2, RelocationKind::Addr16Ha)
            != Some("cbForUnrecoveredError")
        || relocation_target_at(relocations, seek_call - 1, RelocationKind::Addr16Lo)
            != Some("cbForUnrecoveredError")
        || relocation_target_at(relocations, last_state - 1, RelocationKind::EmbSda21)
            != Some("executing")
        || !matches!(
            instructions.get(seek_call - 4..seek_call),
            Some([
                Instruction::LoadWord { d: 3, a: 0, .. },
                Instruction::LoadWord {
                    d: 3,
                    a: 3,
                    offset: 16,
                },
                Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                Instruction::AddImmediate { d: 4, a: 4, .. },
            ])
        )
        || !matches!(
            instructions.get(last_state - 1..last_state + 3),
            Some([
                Instruction::LoadWord { d: 3, a: 0, .. },
                Instruction::LoadWord { d: 12, a: 0, .. },
                Instruction::MoveToLinkRegister { s: 12 },
                Instruction::BranchToLinkRegisterAndLink,
            ])
        )
    {
        return None;
    }

    Some(PrecompositionTailSchedule {
        seek_call,
        last_state,
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
    fn recognizes_seek_and_indirect_fallback_packets() {
        let instructions = vec![
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: 16,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "DVDLowSeek".into(),
            },
            Instruction::Branch { target: 11 },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 12,
                a: 0,
                offset: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ];
        let relocations = vec![
            relocation(1, RelocationKind::EmbSda21, "executing"),
            relocation(3, RelocationKind::Addr16Ha, "cbForUnrecoveredError"),
            relocation(4, RelocationKind::Addr16Lo, "cbForUnrecoveredError"),
            relocation(5, RelocationKind::Rel24, "DVDLowSeek"),
            relocation(7, RelocationKind::EmbSda21, "executing"),
            relocation(8, RelocationKind::EmbSda21, "LastState"),
        ];

        assert_eq!(
            precomposition_tail_schedule(&instructions, &relocations),
            Some(PrecompositionTailSchedule {
                seek_call: 5,
                last_state: 8,
            })
        );
    }
}
