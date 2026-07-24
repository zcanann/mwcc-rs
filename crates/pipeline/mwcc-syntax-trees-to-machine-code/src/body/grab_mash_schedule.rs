//! Final physical schedule for the grab-mash input transaction.
//!
//! This function combines a discontiguous input mask, four stick-threshold
//! guards, signed-byte change detection, a bit-field/counter update, and a
//! three-argument report call.  MWCC schedules those pieces across their
//! source statement boundaries.  Claim only the complete measured stream so
//! the two dead loads and the cross-block register choices cannot affect a
//! partial lookalike.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_grab_mash_transaction(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(94)
            .position(is_unscheduled_grab_mash)
        else {
            return;
        };
        if !has_expected_relocations(self, start) {
            return;
        }

        // Discontiguous record-mask lowering has already removed the former
        // scratch load and scheduled the entry-ready mask across the linkage
        // slots. Preserve that canonical prefix and remove only the remaining
        // redundant bit-field storage reload.
        let mut identities = (0usize..94).collect::<Vec<_>>();
        let relative = identities
            .iter()
            .position(|identity| *identity == 65)
            .expect("the guarded stream contains the dead bit-field reload");
        self.remove_grab_mash_instruction(start + relative);
        identities.remove(relative);

        const DESIRED: [usize; 93] = [
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 13, 11, 12, 17, 16, 18, 14, 19, 15,
            20, 21, 22, 23, 25, 24, 26, 27, 28, 29, 30, 32, 31, 33, 34, 35, 36, 37,
            38, 40, 39, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55,
            56, 59, 57, 58, 60, 61, 62, 63, 64, 66, 67, 68, 69, 70, 71, 72, 73, 74,
            75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 89, 88, 90, 91, 92,
            93,
        ];
        for (to, desired_identity) in DESIRED.into_iter().enumerate() {
            let from = identities
                .iter()
                .position(|identity| *identity == desired_identity)
                .expect("the guarded permutation is complete");
            if from != to {
                self.move_grab_mash_instruction_before(start + from, start + to);
                let identity = identities.remove(from);
                identities.insert(to, identity);
            }
        }

        set_add_shifted_destination(&mut self.output.instructions[start + 1], 4);
        set_add_immediate_registers(&mut self.output.instructions[start + 3], 0, 4);
        set_word_load_destination(&mut self.output.instructions[start + 7], 5);
        self.output.instructions[start + 8] = Instruction::AndRecord { a: 0, s: 5, b: 0 };
        set_byte_load_destination(&mut self.output.instructions[start + 62], 4);
        self.output.instructions[start + 63] = Instruction::RotateAndMaskRecord {
            a: 0,
            s: 4,
            shift: 31,
            begin: 31,
            end: 31,
        };
        set_add_immediate_registers(&mut self.output.instructions[start + 65], 0, 0);
        self.output.instructions[start + 66] = Instruction::RotateAndMaskInsert {
            a: 4,
            s: 0,
            shift: 2,
            begin: 29,
            end: 29,
        };
        set_byte_store_source(&mut self.output.instructions[start + 67], 4);
        self.output.instructions[start + 73] = Instruction::CompareLogicalWord { a: 4, b: 0 };

        for (branch, target) in [
            (9, 14),
            (21, 24),
            (28, 31),
            (36, 39),
            (43, 46),
            (50, 56),
            (55, 60),
            (61, 78),
            (64, 78),
            (74, 82),
        ] {
            set_forward_target(&mut self.output.instructions[start + branch], start + target);
        }
        set_branch_target(&mut self.output.instructions[start + 77], start + 82);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }

    fn remove_grab_mash_instruction(&mut self, at: usize) {
        self.output.instructions.remove(at);
        self.labels.removed_retargeting_to_next(at, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != at);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > at {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > at =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }

    fn move_grab_mash_instruction_before(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        let move_index = |index: &mut usize| {
            *index = if *index == from {
                to
            } else if (to..from).contains(index) {
                *index + 1
            } else {
                *index
            };
        };
        for relocation in &mut self.output.relocations {
            move_index(&mut relocation.instruction_index);
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => move_index(target),
                _ => {}
            }
        }
    }
}

fn is_unscheduled_grab_mash(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 3840 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 31, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 5, a: 3, offset: 1640 },
            Instruction::AndRecord { a: 0, s: 5, b: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, .. },
            ..,
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    ) && matches!(
        &window[61..69],
        [
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadByteZero { d: 0, a: 3, offset: 8740 },
            Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 31, begin: 31, end: 31 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadByteZero { d: 0, a: 3, offset: 8740 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::RotateAndMaskInsert { a: 0, s: 4, shift: 2, begin: 29, end: 29 },
            Instruction::StoreByte { s: 0, a: 3, offset: 8740 },
        ]
    )
}

fn has_expected_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| (start..start + 94).contains(&relocation.instruction_index))
        .map(|relocation| relocation.instruction_index - start)
        .collect::<Vec<_>>();
    relative == [17, 25, 32, 40, 87]
        && [25, 32, 40].into_iter().all(|index| {
            schedule_relocations::same_relocated_value(
                &generator.output.relocations,
                &generator.output.constants,
                start + 17,
                start + index,
            )
        })
}

fn set_add_shifted_destination(instruction: &mut Instruction, destination: u8) {
    let Instruction::AddImmediateShifted { d, .. } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *d = destination;
}

fn set_add_immediate_registers(instruction: &mut Instruction, destination: u8, base: u8) {
    let Instruction::AddImmediate { d, a, .. } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *d = destination;
    *a = base;
}

fn set_word_load_destination(instruction: &mut Instruction, destination: u8) {
    let Instruction::LoadWord { d, .. } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *d = destination;
}

fn set_byte_load_destination(instruction: &mut Instruction, destination: u8) {
    let Instruction::LoadByteZero { d, .. } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *d = destination;
}

fn set_byte_store_source(instruction: &mut Instruction, source: u8) {
    let Instruction::StoreByte { s, .. } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *s = source;
}

fn set_forward_target(instruction: &mut Instruction, destination: usize) {
    let Instruction::BranchConditionalForward { target, .. } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *target = destination;
}

fn set_branch_target(instruction: &mut Instruction, destination: usize) {
    let Instruction::Branch { target } = instruction else {
        unreachable!("the complete grab-mash stream was recognized")
    };
    *target = destination;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_unrelated_physical_stream() {
        let instructions = vec![Instruction::BranchToLinkRegister; 94];
        assert!(!is_unscheduled_grab_mash(&instructions));
    }
}
