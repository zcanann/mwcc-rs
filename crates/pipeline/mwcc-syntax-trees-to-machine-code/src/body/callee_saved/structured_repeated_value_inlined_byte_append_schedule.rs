//! Compact final schedule for repeated value-returning inlined byte appends.
//!
//! A single value append has a distinct register plan. When the same owner is
//! appended several times, MWCC keeps only that owner callee-saved, loads each
//! source byte at its use, and retains the cursor across the successful edge.
//! The source-tree count selects this family; the complete physical skeleton
//! makes the rewrite transactional.

use super::*;

#[derive(Clone, Copy)]
struct AppendWindow {
    start: usize,
    value_offset: i16,
    cursor_offset: i16,
    data_offset: i16,
    length_offset: i16,
    capacity: u16,
    overflow: i16,
}

impl Generator {
    pub(crate) fn schedule_structured_repeated_value_inlined_byte_appends(&mut self) {
        if repeated_value_appends(&self.output.instructions, 30)
            .take(2)
            .count()
            < 2
        {
            return;
        }
        let original = self.clone();
        if !self.try_schedule_structured_repeated_value_inlined_byte_appends() {
            *self = original;
        }
    }

    fn try_schedule_structured_repeated_value_inlined_byte_appends(&mut self) -> bool {
        let Some((frame, epilogue)) = repeated_append_frame(&self.output.instructions) else {
            return false;
        };
        let owner = 30;
        let owner_home = 31;
        for instruction in &mut self.output.instructions[frame + 4..epilogue] {
            mwcc_vreg::for_each_register(instruction, |_, class, register| {
                if class == mwcc_vreg::Class::General && *register == owner {
                    *register = owner_home;
                }
            });
        }

        let mut scheduled = 0;
        while let Some(window) = repeated_value_append(&self.output.instructions, owner_home) {
            schedule_append(self, window, owner_home);
            scheduled += 1;
        }
        if scheduled < 2 {
            return false;
        }

        let Some((frame, epilogue)) = repeated_append_frame(&self.output.instructions) else {
            return false;
        };
        self.output.instructions[frame] = Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -16,
        };
        self.output.instructions[frame + 1] = Instruction::StoreWord {
            s: owner_home,
            a: 1,
            offset: 12,
        };
        crate::remove_instruction_retargeting_to_next(self, frame + 2);
        self.output.instructions[frame + 2] = Instruction::Or {
            a: owner_home,
            s: 3,
            b: 3,
        };

        let epilogue = epilogue - 1;
        self.output.instructions[epilogue] = Instruction::LoadWord {
            d: owner_home,
            a: 1,
            offset: 12,
        };
        crate::remove_instruction_retargeting_to_next(self, epilogue + 1);
        self.output.instructions[epilogue + 1] = Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        };

        self.frame_size = 16;
        self.callee_saved.retain(|register| *register != owner);
        for location in self.locations.values_mut() {
            if location.class == ValueClass::General && location.register == owner {
                location.register = owner_home;
            }
        }
        true
    }
}

fn repeated_append_frame(instructions: &[Instruction]) -> Option<(usize, usize)> {
    let frame = instructions.windows(4).position(|window| {
        matches!(window[0], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 })
            && matches!(window[1], Instruction::StoreWord { s: 31, a: 1, offset: 28 })
            && matches!(window[2], Instruction::StoreWord { s: 30, a: 1, offset: 24 })
            && matches!(window[3], Instruction::Or { a: 30, s: 3, b: 3 })
    })?;
    let epilogue = instructions.windows(3).rposition(|window| {
        matches!(window[0], Instruction::LoadWord { d: 31, a: 1, offset: 28 })
            && matches!(window[1], Instruction::LoadWord { d: 30, a: 1, offset: 24 })
            && matches!(window[2], Instruction::AddImmediate { d: 1, a: 1, immediate: 32 })
    })?;
    (frame < epilogue).then_some((frame, epilogue))
}

fn repeated_value_append(instructions: &[Instruction], owner: u8) -> Option<AppendWindow> {
    repeated_value_appends(instructions, owner).next()
}

