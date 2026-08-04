//! Linkage-first schedule for adjacent zero stores guarded by a narrow flag.
//!
//! The optimizer assigns the shared store zero to `r5`, fills the saved-link
//! latency with that constant, and lets the record-form narrow cast enter the
//! frame prefix before either store. The later three-argument call overwrites
//! `r5` only after both stores have consumed the zero.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_versions::FrameConvention;

const SCHEDULE: [usize; 16] = [0, 2, 1, 6, 3, 4, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15];

impl Generator {
    pub(crate) fn schedule_narrow_guarded_zero_member_reset(&mut self) -> bool {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.frame_size != 8
            || !self.callee_saved.is_empty()
        {
            return false;
        }
        schedule(&mut self.output)
    }
}

fn schedule(output: &mut MachineFunction) -> bool {
    if !candidate(&output.instructions) {
        return false;
    }
    crate::permute_machine_function_region(output, 0, &SCHEDULE);
    output.instructions[1] = Instruction::AddImmediate {
        d: 5,
        a: 0,
        immediate: 0,
    };
    for index in [5, 6] {
        let Instruction::StoreWord { s, .. } = &mut output.instructions[index] else {
            unreachable!("the guarded reset stores were recognized before scheduling")
        };
        *s = 5;
    }
    true
}

fn candidate(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
            Instruction::StoreWord { s: 0, a: 3, offset: 8 },
            Instruction::StoreWord { s: 0, a: 3, offset: 12 },
            Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 24 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    fn input() -> MachineFunction {
        let mut output = MachineFunction::new("reset");
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
            Instruction::StoreWord { s: 0, a: 3, offset: 8 },
            Instruction::StoreWord { s: 0, a: 3, offset: 12 },
            Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 24 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 16 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 2176 },
            Instruction::BranchAndLink { target: "clear".into() },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations.push(Relocation {
            instruction_index: 11,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("clear".into()),
        });
        output
    }

    #[test]
    fn schedules_the_zero_flag_and_linkage_prefix() {
        let mut output = input();
        assert!(schedule(&mut output));
        assert_eq!(output.relocations[0].instruction_index, 11);
        assert!(matches!(
            output.instructions[1],
            Instruction::AddImmediate { d: 5, immediate: 0, .. }
        ));
        assert!(matches!(
            output.instructions[3],
            Instruction::ClearLeftImmediateRecord { a: 0, s: 4, clear: 24 }
        ));
        assert!(matches!(
            output.instructions[5],
            Instruction::StoreWord { s: 5, offset: 8, .. }
        ));
        assert!(matches!(
            output.instructions[6],
            Instruction::StoreWord { s: 5, offset: 12, .. }
        ));
        assert!(matches!(
            output.instructions[7],
            Instruction::BranchConditionalForward { target: 12, .. }
        ));
    }
}
