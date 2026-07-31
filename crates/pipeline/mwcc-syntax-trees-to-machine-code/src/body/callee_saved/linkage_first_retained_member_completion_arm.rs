//! Retain an object and one member address across a linkage-first completion diamond.
//!
//! A command-specific arm tests an object member, then a fixed-bank status bit.
//! MWCC keeps the object in `r4` and its tested member address in `r5`. The
//! callback edge completes through `r4`; the other edge stores through `r5`
//! and reloads the object only once for two call arguments.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RetainedMemberCompletionArm {
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
) -> Option<RetainedMemberCompletionArm> {
    let [Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::LoadWord {
        d: tested,
        a: tested_object,
        offset: 28,
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_member,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: next_arm, ..
    }, Instruction::AddImmediateShifted {
        d: fixed_base,
        a: 0,
        ..
    }, Instruction::LoadWord {
        d: fixed_value,
        a: loaded_fixed_base,
        ..
    }, Instruction::AndMaskRecord {
        a: masked,
        s: masked_value,
        ..
    }, Instruction::BranchConditionalForward {
        target: alternate, ..
    }] = instructions.get(start..start + 8)?
    else {
        return None;
    };
    if tested_object != object
        || compared_member != tested
        || next_arm != &(start + 35)
        || fixed_base != loaded_fixed_base
        || fixed_value != masked
        || fixed_value != masked_value
        || alternate != &(start + 21)
        || external_target_at(relocations, start, RelocationKind::EmbSda21) != Some("executing")
    {
        return None;
    }

    let [Instruction::LoadWord {
        d: callback_object,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: replacement,
        a: anchor,
        ..
    }, Instruction::AddImmediate {
        d: state,
        a: 0,
        immediate: 9,
    }, Instruction::StoreWord {
        s: published, a: 0, ..
    }, Instruction::StoreWord {
        s: stored_state,
        a: state_object,
        offset: 12,
    }, Instruction::LoadWord {
        d: callback,
        a: callback_base,
        offset: 40,
    }, Instruction::CompareLogicalWordImmediate {
        a: compared_callback,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: callback_join,
        ..
    }, Instruction::MoveToLinkRegister { s: linked_callback }, Instruction::AddImmediate {
        d: callback_result,
        a: 0,
        immediate: -2,
    }, Instruction::BranchToLinkRegisterAndLink, Instruction::BranchAndLink { .. }, Instruction::Branch { target: true_exit }] =
        instructions.get(start + 8..start + 21)?
    else {
        return None;
    };
    if anchor == &0
        || replacement != published
        || state != stored_state
        || callback_object != state_object
        || callback_object != callback_base
        || callback != compared_callback
        || callback != linked_callback
        || callback_result != &3
        || callback_join != &(start + 19)
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

    let [Instruction::AddImmediate {
        d: zero,
        a: 0,
        immediate: 0,
    }, Instruction::LoadWord {
        d: member_object,
        a: 0,
        ..
    }, Instruction::StoreWord {
        s: stored_zero,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: one,
        a: 0,
        immediate: 1,
    }, Instruction::StoreWord {
        s: stored_one,
        a: stored_member_object,
        offset: 28,
    }, Instruction::AddImmediate {
        d: call_result,
        a: 0,
        immediate: 0,
    }, Instruction::LoadWord {
        d: first_reload,
        a: 0,
        ..
    }, Instruction::LoadWord {
        d: first_argument,
        a: first_argument_object,
        offset: 20,
    }, Instruction::LoadWord {
        d: second_reload,
        a: 0,
        ..
    }, Instruction::LoadWord {
        d: second_argument,
        a: second_argument_object,
        offset: 16,
    }, Instruction::AddImmediateShifted {
        d: callback_high,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: callback_address,
        a: callback_high_source,
        ..
    }, Instruction::BranchAndLink { .. }, Instruction::Branch {
        target: alternate_exit,
    }] = instructions.get(start + 21..start + 35)?
    else {
        return None;
    };
    if zero != stored_zero
        || one != stored_one
        || member_object != stored_member_object
        || first_reload != first_argument
        || second_reload != second_argument
        || first_reload == second_reload
        || first_argument_object != first_reload
        || second_argument_object != second_reload
        || callback_high != callback_address
        || callback_high != callback_high_source
        || true_exit != alternate_exit
        || external_target_at(relocations, start + 22, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 23, RelocationKind::EmbSda21)
            != Some("AutoFinishing")
        || external_target_at(relocations, start + 27, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 29, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 31, RelocationKind::Addr16Ha)
            != Some("cbForStateBusy")
        || external_target_at(relocations, start + 32, RelocationKind::Addr16Lo)
            != Some("cbForStateBusy")
        || external_target_at(relocations, start + 33, RelocationKind::Rel24)
            != Some("DVDLowAudioStream")
        || call_result != &3
    {
        return None;
    }

    let [Instruction::LoadWord {
        d: next_object,
        a: 0,
        ..
    }, Instruction::AddImmediate {
        d: next_replacement,
        a: next_anchor,
        ..
    }, Instruction::AddImmediate {
        d: next_zero,
        a: 0,
        immediate: 0,
    }, Instruction::StoreWord {
        s: next_published,
        a: 0,
        ..
    }, Instruction::StoreWord {
        s: next_stored_zero,
        a: next_state_object,
        offset: 12,
    }, Instruction::LoadWord {
        d: next_callback,
        a: next_callback_object,
        offset: 40,
    }, Instruction::CompareLogicalWordImmediate {
        a: next_compared_callback,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        target: next_callback_join,
        ..
    }, Instruction::MoveToLinkRegister {
        s: next_linked_callback,
    }, Instruction::AddImmediate {
        d: next_callback_result,
        a: 0,
        immediate: 0,
    }, Instruction::BranchToLinkRegisterAndLink, Instruction::BranchAndLink { .. }, Instruction::Branch { target: next_exit }] =
        instructions.get(start + 35..start + 48)?
    else {
        return None;
    };
    if next_anchor == &0
        || next_replacement != next_published
        || next_zero != next_stored_zero
        || next_object != next_state_object
        || next_object != next_callback_object
        || next_callback != next_compared_callback
        || next_callback != next_linked_callback
        || next_callback_result != &3
        || next_callback_join != &(start + 46)
        || next_exit != true_exit
        || external_target_at(relocations, start + 35, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 38, RelocationKind::EmbSda21)
            != Some("executing")
        || external_target_at(relocations, start + 46, RelocationKind::Rel24) != Some("stateReady")
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 36)
    {
        return None;
    }

    Some(RetainedMemberCompletionArm { start })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_retained_member_completion_arm(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 48 <= self.output.instructions.len() {
            let Some(arm) = recognize_at(
                &self.output.instructions,
                &self.output.relocations,
                &self.output.data_section_displacements,
                start,
            ) else {
                start += 1;
                continue;
            };
            let base = arm.start;

            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[base] else {
                unreachable!("validated retained object load changed form")
            };
            *d = 4;
            crate::insert_instruction_retargeting(
                self,
                base + 1,
                Instruction::AddImmediate {
                    d: 5,
                    a: 4,
                    immediate: 28,
                },
            );
            let Instruction::LoadWord { a, .. } = &mut self.output.instructions[base + 2] else {
                unreachable!("validated tested member load changed form")
            };
            *a = 4;

            crate::remove_instruction_retargeting_to_next(self, base + 9);
            crate::move_instruction_before_retargeting(self, base + 11, base + 10);
            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[base + 9]
            else {
                unreachable!("validated replacement address changed form")
            };
            *d = 0;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[base + 10] else {
                unreachable!("validated replacement publication changed form")
            };
            *s = 0;

            crate::remove_instruction_retargeting_to_next(self, base + 22);
            crate::move_instruction_before_retargeting(self, base + 30, base + 24);
            crate::move_instruction_before_retargeting(self, base + 31, base + 26);
            crate::remove_instruction_retargeting_to_next(self, base + 30);

            let Instruction::AddImmediateShifted { d, .. } =
                &mut self.output.instructions[base + 24]
            else {
                unreachable!("validated callback high half changed form")
            };
            *d = 3;
            let Instruction::StoreWord { a, offset, .. } = &mut self.output.instructions[base + 25]
            else {
                unreachable!("validated retained member store changed form")
            };
            *a = 5;
            *offset = 0;
            let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[base + 26]
            else {
                unreachable!("validated callback low half changed form")
            };
            *a = 3;
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[base + 28] else {
                unreachable!("validated shared callback object load changed form")
            };
            *d = 5;
            let Instruction::LoadWord { a, .. } = &mut self.output.instructions[base + 29] else {
                unreachable!("validated first callback argument load changed form")
            };
            *a = 5;

            crate::remove_instruction_retargeting_to_next(self, base + 33);
            crate::move_instruction_before_retargeting(self, base + 35, base + 34);
            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[base + 33]
            else {
                unreachable!("validated next replacement address changed form")
            };
            *d = 0;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[base + 34] else {
                unreachable!("validated next replacement publication changed form")
            };
            *s = 0;

            debug_assert!(matches!(
                self.output.instructions[base + 1],
                Instruction::AddImmediate {
                    d: 5,
                    a: 4,
                    immediate: 28
                }
            ));
            start += 45;
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
    fn recognizes_a_reloaded_member_completion_diamond() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 28,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 35,
            },
            Instruction::load_immediate_shifted(3, -13312),
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 24608,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 21,
            },
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 64,
            },
            Instruction::load_immediate(0, 9),
            Instruction::StoreWord {
                s: 3,
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
            Instruction::load_immediate(3, -2),
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::BranchAndLink {
                target: "stateReady".into(),
            },
            Instruction::Branch { target: 53 },
            Instruction::load_immediate(0, 0),
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 1),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 28,
            },
            Instruction::load_immediate(3, 0),
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: 4,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 5,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: 5,
                offset: 16,
            },
            Instruction::load_immediate_shifted(6, 0),
            Instruction::AddImmediate {
                d: 6,
                a: 6,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "DVDLowAudioStream".into(),
            },
            Instruction::Branch { target: 53 },
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 64,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 3,
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
                target: 46,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::BranchAndLink {
                target: "stateReady".into(),
            },
            Instruction::Branch { target: 53 },
        ];
        instructions.extend((0..6).map(|_| Instruction::load_immediate(0, 0)));
        let relocations = vec![
            relocation(0, RelocationKind::EmbSda21, "executing"),
            relocation(8, RelocationKind::EmbSda21, "executing"),
            relocation(11, RelocationKind::EmbSda21, "executing"),
            relocation(19, RelocationKind::Rel24, "stateReady"),
            relocation(22, RelocationKind::EmbSda21, "executing"),
            relocation(23, RelocationKind::EmbSda21, "AutoFinishing"),
            relocation(27, RelocationKind::EmbSda21, "executing"),
            relocation(29, RelocationKind::EmbSda21, "executing"),
            relocation(31, RelocationKind::Addr16Ha, "cbForStateBusy"),
            relocation(32, RelocationKind::Addr16Lo, "cbForStateBusy"),
            relocation(33, RelocationKind::Rel24, "DVDLowAudioStream"),
            relocation(35, RelocationKind::EmbSda21, "executing"),
            relocation(38, RelocationKind::EmbSda21, "executing"),
            relocation(46, RelocationKind::Rel24, "stateReady"),
        ];
        let displacements = vec![
            mwcc_machine_code::DataSectionDisplacement {
                instruction_index: 9,
                target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                    "DummyCommandBlock".into(),
                ),
            },
            mwcc_machine_code::DataSectionDisplacement {
                instruction_index: 36,
                target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol(
                    "DummyCommandBlock".into(),
                ),
            },
        ];

        assert_eq!(
            recognize_at(&instructions, &relocations, &displacements, 0),
            Some(RetainedMemberCompletionArm { start: 0 })
        );
        instructions[25] = Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 28,
        };
        assert_eq!(
            recognize_at(&instructions, &relocations, &displacements, 0),
            None
        );
    }
}
