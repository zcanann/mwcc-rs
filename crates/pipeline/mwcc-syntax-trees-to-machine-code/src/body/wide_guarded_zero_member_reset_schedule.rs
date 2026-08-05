//! Predecrement schedule for adjacent zero stores guarded by a full-width flag.
//!
//! The optimizer hoists the independent signed-zero compare into the saved-link
//! latency slot. Unlike the narrow-flag form, the zero value remains in `r0`
//! and needs neither a record-form mask nor a different store register.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Function, Type};
use mwcc_versions::FrameConvention;

const SCHEDULE: [usize; 16] = [0, 1, 6, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15];

impl Generator {
    pub(crate) fn schedule_wide_guarded_zero_member_reset(
        &mut self,
        function: &Function,
    ) -> bool {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.frame_size != 16
            || !self.callee_saved.is_empty()
            || function
                .parameters
                .get(1)
                .is_none_or(|parameter| parameter.parameter_type != Type::Int)
            || !candidate(&self.output.instructions)
        {
            return false;
        }

        crate::permute_machine_function_region(&mut self.output, 0, &SCHEDULE);
        true
    }
}

fn candidate(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 3, offset: 8 },
            Instruction::StoreWord { s: 0, a: 3, offset: 12 },
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::BranchToLinkRegister,
        ]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation, RelocationKind, RelocationTarget};

    #[test]
    fn hoists_the_full_width_guard_into_the_linkage_prefix() {
        let mut output = MachineFunction::new("reset");
        output.instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 3, offset: 8 },
            Instruction::StoreWord { s: 0, a: 3, offset: 12 },
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 16 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 2176 },
            Instruction::BranchAndLink { target: "clear".into() },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations.push(Relocation {
            instruction_index: 11,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("clear".into()),
        });

        assert!(candidate(&output.instructions));
        crate::permute_machine_function_region(&mut output, 0, &SCHEDULE);

        assert!(matches!(
            output.instructions[2],
            Instruction::CompareWordImmediate { a: 4, immediate: 0 }
        ));
        assert!(matches!(
            output.instructions[3],
            Instruction::StoreWord { s: 0, a: 1, offset: 20 }
        ));
        assert!(matches!(
            output.instructions[4],
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 }
        ));
        assert_eq!(output.relocations[0].instruction_index, 11);
    }
}
