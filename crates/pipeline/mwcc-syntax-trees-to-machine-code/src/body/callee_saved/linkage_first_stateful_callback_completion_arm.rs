//! Physical layout for a stateful linkage-first callback completion arm.
//!
//! A command-specific arm can reuse the zero that cleared the retry counter,
//! publish an anchored replacement object, update an independent global state,
//! and complete the old object through its callback.  MWCC gives those values
//! the short-lived `r5`, `r0`, `r3`, and `r4` lanes respectively.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StatefulCompletionArm {
    start: usize,
    object: u8,
    call: usize,
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
) -> Option<StatefulCompletionArm> {
    if start < 5 {
        return None;
    }
    let [Instruction::AddImmediate {
        d: 5,
        a: 0,
        immediate: 0,
    }, Instruction::StoreWord { s: 5, a: 0, .. }, Instruction::LoadWord {
        d: command, a: 0, ..
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_command,
        immediate: 16,
    }, Instruction::BranchConditionalForward { .. }] = instructions.get(start - 5..start)?
    else {
        return None;
    };
    if command != compared_command
        || external_target_at(relocations, start - 4) != Some("NumInternalRetry")
        || external_target_at(relocations, start - 3) != Some("CurrCommand")
    {
        return None;
    }

    let [Instruction::AddImmediate {
        d: state,
        a: 0,
        immediate: 1,
    }, Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::StoreWord {
        s: stored_state,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: replacement,
        a: anchor,
        ..
    }, Instruction::StoreWord {
        s: published, a: 0, ..
    }, Instruction::AddImmediate {
        d: zero,
        a: 0,
        immediate: 0,
    }, Instruction::StoreWord {
        s: stored_zero,
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
    if state != stored_state
        || replacement != published
        || zero != stored_zero
        || object != state_object
        || object != callback_object
        || anchor == &0
        || external_target_at(relocations, start + 1) != Some("executing")
        || external_target_at(relocations, start + 2) != Some("MotorState")
        || external_target_at(relocations, start + 4) != Some("executing")
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 3)
    {
        return None;
    }

    let call = instructions[start + 8..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchToLinkRegisterAndLink))
        .map(|relative| start + 8 + relative)?;
    if call > start + 14
        || !instructions[start + 8..call]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { .. }))
        || !instructions[start + 8..call].iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 4,
                    a,
                    immediate: 0,
                } if a == object
            )
        })
        || instructions[start + 8..call].iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchAndLink { .. }
                    | Instruction::BranchToCountRegisterAndLink
                    | Instruction::Branch { .. }
            )
        })
    {
        return None;
    }

    Some(StatefulCompletionArm {
        start,
        object: *object,
        call,
    })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_stateful_callback_completion_arm(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 5;
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

            for instruction in &mut self.output.instructions[arm.start..=arm.call] {
                mwcc_vreg::for_each_register(instruction, |_, class, register| {
                    if class == mwcc_vreg::Class::General && *register == arm.object {
                        *register = 4;
                    }
                });
            }

            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[arm.start + 1]
            else {
                unreachable!("validated anchored replacement address changed form")
            };
            *d = 0;
            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[arm.start + 2]
            else {
                unreachable!("validated state constant changed form")
            };
            *d = 3;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[arm.start + 3]
            else {
                unreachable!("validated global state store changed form")
            };
            *s = 3;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[arm.start + 4]
            else {
                unreachable!("validated replacement publication changed form")
            };
            *s = 0;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[arm.start + 6]
            else {
                unreachable!("validated object state store changed form")
            };
            *s = 5;

            crate::remove_instruction_retargeting_to_next(self, arm.start + 5);
            let self_copy = self.output.instructions[arm.start + 6..arm.call]
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction,
                        Instruction::AddImmediate {
                            d: 4,
                            a: 4,
                            immediate: 0,
                        }
                    )
                })
                .map(|relative| arm.start + 6 + relative)
                .expect("recognized object copy should become a physical self-copy");
            crate::remove_instruction_retargeting_to_next(self, self_copy);
            start += 10;
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
    fn recognizes_a_stateful_completion_arm_with_a_dominating_zero() {
        let instructions = vec![
            Instruction::load_immediate(5, 0),
            Instruction::StoreWord {
                s: 5,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 16,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 19,
            },
            Instruction::load_immediate(0, 1),
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
                immediate: 64,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
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
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 18,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::AddImmediate {
                d: 4,
                a: 30,
                immediate: 0,
            },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegisterAndLink,
        ];
        let relocations = vec![
            relocation(1, "NumInternalRetry"),
            relocation(2, "CurrCommand"),
            relocation(6, "executing"),
            relocation(7, "MotorState"),
            relocation(9, "executing"),
        ];
        let displacements = vec![mwcc_machine_code::DataSectionDisplacement {
            instruction_index: 8,
            target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                "DummyCommandBlock".into(),
            ),
        }];

        assert_eq!(
            recognize_at(&instructions, &relocations, &displacements, 5),
            Some(StatefulCompletionArm {
                start: 5,
                object: 30,
                call: 18,
            })
        );
    }
}
