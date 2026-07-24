//! Entry scheduling for allocator calls that publish a frame cursor.
//!
//! After two saved flags are extracted from one member, an address-taken
//! cursor is passed to an allocator and reloaded. MWCC fills the call-setup
//! latency slots with the cursor publication and flag extracts while retaining
//! the incoming pointer alias until its load.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_allocator_cursor_entry(&mut self) {
        let Some(start) = allocator_cursor_entry(&self.output.instructions) else {
            return;
        };
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[start + 5] else {
            unreachable!("the cursor load was matched")
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT + 1;

        // Original identities: A B C D E F G H I J
        // Measured order:      A C F B H I G D E J
        self.move_instruction_before(start + 2, start + 1);
        self.move_instruction_before(start + 5, start + 2);
        self.move_instruction_before(start + 7, start + 4);
        self.move_instruction_before(start + 8, start + 5);
        self.move_instruction_before(start + 8, start + 6);
    }
}

fn allocator_cursor_entry(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(10).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: flags,
                    a: Eabi::FIRST_GENERAL_ARGUMENT,
                    ..
                },
                Instruction::Or {
                    a: cursor_parameter,
                    s: 4,
                    b: 4,
                },
                Instruction::Or {
                    a: object_parameter,
                    s: Eabi::FIRST_GENERAL_ARGUMENT,
                    b: Eabi::FIRST_GENERAL_ARGUMENT,
                },
                Instruction::RotateAndMask {
                    a: first_flag,
                    s: first_source,
                    ..
                },
                Instruction::RotateAndMask {
                    a: second_flag,
                    s: second_source,
                    ..
                },
                Instruction::LoadWord {
                    d: 0,
                    a: cursor_load_base,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: frame_offset,
                },
                Instruction::AddImmediate {
                    d: Eabi::FIRST_GENERAL_ARGUMENT,
                    a: 1,
                    immediate: address_offset,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: allocation_size,
                },
                Instruction::BranchAndLink { .. },
            ] if cursor_parameter != object_parameter
                && cursor_parameter == cursor_load_base
                && flags == first_source
                && flags == second_source
                && first_flag != second_flag
                && frame_offset == address_offset
                && *allocation_size > 0
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_published_cursor_allocator_entry() {
        let instructions = vec![
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: 36,
            },
            Instruction::move_register(27, 4),
            Instruction::move_register(26, 3),
            Instruction::RotateAndMask {
                a: 28,
                s: 5,
                shift: 29,
                begin: 31,
                end: 31,
            },
            Instruction::RotateAndMask {
                a: 30,
                s: 5,
                shift: 30,
                begin: 31,
                end: 31,
            },
            Instruction::LoadWord {
                d: 0,
                a: 27,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            Instruction::load_immediate(4, 40),
            Instruction::BranchAndLink {
                target: "allocate".into(),
            },
        ];
        assert_eq!(allocator_cursor_entry(&instructions), Some(0));
    }
}
