//! Linkage-first register splitting for callback completion arms.
//!
//! These arms replace the global current object, update the completed object's
//! state, and invoke its callback.  Give the path-local object and replacement
//! address separate virtual homes so allocation can use MWCC's short-lived
//! argument lanes instead of extending a shared switch-arm value into r30.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompletionArm {
    start: usize,
    object: u8,
    replacement: u8,
    call: usize,
    constant_first_argument: bool,
}

fn external_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

fn recognize_at(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
    start: usize,
) -> Option<CompletionArm> {
    let [Instruction::LoadWord {
        d: object, a: 0, ..
    }, Instruction::AddImmediate { d: replacement, .. }, Instruction::StoreWord {
        s: published, a: 0, ..
    }, Instruction::AddImmediate { d: state, a: 0, .. }, Instruction::StoreWord {
        s: stored_state,
        a: state_object,
        offset: 12,
    }, Instruction::LoadWord {
        a: callback_object,
        offset: 40,
        ..
    }] = instructions.get(start..start + 6)?
    else {
        return None;
    };
    if replacement != published
        || state != stored_state
        || object != state_object
        || object != callback_object
        || !displacements
            .iter()
            .any(|displacement| displacement.instruction_index == start + 1)
    {
        return None;
    }

    let load_target = relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == start && relocation.kind == RelocationKind::EmbSda21)
            .then(|| external_target(relocation))
            .flatten()
    })?;
    let store_target = relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == start + 2 && relocation.kind == RelocationKind::EmbSda21)
            .then(|| external_target(relocation))
            .flatten()
    })?;
    if load_target != store_target {
        return None;
    }

    let call = instructions[start + 6..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchToLinkRegisterAndLink))
        .map(|relative| start + 6 + relative)?;
    if call > start + 14
        || !instructions[start + 6..call]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { .. }))
        || instructions[start + 6..call].iter().any(|instruction| {
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
    let constant_first_argument = instructions[start + 6..call]
        .iter()
        .any(|instruction| matches!(instruction, Instruction::AddImmediate { a: 0, .. }));

    Some(CompletionArm {
        start,
        object: *object,
        replacement: *replacement,
        call,
        constant_first_argument,
    })
}

impl Generator {
    pub(crate) fn schedule_linkage_first_callback_completion_arms(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 6 <= self.output.instructions.len() {
            let Some(arm) = recognize_at(
                &self.output.instructions,
                &self.output.relocations,
                &self.output.data_section_displacements,
                start,
            ) else {
                start += 1;
                continue;
            };

            let object = self.fresh_virtual_general_preferring(4);
            let replacement = self
                .fresh_virtual_general_preferring(if arm.constant_first_argument { 3 } else { 5 });
            for instruction in &mut self.output.instructions[arm.start..=arm.call] {
                mwcc_vreg::for_each_register(instruction, |_, class, register| {
                    if class == mwcc_vreg::Class::General && *register == arm.object {
                        *register = object;
                    }
                });
            }
            let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[arm.start + 1]
            else {
                unreachable!("the recognized replacement address remains an addi");
            };
            *d = replacement;
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[arm.start + 2]
            else {
                unreachable!("the recognized replacement publication remains a store");
            };
            *s = replacement;
            crate::move_instruction_before_retargeting(self, arm.start + 3, arm.start + 2);
            start += 6;
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
            target: RelocationTarget::External(target.to_string()),
        }
    }

    #[test]
    fn recognizes_a_completion_publication_before_an_indirect_callback() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 33,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 32,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 33,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 0,
                a: 33,
                offset: 40,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 13,
            },
            Instruction::LoadWord {
                d: 12,
                a: 33,
                offset: 40,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegisterAndLink,
        ];
        let relocations = vec![relocation(0, "executing"), relocation(2, "executing")];
        let displacements = vec![mwcc_machine_code::DataSectionDisplacement {
            instruction_index: 1,
            target: mwcc_machine_code::DataSectionDisplacementTarget::Symbol("dummy".into()),
        }];

        assert_eq!(
            recognize_at(&instructions, &relocations, &displacements, 0),
            Some(CompletionArm {
                start: 0,
                object: 33,
                replacement: 0,
                call: 11,
                constant_first_argument: true,
            })
        );
    }
}
