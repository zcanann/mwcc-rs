//! Final scheduling for repeated inlined one-byte buffer appends.
//!
//! Inline expansion initially preserves the helper's value-oriented diamond:
//! a true-edge branch plus an explicit join, followed by independent reloads
//! of the cursor. When two byte appends are used only for side effects, MWCC
//! inverts each capacity guard and keeps the first cursor load live throughout
//! the append. This owner requires the complete repeated machine skeleton
//! before removing either branch or reload.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InlineByteAppend {
    start: usize,
    cursor: u8,
    cursor_scratch: u8,
    end: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_inlined_byte_appends(&mut self) {
        if inline_byte_appends(&self.output.instructions).count() < 2 {
            return;
        }
        let mut scheduled = false;
        loop {
            let Some(plan) = inline_byte_append(&self.output.instructions) else {
                break;
            };
            scheduled = true;
            let Instruction::LoadWord { d, .. } =
                &mut self.output.instructions[plan.start]
            else {
                unreachable!("the append cursor load was matched")
            };
            *d = plan.cursor;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[plan.start + 1]
            else {
                unreachable!("the append capacity test was matched")
            };
            *a = plan.cursor;
            let Instruction::BranchConditionalForward {
                options, target, ..
            } = &mut self.output.instructions[plan.start + 2]
            else {
                unreachable!("the append success edge was matched")
            };
            *options ^= 8;
            *target = plan.end;

            crate::remove_instruction_retargeting_to_next(self, plan.start + 3);
            crate::remove_instruction_retargeting_to_next(self, plan.start + 3);

            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[plan.start + 3]
            else {
                unreachable!("the append cursor increment was matched")
            };
            *d = plan.cursor_scratch;
            let Instruction::StoreWord { s, .. } =
                &mut self.output.instructions[plan.start + 4]
            else {
                unreachable!("the append cursor publication was matched")
            };
            *s = plan.cursor_scratch;
            crate::remove_instruction_retargeting_to_next(self, plan.start + 5);
            let Instruction::Add { b, .. } =
                &mut self.output.instructions[plan.start + 5]
            else {
                unreachable!("the append byte address was matched")
            };
            *b = plan.cursor;
            crate::move_instruction_before_retargeting(self, plan.start + 5, plan.start + 4);
        }
        if scheduled {
            schedule_reset_argument_linkage_slot(self);
        }
    }
}

fn inline_byte_append(instructions: &[Instruction]) -> Option<InlineByteAppend> {
    inline_byte_appends(instructions).next()
}

fn inline_byte_appends(
    instructions: &[Instruction],
) -> impl Iterator<Item = InlineByteAppend> + '_ {
    instructions.windows(13).enumerate().filter_map(|(start, window)| {
        let [
            Instruction::LoadWord {
                d: guarded_cursor,
                a: guard_buffer,
                offset: cursor_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: guarded, .. },
            Instruction::BranchConditionalForward {
                options,
                condition_bit: 0,
                target: success,
            },
            Instruction::Branch { target: end },
            Instruction::LoadWord {
                d: cursor,
                a: cursor_buffer,
                offset: reloaded_cursor_offset,
            },
            Instruction::AddImmediate {
                d: incremented_cursor,
                a: incremented_from,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: stored_cursor,
                a: cursor_store_buffer,
                offset: stored_cursor_offset,
            },
            Instruction::AddImmediate {
                d: cursor_scratch,
                a: copied_cursor,
                immediate: 0,
            },
            Instruction::Add {
                d: byte_address,
                a: append_buffer,
                b: cursor_index,
            },
            Instruction::StoreByte { a: byte_base, .. },
            Instruction::LoadWord {
                d: old_length,
                a: length_buffer,
                offset: length_offset,
            },
            Instruction::AddImmediate {
                d: new_length,
                a: incremented_length,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: stored_length,
                a: length_store_buffer,
                offset: stored_length_offset,
            },
        ] = window
        else {
            return None;
        };
        (*options == 12
            && *success == start + 4
            && *end == start + 13
            && *guarded == *guarded_cursor
            && *guard_buffer == *cursor_buffer
            && *guard_buffer == *cursor_store_buffer
            && *guard_buffer == *append_buffer
            && *guard_buffer == *length_buffer
            && *guard_buffer == *length_store_buffer
            && *cursor_offset == *reloaded_cursor_offset
            && *cursor_offset == *stored_cursor_offset
            && *incremented_from == *cursor
            && *stored_cursor == *incremented_cursor
            && *copied_cursor == *cursor
            && *cursor_index == *cursor_scratch
            && *byte_address == *byte_base
            && *incremented_length == *old_length
            && *stored_length == *new_length
            && *length_offset == *stored_length_offset
            && *cursor != 0
            && *cursor != 1
            && *cursor_scratch == 0)
            .then_some(InlineByteAppend {
                start,
                cursor: *cursor,
                cursor_scratch: *cursor_scratch,
                end: *end,
            })
    })
}

fn schedule_reset_argument_linkage_slot(generator: &mut Generator) {
    let instructions = &generator.output.instructions;
    if !matches!(instructions.get(..11), Some([
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
        Instruction::StoreWord { s: first, a: 1, .. },
        Instruction::AddImmediate { d: first_copy, a: 5, immediate: 0 },
        Instruction::StoreWord { s: second, a: 1, .. },
        Instruction::AddImmediate { d: second_copy, a: 4, immediate: 0 },
        Instruction::StoreWord { s: third, a: 1, .. },
        Instruction::AddImmediate { d: third_copy, a: 3, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
        Instruction::BranchAndLink { .. },
    ]) if first == first_copy
        && second == second_copy
        && third == third_copy
        && first != second
        && second != third
        && first != third)
    {
        return;
    }
    crate::move_instruction_before_retargeting(generator, 9, 7);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_complete_value_oriented_append_diamond() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 29, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2176 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 4,
            },
            Instruction::Branch { target: 13 },
            Instruction::LoadWord { d: 3, a: 29, offset: 12 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 4, a: 29, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::Add { d: 3, a: 29, b: 0 },
            Instruction::StoreByte { s: 30, a: 3, offset: 16 },
            Instruction::LoadWord { d: 3, a: 29, offset: 8 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 29, offset: 8 },
        ];

        assert_eq!(
            inline_byte_append(&instructions),
            Some(InlineByteAppend {
                start: 0,
                cursor: 3,
                cursor_scratch: 0,
                end: 13,
            })
        );
    }
}
