use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GuardedFramePointerUpdate {
    pub(super) start: usize,
    pub(super) initial_pointer: u8,
    pub(super) old_cursor: u8,
    pub(super) scratch: u8,
}

pub(super) fn guarded_frame_pointer_update(
    instructions: &[Instruction],
) -> Option<GuardedFramePointerUpdate> {
    instructions.windows(22).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord {
                d: initial_pointer,
                a: 1,
                offset: frame_offset,
            },
            Instruction::LoadWord {
                d: position,
                a: guard_pointer,
                offset: cursor_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: guarded, .. },
            Instruction::BranchConditionalForward {
                target: success_entry,
                ..
            },
            Instruction::AddImmediate {
                d: error_status,
                a: 0,
                ..
            },
            Instruction::Branch {
                target: result_join,
            },
            Instruction::LoadWord {
                d: append_pointer,
                a: 1,
                offset: append_frame_offset,
            },
            Instruction::LoadWord {
                d: cursor_pointer,
                a: 1,
                offset: cursor_frame_offset,
            },
            Instruction::LoadWord {
                d: old_cursor,
                a: cursor_base,
                offset: reloaded_cursor_offset,
            },
            Instruction::AddImmediate {
                d: new_cursor,
                a: incremented_cursor,
                immediate: cursor_step,
            },
            Instruction::StoreWord {
                s: stored_cursor,
                a: cursor_store_base,
                offset: stored_cursor_offset,
            },
            Instruction::AddImmediate {
                d: scratch,
                a: copied_cursor,
                immediate: 0,
            },
            Instruction::Add {
                d: byte_address,
                a: append_base,
                b: cursor_index,
            },
            Instruction::StoreByte {
                s: byte,
                a: byte_base,
                ..
            },
            Instruction::LoadWord {
                d: length_store_pointer,
                a: 1,
                offset: length_store_frame_offset,
            },
            Instruction::LoadWord {
                d: length_load_pointer,
                a: 1,
                offset: length_load_frame_offset,
            },
            Instruction::LoadWord {
                d: old_length,
                a: length_base,
                offset: length_offset,
            },
            Instruction::AddImmediate {
                d: new_length,
                a: incremented_length,
                immediate: length_step,
            },
            Instruction::StoreWord {
                s: stored_length,
                a: length_store_base,
                offset: stored_length_offset,
            },
            Instruction::AddImmediate {
                d: success_status,
                a: 0,
                immediate: 0,
            },
            Instruction::CompareWordImmediate {
                a: joined_status,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                condition_bit: 2,
                ..
            },
        ] = window
        else {
            return None;
        };

        (*initial_pointer != 0
            && *initial_pointer != 1
            && *old_cursor != 0
            && *old_cursor != 1
            && *old_cursor != *initial_pointer
            && *old_cursor != *error_status
            && *old_cursor != *byte
            && *guard_pointer == *initial_pointer
            && *guarded == *position
            && *success_entry == start + 6
            && *result_join == start + 20
            && *append_frame_offset == *frame_offset
            && *cursor_frame_offset == *frame_offset
            && *cursor_base == *cursor_pointer
            && *reloaded_cursor_offset == *cursor_offset
            && *incremented_cursor == *old_cursor
            && *cursor_step == 1
            && *stored_cursor == *new_cursor
            && *cursor_store_base == *cursor_pointer
            && *stored_cursor_offset == *cursor_offset
            && *scratch == 0
            && *copied_cursor == *old_cursor
            && *append_base == *append_pointer
            && *cursor_index == *scratch
            && *byte_base == *byte_address
            && *length_store_frame_offset == *frame_offset
            && *length_load_frame_offset == *frame_offset
            && *length_base == *length_load_pointer
            && *incremented_length == *old_length
            && *length_step == 1
            && *stored_length == *new_length
            && *length_store_base == *length_store_pointer
            && *stored_length_offset == *length_offset
            && *success_status == *error_status
            && *joined_status == *success_status)
            .then_some(GuardedFramePointerUpdate {
                start,
                initial_pointer: *initial_pointer,
                old_cursor: *old_cursor,
                scratch: *scratch,
            })
    })
}

pub(super) fn direct_call_result_zero_test(instructions: &[Instruction]) -> Option<(usize, u8)> {
    instructions.windows(4).enumerate().find_map(|(call, window)| {
        let [
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate {
                d: saved,
                a: Eabi::FIRST_GENERAL_ARGUMENT,
                immediate: 0,
            },
            Instruction::CompareWordImmediate {
                a: compared,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                condition_bit: 2,
                ..
            },
        ] = window
        else {
            return None;
        };
        (*saved == *compared && (14..=31).contains(saved)).then_some((call + 1, *saved))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_call_free_guarded_frame_pointer_update() {
        let instructions = vec![
            Instruction::LoadWord { d: 3, a: 1, offset: 12 },
            Instruction::LoadWord { d: 0, a: 3, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2176 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 6,
            },
            Instruction::load_immediate(31, 769),
            Instruction::Branch { target: 20 },
            Instruction::LoadWord { d: 3, a: 1, offset: 12 },
            Instruction::LoadWord { d: 4, a: 1, offset: 12 },
            Instruction::LoadWord { d: 5, a: 4, offset: 12 },
            Instruction::AddImmediate { d: 6, a: 5, immediate: 1 },
            Instruction::StoreWord { s: 6, a: 4, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 5, immediate: 0 },
            Instruction::Add { d: 3, a: 3, b: 0 },
            Instruction::StoreByte { s: 30, a: 3, offset: 16 },
            Instruction::LoadWord { d: 3, a: 1, offset: 12 },
            Instruction::LoadWord { d: 4, a: 1, offset: 12 },
            Instruction::LoadWord { d: 4, a: 4, offset: 8 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 3, offset: 8 },
            Instruction::load_immediate(31, 0),
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 22,
            },
        ];

        assert_eq!(
            guarded_frame_pointer_update(&instructions),
            Some(GuardedFramePointerUpdate {
                start: 0,
                initial_pointer: 3,
                old_cursor: 5,
                scratch: 0,
            })
        );
    }

    #[test]
    fn recognizes_an_adjacent_saved_call_result_test() {
        let instructions = vec![
            Instruction::BranchAndLink { target: "send".into() },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 4,
            },
        ];

        assert_eq!(direct_call_result_zero_test(&instructions), Some((1, 31)));
    }
}
