//! Final scheduling for a frame-vector accumulation with a nullable source.
//!
//! The structured owner establishes the semantic homes and frame slots. Build
//! 163 then fills the first call's setup window, forms the nullable frame
//! address in r4 while loading an indexed source vector, and spells the final
//! saved-base call argument as a materialization copy.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_frame_vector_accumulation(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(entry) = self
            .output
            .instructions
            .windows(7)
            .position(is_unscheduled_entry)
        else {
            return;
        };
        let Some(select) = self
            .output
            .instructions
            .windows(11)
            .position(is_unscheduled_nullable_vector_select)
        else {
            return;
        };
        let Some(tail) = self
            .output
            .instructions
            .windows(3)
            .rposition(is_unscheduled_tail_call)
        else {
            return;
        };

        self.move_frame_vector_instruction_before(entry + 5, entry);
        self.move_frame_vector_instruction_before(entry + 5, entry + 3);
        self.move_frame_vector_instruction_before(select + 9, select + 1);
        self.move_frame_vector_instruction_before(select + 3, select + 2);

        let source = match self.output.instructions[tail] {
            Instruction::Or { s, .. } => s,
            _ => unreachable!(),
        };
        self.output.instructions[tail] = Instruction::AddImmediate {
            d: 3,
            a: source,
            immediate: 0,
        };
    }

    fn move_frame_vector_instruction_before(&mut self, from: usize, to: usize) {
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == from {
                to
            } else if (to..from).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => {
                    *target = if *target == from {
                        to
                    } else if (to..from).contains(&*target) {
                        *target + 1
                    } else {
                        *target
                    };
                }
                _ => {}
            }
        }
    }
}

fn is_unscheduled_entry(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: saved, a: 3, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: first_base, .. },
        Instruction::StoreFloatSingle { s: 0, a: second_base, .. },
        Instruction::Or { a: 3, s: argument, b: duplicate },
        Instruction::AddImmediate { d: 4, a: 1, .. },
        Instruction::BranchAndLink { .. },
    ] if saved == first_base
        && first_base == second_base
        && second_base == argument
        && argument == duplicate
        && (14..=31).contains(saved))
}

fn is_unscheduled_nullable_vector_select(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: 3, a: 0, .. },
        Instruction::LoadWord { d: 3, a: 3, offset: 0 },
        Instruction::LoadByteZero { d: 0, a: saved, .. },
        Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 },
        Instruction::Add { d: 3, a: 3, b: 0 },
        Instruction::LoadFloatSingle { d: 0, a: 3, offset: 0 },
        Instruction::StoreFloatSingle { s: 0, a: 1, offset: first_offset },
        Instruction::LoadFloatSingle { d: 0, a: 3, offset: 4 },
        Instruction::StoreFloatSingle { s: 0, a: 1, offset: second_offset },
        Instruction::AddImmediate { d: 4, a: 1, immediate: address_offset },
        Instruction::Branch { .. },
    ] if (14..=31).contains(saved)
        && first_offset.checked_add(4) == Some(*second_offset)
        && first_offset == address_offset)
}

fn is_unscheduled_tail_call(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::Or { a: 3, s: saved, b: duplicate },
        Instruction::AddImmediate { d: 4, a: 1, .. },
        Instruction::BranchAndLink { .. },
    ] if saved == duplicate && (14..=31).contains(saved))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_nullable_frame_vector_select() {
        let instructions = [
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadWord { d: 3, a: 3, offset: 0 },
            Instruction::LoadByteZero { d: 0, a: 31, offset: 12 },
            Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 3 },
            Instruction::Add { d: 3, a: 3, b: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 16 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 4 },
            Instruction::StoreFloatSingle { s: 0, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 4, a: 1, immediate: 16 },
            Instruction::Branch { target: 12 },
        ];
        assert!(is_unscheduled_nullable_vector_select(&instructions));
    }
}
