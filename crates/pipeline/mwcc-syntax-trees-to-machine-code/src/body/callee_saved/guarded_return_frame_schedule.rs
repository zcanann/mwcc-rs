//! Physical entry schedule for a guarded return through an address-taken local.
//!
//! The frame owner deliberately emits a dependency-safe order. MWCC fills the
//! absolute-address and linkage latency slots differently in the two frame
//! convention families, after register allocation has fixed the ABI lanes.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    pub(crate) fn schedule_guarded_return_address_frame(&mut self) {
        if self.legacy_callee_saved_frame_layout
            != LegacyCalleeSavedFrameLayout::RetainGuardedEntryParameterTable
            || guarded_return_address_entry(&self.output).is_none()
        {
            return;
        }

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[5]
        else {
            unreachable!("the absolute-address high instruction was matched")
        };
        *d = 4;
        let Instruction::AddImmediate { d, a, .. } = &mut self.output.instructions[6] else {
            unreachable!("the absolute-address low instruction was matched")
        };
        *d = 3;
        *a = 4;

        // Fill the frame-setup latency slots with the independent global
        // address while leaving the address-taken local immediately at the call.
        crate::move_instruction_before_retargeting(self, 5, 2);
        crate::move_instruction_before_retargeting(self, 6, 5);

        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        // Build 163 records LR before allocating this compact frame. Express the
        // permutation as earlier-only moves so all instruction owners are
        // retargeted by the shared scheduling primitive.
        crate::move_instruction_before_retargeting(self, 1, 0);
        crate::move_instruction_before_retargeting(self, 2, 1);
        crate::move_instruction_before_retargeting(self, 4, 2);
        crate::move_instruction_before_retargeting(self, 4, 3);
        crate::move_instruction_before_retargeting(self, 5, 4);
        let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[2] else {
            unreachable!("the saved-LR store was matched")
        };
        *offset = 4;
        self.output.instructions[3] = Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        };

        let len = self.output.instructions.len();
        if len >= 4
            && matches!(
                self.output.instructions.get(len - 4..),
                Some([
                    Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset,
                    },
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate,
                    },
                    Instruction::BranchToLinkRegister,
                ]) if *offset == self.frame_size + 4 && *immediate == self.frame_size
            )
        {
            crate::move_instruction_before_retargeting(self, len - 2, len - 3);
        }
    }
}

fn guarded_return_address_entry(output: &mwcc_machine_code::MachineFunction) -> Option<()> {
    let instructions = output.instructions.get(0..8)?;
    let frame_offset = match instructions {
        [
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            parameter_move,
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: frame_offset,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchAndLink { .. },
        ] if matches!(parameter_move,
            Instruction::Or { a: 5, s: 3, b: 3 }
            | Instruction::AddImmediate { d: 5, a: 3, immediate: 0 }) => *frame_offset,
        _ => return None,
    };
    if !matches!(frame_offset, 8 | 12) {
        return None;
    }

    let high = external_relocation_target(output, 5, RelocationKind::Addr16Ha)?;
    let low = external_relocation_target(output, 6, RelocationKind::Addr16Lo)?;
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
