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
        self.schedule_allocator_cursor_result();
    }

    fn schedule_allocator_cursor_result(&mut self) {
        let Some(region) = allocator_cursor_result(&self.output) else {
            return;
        };
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[region.pool_high]
        else {
            unreachable!("the pool high half was matched")
        };
        *d = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        let Instruction::LoadFloatSingle { a, .. } = &mut self.output.instructions[region.pool_low]
        else {
            unreachable!("the pool load was matched")
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.output.instructions[region.zero] = Instruction::load_immediate(5, 0);
        let Instruction::StoreHalfword { s, a, .. } =
            &mut self.output.instructions[region.first_store]
        else {
            unreachable!("the first member store was matched")
        };
        *s = 5;
        *a = Eabi::FIRST_GENERAL_ARGUMENT;

        self.move_instruction_before(region.cursor_reload, region.result_copy);
        self.move_instruction_before(region.zero, region.result_copy + 1);
        self.move_instruction_before(region.pool_high, region.result_copy + 2);
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

#[derive(Clone, Copy)]
struct AllocatorCursorResult {
    result_copy: usize,
    cursor_reload: usize,
    zero: usize,
    first_store: usize,
    pool_high: usize,
    pool_low: usize,
}

fn allocator_cursor_result(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<AllocatorCursorResult> {
    for result_copy in 1..output.instructions.len().saturating_sub(4) {
        if !matches!(
            output.instructions[result_copy - 1],
            Instruction::BranchAndLink { .. }
        ) {
            continue;
        }
        let [Instruction::Or {
            a: result,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        }, Instruction::LoadWord {
            d: cursor, a: 1, ..
        }, Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        }, Instruction::StoreHalfword {
            s: 0,
            a: store_base,
            offset: 0,
        }, ..] = &output.instructions[result_copy..]
        else {
            continue;
        };
        if result != store_base || result == cursor {
            continue;
        }
        let search_end = (result_copy + 12).min(output.instructions.len().saturating_sub(1));
        let Some(pool_high) = (result_copy + 4..search_end).find(|&high| {
            matches!(
                (&output.instructions[high], &output.instructions[high + 1]),
                (
                    Instruction::AddImmediateShifted {
                        d: base,
                        a: 0,
                        immediate: 0,
                    },
                    Instruction::LoadFloatSingle {
                        d: _,
                        a: load_base,
                        offset: 0,
                    },
                ) if base == load_base
            ) && output.relocations.iter().any(|relocation| {
                relocation.instruction_index == high && relocation.kind == RelocationKind::Addr16Ha
            }) && output.relocations.iter().any(|relocation| {
                relocation.instruction_index == high + 1
                    && relocation.kind == RelocationKind::Addr16Lo
            })
        }) else {
            continue;
        };
        if output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if (result_copy..=pool_high + 1).contains(target)
            )
        }) {
            continue;
        }
        return Some(AllocatorCursorResult {
            result_copy,
            cursor_reload: result_copy + 1,
            zero: result_copy + 2,
            first_store: result_copy + 3,
            pool_high,
            pool_low: pool_high + 1,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

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

    #[test]
    fn recognizes_the_allocator_result_publication_window() {
        let mut output = mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::BranchAndLink {
                    target: "allocate".into(),
                },
                Instruction::move_register(31, 3),
                Instruction::LoadWord {
                    d: 29,
                    a: 1,
                    offset: 8,
                },
                Instruction::load_immediate(0, 0),
                Instruction::StoreHalfword {
                    s: 0,
                    a: 31,
                    offset: 0,
                },
                Instruction::LoadHalfwordZero {
                    d: 0,
                    a: 26,
                    offset: 8,
                },
                Instruction::ShiftLeftImmediate {
                    a: 3,
                    s: 0,
                    shift: 2,
                },
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 3,
                    offset: 0,
                },
            ],
            ..Default::default()
        };
        output.relocations = vec![
            Relocation {
                instruction_index: 7,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::Constant(0),
            },
            Relocation {
                instruction_index: 8,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::Constant(0),
            },
        ];
        let region = allocator_cursor_result(&output).expect("the result window should match");
        assert_eq!(
            (
                region.result_copy,
                region.cursor_reload,
                region.zero,
                region.first_store,
                region.pool_high,
                region.pool_low,
            ),
            (1, 2, 3, 4, 7, 8)
        );
    }
}
