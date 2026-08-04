//! Final scheduling for a frame-free singly-linked tail append.
//!
//! The structured CFG emitter keeps void returns as branches to one terminal
//! `blr` and emits the new-node owner store in source order. Legacy MWCC moves
//! that independent store below the initial null comparison and spells both
//! successful exits as inline returns, retaining the otherwise unreachable
//! terminal `blr` as part of the function image.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_leaf_tail_append(&mut self) {
        let Some(start) = leaf_tail_append(&self.output.instructions) else {
            return;
        };

        self.output.instructions.swap(start + 1, start + 2);
        swap_relocation_indices(&mut self.output.relocations, start + 1, start + 2);
        self.output.instructions[start + 7] = Instruction::BranchToLinkRegister;
        self.output.instructions[start + 14] = Instruction::BranchToLinkRegister;
    }
}

fn leaf_tail_append(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(18).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord {
                d: cursor,
                a: head,
                offset: 0,
            },
            Instruction::StoreWord {
                s: stored_head,
                a: item,
                ..
            },
            Instruction::CompareLogicalWordImmediate {
                a: compared_cursor,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: loop_entry,
            },
            Instruction::StoreWord {
                s: stored_item,
                a: stored_head_pointer,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: first_zero,
                a: first_item,
                offset: first_next_offset,
            },
            Instruction::Branch { target: first_return },
            Instruction::LoadWord {
                d: next,
                a: loop_cursor,
                offset: loop_next_offset,
            },
            Instruction::CompareLogicalWordImmediate {
                a: compared_next,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: carry,
            },
            Instruction::StoreWord {
                s: appended_item,
                a: append_cursor,
                offset: append_next_offset,
            },
            Instruction::AddImmediate {
                d: second_zero,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: stored_zero,
                a: second_item,
                offset: second_next_offset,
            },
            Instruction::Branch { target: second_return },
            Instruction::Or {
                a: carried_cursor,
                s: carried_next,
                b: carried_next_again,
            },
            Instruction::Branch { target: back_edge },
            Instruction::BranchToLinkRegister,
        ] = window
        else {
            return None;
        };

        (*head == 3
            && *item == 4
            && *cursor == 5
            && stored_head == head
            && compared_cursor == cursor
            && stored_item == item
            && stored_head_pointer == head
            && *zero == 0
            && first_zero == zero
            && first_item == item
            && *next == 0
            && loop_cursor == cursor
            && compared_next == next
            && appended_item == item
            && append_cursor == cursor
            && append_next_offset == loop_next_offset
            && *second_zero == 0
            && stored_zero == second_zero
            && second_item == item
            && second_next_offset == first_next_offset
            && carried_cursor == cursor
            && carried_next == next
            && carried_next_again == next
            && *loop_entry == start + 8
            && *carry == start + 15
            && *back_edge == start + 8
            && *first_return == start + 17
            && *second_return == start + 17)
            .then_some(start)
    })
}

fn swap_relocation_indices(
    relocations: &mut [mwcc_machine_code::Relocation],
    left: usize,
    right: usize,
) {
    for relocation in relocations {
        if relocation.instruction_index == left {
            relocation.instruction_index = right;
        } else if relocation.instruction_index == right {
            relocation.instruction_index = left;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_leaf_tail_append_transaction() {
        let instructions = vec![
            Instruction::LoadWord { d: 5, a: 3, offset: 0 },
            Instruction::StoreWord { s: 3, a: 4, offset: 8 },
            Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 8,
            },
            Instruction::StoreWord { s: 4, a: 3, offset: 0 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 4, offset: 36 },
            Instruction::Branch { target: 17 },
            Instruction::LoadWord { d: 0, a: 5, offset: 36 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 15,
            },
            Instruction::StoreWord { s: 4, a: 5, offset: 36 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 4, offset: 36 },
            Instruction::Branch { target: 17 },
            Instruction::move_register(5, 0),
            Instruction::Branch { target: 8 },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(leaf_tail_append(&instructions), Some(0));
    }
}
