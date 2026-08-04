//! Final value layout for a frame-free singly-linked unlink transaction.
//!
//! Source lowering initially reuses r4 for both the list-head address and the
//! cursor, forcing later reloads. Legacy MWCC retains the head in r5, walks in
//! r6, counts in r7, and uses r0 for each freshly loaded successor. Keeping
//! this complete physical transaction separate avoids leaking list-specific
//! caching policy into the general structured loop emitter.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_leaf_singly_linked_unlink(&mut self) {
        let Some((start, head_offset, next_offset)) =
            leaf_singly_linked_unlink(&self.output.instructions)
        else {
            return;
        };
        if self.output.relocations.iter().any(|relocation| {
            (start..start + 28).contains(&relocation.instruction_index)
        }) {
            return;
        }

        let replacement = [
            Instruction::LoadWord { d: 5, a: 3, offset: head_offset },
            Instruction::load_immediate(7, 0),
            Instruction::LoadWord { d: 0, a: 5, offset: 0 },
            Instruction::CompareLogicalWord { a: 0, b: 3 },
            Instruction::move_register(6, 0),
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: start + 12,
            },
            Instruction::LoadWord { d: 4, a: 3, offset: next_offset },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 4, a: 5, offset: 0 },
            Instruction::StoreWord { s: 0, a: 3, offset: head_offset },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
            Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 },
            Instruction::AddImmediate { d: 7, a: 7, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: start + 17,
            },
            Instruction::load_immediate(3, -1),
            Instruction::BranchToLinkRegister,
            Instruction::LoadWord { d: 0, a: 6, offset: next_offset },
            Instruction::CompareLogicalWord { a: 0, b: 3 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: start + 22,
            },
            Instruction::move_register(6, 0),
            Instruction::Branch { target: start + 12 },
            Instruction::LoadWord { d: 4, a: 3, offset: next_offset },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 4, a: 6, offset: next_offset },
            Instruction::StoreWord { s: 0, a: 3, offset: head_offset },
            Instruction::move_register(3, 7),
            Instruction::BranchToLinkRegister,
        ];
        self.output.instructions[start..start + replacement.len()]
            .clone_from_slice(&replacement);
    }
}

fn leaf_singly_linked_unlink(instructions: &[Instruction]) -> Option<(usize, i16, i16)> {
    instructions.windows(28).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord { d: 4, a: 3, offset: head_offset },
            Instruction::LoadWord { d: 4, a: 4, offset: 0 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
            Instruction::CompareLogicalWord { a: 4, b: 3 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: loop_entry },
            Instruction::LoadWord { d: 4, a: 3, offset: reloaded_head_offset },
            Instruction::LoadWord { d: 0, a: 3, offset: first_next_offset },
            Instruction::StoreWord { s: 0, a: 4, offset: 0 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 3, offset: first_clear_offset },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::Branch { target: first_return },
            Instruction::AddImmediate { d: 5, a: 5, immediate: 1 },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: load_next },
            Instruction::AddImmediate { d: 3, a: 0, immediate: -1 },
            Instruction::Branch { target: missing_return },
            Instruction::LoadWord { d: 0, a: 4, offset: loop_next_offset },
            Instruction::CompareLogicalWord { a: 0, b: 3 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: unlink },
            Instruction::LoadWord { d: 4, a: 4, offset: carried_next_offset },
            Instruction::Branch { target: back_edge },
            Instruction::LoadWord { d: 0, a: 3, offset: final_next_offset },
            Instruction::StoreWord { s: 0, a: 4, offset: stored_next_offset },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 3, offset: final_clear_offset },
            Instruction::Or { a: 3, s: 5, b: 5 },
            Instruction::BranchToLinkRegister,
        ] = window
        else {
            return None;
        };
        (*head_offset == *reloaded_head_offset
            && *head_offset == *first_clear_offset
            && *head_offset == *final_clear_offset
            && *first_next_offset == *loop_next_offset
            && *first_next_offset == *carried_next_offset
            && *first_next_offset == *final_next_offset
            && *first_next_offset == *stored_next_offset
            && *loop_entry == start + 12
            && *first_return == start + 27
            && *load_next == start + 17
            && *missing_return == start + 27
            && *unlink == start + 22
            && *back_edge == start + 12)
            .then_some((start, *head_offset, *first_next_offset))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_incomplete_unlink_stream() {
        assert_eq!(
            leaf_singly_linked_unlink(&[
                Instruction::LoadWord { d: 4, a: 3, offset: 8 },
                Instruction::BranchToLinkRegister,
            ]),
            None
        );
    }
}
