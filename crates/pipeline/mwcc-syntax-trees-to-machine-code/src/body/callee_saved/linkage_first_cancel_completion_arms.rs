//! Physical issue order for linkage-first cancellation completion arms.
//!
//! A cancellation arm clears the cancellation flag, publishes the anchored
//! dummy command block, changes the completed object's state, and invokes its
//! callbacks. MWCC issues the independent object load and replacement address
//! before the zero/store pair and uses `r3` for the published address.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CancelCompletionArm {
    start: usize,
    object: u8,
}

fn external_target_at<'a>(
    relocations: &'a [mwcc_machine_code::Relocation],
    instruction_index: usize,
) -> Option<&'a str> {
    relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index
            && relocation.kind == RelocationKind::EmbSda21)
            .then(|| match &relocation.target {
                mwcc_machine_code::RelocationTarget::External(target) => Some(target.as_str()),
                _ => None,
            })
            .flatten()
    })
}

fn recognize_at(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
    start: usize,
) -> Option<CancelCompletionArm> {
    let [Instruction::AddImmediate {
        d: 0,
        a: 0,
        immediate: 0,
    }, Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::StoreWord { s: 0, a: 0, .. }, Instruction::AddImmediate {
        d: 0, a: anchor, ..
    }, Instruction::StoreWord { s: 0, a: 0, .. }, Instruction::AddImmediate {
        d: 0,
        a: 0,
        immediate: 10,
    }, Instruction::StoreWord {
        s: 0,
        a: state_object,
        offset: 12,
    }, Instruction::LoadWord {
        a: callback_object,
        offset: 40,
        ..
    }] = instructions.get(start..start + 8)?
    else {
        return None;
    };
    if anchor == &0
        || object != state_object
        || object != callback_object
        || external_target_at(relocations, start + 1) != Some("executing")
        || external_target_at(relocations, start + 2) != Some("Canceling")
        || external_target_at(relocations, start + 4) != Some("executing")
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 3)
    {
        return None;
    }
    Some(CancelCompletionArm {
        start,
        object: *object,
    })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_cancel_completion_arms(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 8 <= self.output.instructions.len() {
            let Some(arm) = recognize_at(
                &self.output.instructions,
                &self.output.relocations,
                &self.output.data_section_displacements,
                start,
            ) else {
                start += 1;
                continue;
            };

            crate::move_instruction_before_retargeting(self, arm.start + 1, arm.start);
            crate::move_instruction_before_retargeting(self, arm.start + 3, arm.start + 1);
            crate::move_instruction_before_retargeting(self, arm.start + 5, arm.start + 4);

            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[arm.start + 1]
            else {
                unreachable!("validated anchored replacement address changed form")
            };
            *d = 3;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[arm.start + 5]
            else {
                unreachable!("validated replacement publication changed form")
            };
            *s = 3;
            start += 8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn relocation(instruction_index: usize, target: &str) -> Relocation {
        Relocation {
            instruction_index,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn recognizes_an_anchored_cancellation_completion_arm() {
        let instructions = vec![
            Instruction::load_immediate(0, 0),
            Instruction::LoadWord {
                d: 30,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 31,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 10),
            Instruction::StoreWord {
                s: 0,
                a: 30,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 12,
                a: 30,
                offset: 40,
            },
        ];
        let relocations = vec![
            relocation(1, "executing"),
            relocation(2, "Canceling"),
            relocation(4, "executing"),
        ];
        let displacements = vec![mwcc_machine_code::DataSectionDisplacement {
            instruction_index: 3,
            target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                "DummyCommandBlock".into(),
            ),
        }];

        assert_eq!(
            recognize_at(&instructions, &relocations, &displacements, 0),
            Some(CancelCompletionArm {
                start: 0,
                object: 30,
            })
        );
    }
}
