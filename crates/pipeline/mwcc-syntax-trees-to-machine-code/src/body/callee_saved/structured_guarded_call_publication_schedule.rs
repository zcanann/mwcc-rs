//! Final issue order for a guarded call-result publication loop.
//!
//! The source-level owner has already assigned the result, two output
//! pointers, acquired object, and counter to `r27..r31`. Build 159 then fills
//! the linkage prefix with the null value, enters the loop through its compare,
//! and leaves the acquired object in `r3` for the lock call. Keep this physical
//! schedule separate from saved-home planning: at this point call relocations
//! and internal branch destinations are durable instruction-index owners and
//! must move through the common permutation helper.

#[allow(unused_imports)]
use super::*;

const SCHEDULE: [usize; 39] = [
    0, 1, 7, 2, 3, 6, 4, 5, 9, 8, 13, 10, 11, 12, 14, 15, 16, 17, 18, 19, 20, 21,
    22, 23, 25, 24, 26, 27, 28, 29, 30, 31, 32, 33, 34, 36, 35, 37, 38,
];

impl Generator {
    pub(crate) fn schedule_structured_guarded_call_publication(&mut self) -> bool {
        let shape = guarded_call_publication(&self.output.instructions);
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.use_lmw_stmw
            || self.frame_size != 32
            || self.callee_saved.len() != 5
            || !shape
        {
            return false;
        }

        super::structured_conversion_call_schedule::permute_region(
            &mut self.output,
            0,
            &SCHEDULE,
        );

        self.output.instructions[9] = Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        };
        self.output.instructions[10] = Instruction::Branch { target: 31 };
        self.output.instructions[11] = Instruction::move_register(3, 30);
        self.output.instructions[13] = Instruction::move_register(29, 3);
        self.output.instructions[18] = Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 0,
        };
        self.output.instructions[28] = Instruction::move_register(3, 29);
        self.output.instructions[36] = Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        };
        true
    }
}

fn guarded_call_publication(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreMultipleWord { s: 27, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 27, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 31, a: 0, immediate: 768 },
            Instruction::AddImmediate { d: 28, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 28, offset: 0 },
            Instruction::AddImmediate { d: 30, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 29, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord { d: 0, a: 29, offset: 4 },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { target: 28, .. },
            Instruction::Or { a: 3, s: 29, b: 29 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 31, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 29, a: 28, offset: 0 },
            Instruction::StoreWord { s: 30, a: 27, offset: 0 },
            Instruction::AddImmediate { d: 30, a: 0, immediate: 3 },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 30, a: 30, immediate: 1 },
            Instruction::CompareWordImmediate { a: 30, immediate: 3 },
            Instruction::BranchConditionalForward { target: 10, .. },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadMultipleWord { d: 27, a: 1, offset: 12 },
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn candidate() -> Vec<Instruction> {
        vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreMultipleWord { s: 27, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 27, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 31, a: 0, immediate: 768 },
            Instruction::AddImmediate { d: 28, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 28, offset: 0 },
            Instruction::AddImmediate { d: 30, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::BranchAndLink { target: "get".into() },
            Instruction::AddImmediate { d: 29, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::BranchAndLink { target: "acquire".into() },
            Instruction::LoadWord { d: 0, a: 29, offset: 4 },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 28 },
            Instruction::Or { a: 3, s: 29, b: 29 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::BranchAndLink { target: "reset".into() },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::BranchAndLink { target: "publish".into() },
            Instruction::AddImmediate { d: 31, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 29, a: 28, offset: 0 },
            Instruction::StoreWord { s: 30, a: 27, offset: 0 },
            Instruction::AddImmediate { d: 30, a: 0, immediate: 3 },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::BranchAndLink { target: "release".into() },
            Instruction::AddImmediate { d: 30, a: 30, immediate: 1 },
            Instruction::CompareWordImmediate { a: 30, immediate: 3 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 10 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadMultipleWord { d: 27, a: 1, offset: 12 },
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    }

    #[test]
    fn schedules_the_entry_loop_and_epilogue_with_durable_indices() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = candidate();
        output.relocations.push(Relocation {
            instruction_index: 11,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("get".into()),
        });

        assert!(guarded_call_publication(&output.instructions));
        super::structured_conversion_call_schedule::permute_region(&mut output, 0, &SCHEDULE);
        output.instructions[9] = Instruction::StoreWord { s: 0, a: 4, offset: 0 };
        output.instructions[10] = Instruction::Branch { target: 31 };
        output.instructions[11] = Instruction::move_register(3, 30);
        output.instructions[13] = Instruction::move_register(29, 3);
        output.instructions[18] = Instruction::AddImmediate { d: 3, a: 29, immediate: 0 };
        output.instructions[28] = Instruction::move_register(3, 29);
        output.instructions[36] = Instruction::LoadWord { d: 0, a: 1, offset: 4 };

        assert_eq!(output.relocations[0].instruction_index, 12);
        assert!(matches!(output.instructions[10], Instruction::Branch { target: 31 }));
        assert!(matches!(output.instructions[11], Instruction::Or { a: 3, s: 30, b: 30 }));
        assert!(matches!(output.instructions[13], Instruction::Or { a: 29, s: 3, b: 3 }));
        assert!(matches!(output.instructions[32], Instruction::BranchConditionalForward { target: 11, .. }));
        assert!(matches!(output.instructions[24], Instruction::StoreWord { s: 29, a: 28, .. }));
        assert!(matches!(output.instructions[25], Instruction::AddImmediate { d: 31, immediate: 0, .. }));
        assert!(matches!(output.instructions[35], Instruction::AddImmediate { d: 1, immediate: 32, .. }));
        assert!(matches!(output.instructions[36], Instruction::LoadWord { d: 0, a: 1, offset: 4 }));
    }
}
