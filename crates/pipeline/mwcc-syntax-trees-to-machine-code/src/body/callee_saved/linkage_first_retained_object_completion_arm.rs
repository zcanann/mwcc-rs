//! Retain a linkage-first object across a guarded completion transaction.
//!
//! A progress guard either calls a busy-state helper or completes the same
//! object.  On the completion edge MWCC retains that object in `r3`, avoiding
//! both global reloads, then copies it to `r4` only when the callback argument
//! needs to replace `r3`.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainedCompletionArm {
    start: usize,
}

fn external_target_at<'a>(
    relocations: &'a [mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&'a str> {
    relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index && relocation.kind == kind)
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
) -> Option<RetainedCompletionArm> {
    let [Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::LoadWord {
        d: progress,
        a: progress_object,
        offset: 32,
    }, Instruction::LoadWord {
        d: expected,
        a: expected_object,
        offset: 20,
    }, Instruction::CompareLogicalWord {
        a: compared_progress,
        b: compared_expected,
    }, Instruction::BranchConditionalForward {
        target: completion, ..
    }, Instruction::LoadWord {
        d: reloaded, a: 0, ..
    }, Instruction::BranchAndLink { .. }, Instruction::Branch { .. }, Instruction::LoadWord {
        d: completed_object,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: replacement,
        a: anchor,
        ..
    }, Instruction::AddImmediate {
        d: zero,
        a: 0,
        immediate: 0,
    }, Instruction::StoreWord {
        s: published, a: 0, ..
    }, Instruction::StoreWord {
        s: stored_zero,
        a: state_object,
        offset: 12,
    }, Instruction::LoadWord {
        d: callback,
        a: callback_object,
        offset: 40,
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_callback,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: callback_join,
        ..
    }, Instruction::MoveToLinkRegister { s: linked_callback }, Instruction::LoadWord {
        d: callback_argument,
        a: argument_object,
        offset: 32,
    }, Instruction::BranchToLinkRegisterAndLink, Instruction::BranchAndLink { .. }] =
        instructions.get(start..start + 20)?
    else {
        return None;
    };

    if progress_object != object
        || expected_object != object
        || compared_progress != progress
        || compared_expected != expected
        || completion != &(start + 8)
        || reloaded != progress
        || completed_object != object
        || anchor == &0
        || replacement != published
        || zero != stored_zero
        || state_object != completed_object
        || callback_object != completed_object
        || callback != compared_callback
        || callback != linked_callback
        || callback_join != &(start + 19)
        || callback_argument != reloaded
        || argument_object != completed_object
        || external_target_at(relocations, start, RelocationKind::EmbSda21) != Some("executing")
        || external_target_at(relocations, start + 5, RelocationKind::EmbSda21) != Some("executing")
        || external_target_at(relocations, start + 6, RelocationKind::Rel24) != Some("stateBusy")
        || external_target_at(relocations, start + 8, RelocationKind::EmbSda21) != Some("executing")
        || external_target_at(relocations, start + 11, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 19, RelocationKind::Rel24) != Some("stateReady")
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 9)
    {
        return None;
    }

    Some(RetainedCompletionArm { start })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_retained_object_completion_arm(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 20 <= self.output.instructions.len() {
            let Some(arm) = recognize_at(
                &self.output.instructions,
                &self.output.relocations,
                &self.output.data_section_displacements,
                start,
            ) else {
                start += 1;
                continue;
            };

            crate::remove_instruction_retargeting_to_next(self, arm.start + 5);
            self.output
                .relocations
                .retain(|relocation| relocation.instruction_index != arm.start + 7);
            self.output.instructions[arm.start + 7] = Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: 0,
            };

            crate::move_instruction_before_retargeting(self, arm.start + 8, arm.start + 7);
            crate::move_instruction_before_retargeting(self, arm.start + 10, arm.start + 8);
            crate::move_instruction_before_retargeting(self, arm.start + 10, arm.start + 9);
            let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[arm.start + 4]
            else {
                unreachable!("validated progress guard changed form")
            };
            *target = arm.start + 7;

            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[arm.start] else {
                unreachable!("validated retained-object load changed form")
            };
            *d = 3;
            let Instruction::LoadWord { d, a, .. } = &mut self.output.instructions[arm.start + 1]
            else {
                unreachable!("validated progress load changed form")
            };
            *d = 4;
            *a = 3;
            let Instruction::LoadWord { d, a, .. } = &mut self.output.instructions[arm.start + 2]
            else {
                unreachable!("validated expected-progress load changed form")
            };
            *d = 0;
            *a = 3;
            self.output.instructions[arm.start + 3] =
                Instruction::CompareLogicalWord { a: 4, b: 0 };

            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[arm.start + 7]
            else {
                unreachable!("validated replacement address changed form")
            };
            *d = 0;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[arm.start + 8]
            else {
                unreachable!("validated replacement publication changed form")
            };
            *s = 0;
            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[arm.start + 9]
            else {
                unreachable!("validated zero constant changed form")
            };
            *d = 0;
            self.output.instructions[arm.start + 10] = Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: 0,
            };
            let Instruction::StoreWord { s, a, .. } = &mut self.output.instructions[arm.start + 11]
            else {
                unreachable!("validated object state store changed form")
            };
            *s = 0;
            *a = 3;
            let Instruction::LoadWord { a, .. } = &mut self.output.instructions[arm.start + 12]
            else {
                unreachable!("validated callback load changed form")
            };
            *a = 3;
            let Instruction::LoadWord { d, a, .. } = &mut self.output.instructions[arm.start + 16]
            else {
                unreachable!("validated callback argument load changed form")
            };
            *d = 3;
            *a = 4;
            crate::move_instruction_before_retargeting(self, arm.start + 16, arm.start + 15);
            debug_assert!(matches!(
                self.output.instructions[arm.start + 4],
                Instruction::BranchConditionalForward { target, .. }
                    if target == arm.start + 7
            ));

            start += 19;
        }
    }
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
    fn recognizes_a_reloaded_object_completion_transaction() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 20,
            },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::BranchAndLink {
                target: "stateBusy".into(),
            },
            Instruction::Branch { target: 20 },
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 31,
                immediate: 64,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 5,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 12,
                a: 4,
                offset: 40,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 19,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 32,
            },
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::BranchAndLink {
                target: "stateReady".into(),
            },
        ];
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "executing"),
            relocation(5, RelocationKind::EmbSda21, "executing"),
            relocation(6, RelocationKind::Rel24, "stateBusy"),
            relocation(8, RelocationKind::EmbSda21, "executing"),
            relocation(11, RelocationKind::EmbSda21, "executing"),
            relocation(19, RelocationKind::Rel24, "stateReady"),
        ];
        let displacements = vec![mwcc_machine_code::DataSectionDisplacement {
            instruction_index: 9,
            target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                "DummyCommandBlock".into(),
            ),
        }];

        assert_eq!(
            recognize_at(&instructions, &relocations, &displacements, 0),
            Some(RetainedCompletionArm { start: 0 })
        );
    }
}
