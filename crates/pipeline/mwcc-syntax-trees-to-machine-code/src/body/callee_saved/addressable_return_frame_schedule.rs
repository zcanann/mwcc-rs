//! Physical frame schedule for a call result returned through an escaped local.
//!
//! The structured frame owner emits a safe predecrement layout. Once physical
//! ABI lanes are known, newer MWCC moves the local address next to the call;
//! build 163 additionally uses its linkage-first ordering and lower local slot.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    pub(crate) fn schedule_addressable_return_frame(&mut self) {
        if addressable_return_entry(&self.output).is_none() {
            return;
        }

        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            crate::move_instruction_before_retargeting(self, 6, 5);
            return;
        }

        crate::move_instruction_before_retargeting(self, 1, 0);
        crate::move_instruction_before_retargeting(self, 2, 1);
        crate::move_instruction_before_retargeting(self, 4, 2);
        crate::move_instruction_before_retargeting(self, 5, 3);
        crate::move_instruction_before_retargeting(self, 5, 4);
        let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[2] else {
            unreachable!("the saved-LR store was matched")
        };
        *offset = 4;

        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: 12,
                } => *instruction = Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: 8,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 1,
                    offset: 12,
                } => *instruction = Instruction::LoadWord {
                    d: 3,
                    a: 1,
                    offset: 8,
                },
                _ => {}
            }
        }

        let len = self.output.instructions.len();
        if len >= 3
            && matches!(
                self.output.instructions.get(len - 3..),
                Some([
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 16,
                    },
                    Instruction::BranchToLinkRegister,
                ])
            )
        {
            crate::move_instruction_before_retargeting(self, len - 2, len - 3);
        }
    }
}

fn addressable_return_entry(output: &mwcc_machine_code::MachineFunction) -> Option<()> {
    if !matches!(
        output.instructions.get(0..8),
        Some([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate,
            },
            Instruction::BranchAndLink { .. },
        ]) if *immediate == 8 || *immediate == 12
    ) {
        return None;
    }

    let high = external_relocation_target(output, 2, RelocationKind::Addr16Ha)?;
    let low = external_relocation_target(output, 5, RelocationKind::Addr16Lo)?;
    (high == low).then_some(())
}

fn external_relocation_target(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(target) => Some(target.as_str()),
            _ => None,
        }
    })
}