fn repeated_value_appends(
    instructions: &[Instruction],
    owner: u8,
) -> impl Iterator<Item = AppendWindow> + '_ {
    instructions.windows(16).enumerate().filter_map(move |(start, window)| {
        let [
            Instruction::LoadByteZero { d: value, a: 1, offset: value_offset },
            Instruction::LoadWord { d: guarded_cursor, a: guard_owner, offset: cursor_offset },
            Instruction::CompareLogicalWordImmediate { a: guarded, immediate: capacity },
            Instruction::BranchConditionalForward { options, condition_bit: 0, target: success },
            Instruction::AddImmediate { d: 3, a: 0, immediate: overflow },
            Instruction::Branch { target: end },
            Instruction::LoadWord { d: cursor, a: cursor_owner, offset: reloaded_cursor_offset },
            Instruction::AddImmediate { d: incremented_cursor, a: incremented_from, immediate: 1 },
            Instruction::StoreWord { s: stored_cursor, a: store_owner, offset: stored_cursor_offset },
            Instruction::AddImmediate { d: cursor_copy, a: copied_cursor, immediate: 0 },
            Instruction::Add { d: byte_address, a: append_owner, b: cursor_index },
            Instruction::StoreByte { s: stored_value, a: byte_base, offset: data_offset },
            Instruction::LoadWord { d: old_length, a: length_owner, offset: length_offset },
            Instruction::AddImmediate { d: new_length, a: incremented_length, immediate: 1 },
            Instruction::StoreWord { s: stored_length, a: length_store_owner, offset: stored_length_offset },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        ] = window
        else {
            return None;
        };
        (*value == 31
            && *guard_owner == owner
            && *cursor_owner == owner
            && *store_owner == owner
            && *append_owner == owner
            && *length_owner == owner
            && *length_store_owner == owner
            && *guarded_cursor == 0
            && *guarded == *guarded_cursor
            && *options == 12
            && *success == start + 6
            && *end == start + 16
            && *cursor_offset == *reloaded_cursor_offset
            && *cursor_offset == *stored_cursor_offset
            && *incremented_from == *cursor
            && *stored_cursor == *incremented_cursor
            && *cursor_copy == 0
            && *copied_cursor == *cursor
            && *cursor_index == *cursor_copy
            && *byte_address == *byte_base
            && *stored_value == *value
            && *incremented_length == *old_length
            && *stored_length == *new_length
            && *length_offset == *stored_length_offset)
            .then_some(AppendWindow {
                start,
                value_offset: *value_offset,
                cursor_offset: *cursor_offset,
                data_offset: *data_offset,
                length_offset: *length_offset,
                capacity: *capacity,
                overflow: *overflow,
            })
    })
}

fn schedule_append(generator: &mut Generator, window: AppendWindow, owner: u8) {
    let start = window.start;
    generator.output.instructions[start] = Instruction::LoadWord {
        d: 3,
        a: owner,
        offset: window.cursor_offset,
    };
    generator.output.instructions[start + 1] = Instruction::LoadByteZero {
        d: 5,
        a: 1,
        offset: window.value_offset,
    };
    generator.output.instructions[start + 2] = Instruction::CompareLogicalWordImmediate {
        a: 3,
        immediate: window.capacity,
    };
    generator.output.instructions[start + 3] = Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 0,
        target: start + 6,
    };
    generator.output.instructions[start + 4] = Instruction::load_immediate(3, window.overflow);
    generator.output.instructions[start + 5] = Instruction::Branch { target: start + 14 };
    generator.output.instructions[start + 6] = Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 1,
    };
    generator.output.instructions[start + 7] = Instruction::Add {
        d: 4,
        a: owner,
        b: 3,
    };
    generator.output.instructions[start + 8] = Instruction::StoreWord {
        s: 0,
        a: owner,
        offset: window.cursor_offset,
    };
    generator.output.instructions[start + 9] = Instruction::load_immediate(3, 0);
    generator.output.instructions[start + 10] = Instruction::StoreByte {
        s: 5,
        a: 4,
        offset: window.data_offset,
    };
    generator.output.instructions[start + 11] = Instruction::LoadWord {
        d: 4,
        a: owner,
        offset: window.length_offset,
    };
    generator.output.instructions[start + 12] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: 1,
    };
    generator.output.instructions[start + 13] = Instruction::StoreWord {
        s: 0,
        a: owner,
        offset: window.length_offset,
    };
    crate::remove_instruction_retargeting_to_next(generator, start + 15);
    crate::remove_instruction_retargeting_to_next(generator, start + 14);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_preloaded_value_append_window() {
        let instructions = vec![
            Instruction::LoadByteZero { d: 31, a: 1, offset: 8 },
            Instruction::LoadWord { d: 0, a: 31, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 2176 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 6 },
            Instruction::load_immediate(3, 769),
            Instruction::Branch { target: 16 },
            Instruction::LoadWord { d: 3, a: 31, offset: 12 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 4, a: 31, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::Add { d: 3, a: 31, b: 0 },
            Instruction::StoreByte { s: 31, a: 3, offset: 16 },
            Instruction::LoadWord { d: 3, a: 31, offset: 8 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 31, offset: 8 },
            Instruction::load_immediate(3, 0),
        ];

        let plan = repeated_value_append(&instructions, 31).expect("append window");
        assert_eq!(plan.start, 0);
        assert_eq!(plan.value_offset, 8);
        assert_eq!(plan.cursor_offset, 12);
        assert_eq!(plan.length_offset, 8);
    }
}
