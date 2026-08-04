//! Saved-byte value-returning inline append transactions.
//!
//! A caller can pass a saved narrow command into an inlined one-byte append,
//! consume the inline status immediately, then overwrite its source-level
//! status with another call. Linkage-first MWCC keeps only the command and the
//! surrounding call status, computes the inline status in r4, and reuses the
//! first cursor load. This owner schedules that complete physical transaction
//! after allocation has exposed all of those lifetimes.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Transaction {
    append: usize,
    command: u8,
    status: u8,
}

fn copied_from_result(instruction: &Instruction) -> Option<u8> {
    match instruction {
        Instruction::AddImmediate {
            d,
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            immediate: 0,
        } => Some(*d),
        Instruction::Or {
            a,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        } => Some(*a),
        _ => None,
    }
}

fn copied_from_incoming(instruction: &Instruction) -> Option<u8> {
    match instruction {
        Instruction::AddImmediate {
            d,
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            immediate: 0,
        } => Some(*d),
        Instruction::Or {
            a,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        } => Some(*a),
        _ => None,
    }
}

fn copies_register(instruction: &Instruction, destination: u8, source: u8) -> bool {
    match instruction {
        Instruction::AddImmediate {
            d,
            a,
            immediate: 0,
        } => *d == destination && *a == source,
        Instruction::Or { a, s, b } => {
            *a == destination && *s == source && *b == source
        }
        _ => false,
    }
}

fn recorded_result_packets(instructions: &[Instruction]) -> Vec<(usize, u8)> {
    instructions
        .windows(3)
        .enumerate()
        .filter_map(|(call, window)| {
            let [
                Instruction::BranchAndLink { .. },
                Instruction::OrRecord {
                    a: saved,
                    s: Eabi::FIRST_GENERAL_ARGUMENT,
                    b: Eabi::FIRST_GENERAL_ARGUMENT,
                },
                branch,
            ] = window
            else {
                return None;
            };
            ((14..=31).contains(saved)
                && matches!(branch, Instruction::BranchConditionalForward {
                    condition_bit: 2,
                    ..
                }))
            .then_some((call + 1, *saved))
        })
        .collect()
}

fn recognize(instructions: &[Instruction]) -> Option<Transaction> {
    if instructions.len() < 36 {
        return None;
    }
    let [
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 36 },
        Instruction::StoreWord { s: command_store, a: 1, offset: 28 },
        command_copy,
        Instruction::StoreWord { s: status_store, a: 1, offset: 24 },
        Instruction::AddImmediate { d: 3, a: 1, .. },
        Instruction::AddImmediate { d: 4, a: 1, .. },
        Instruction::BranchAndLink { .. },
    ] = &instructions[..9]
    else {
        return None;
    };
    let command = copied_from_incoming(command_copy)?;
    if command != *command_store || command != 30 || *status_store != 31 {
        return None;
    }
    let status = *status_store;

    let packets = recorded_result_packets(instructions);
    if packets.len() != 2
        || packets.iter().any(|(_, saved)| *saved != status)
    {
        return None;
    }

    let append = instructions.windows(19).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord { d: buffer, a: 1, .. },
            Instruction::LoadWord { d: guard, a: guard_buffer, offset: guard_offset },
            Instruction::CompareLogicalWordImmediate { a: compared, .. },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: success },
            Instruction::AddImmediate { d: error_result, a: 0, .. },
            Instruction::Branch { target: end },
            Instruction::LoadWord { d: cursor, a: cursor_buffer, offset: cursor_offset },
            Instruction::AddImmediate { d: incremented, a: incremented_from, immediate: 1 },
            Instruction::StoreWord { s: stored_cursor, a: cursor_store_buffer, offset: stored_cursor_offset },
            cursor_copy,
            Instruction::Add { d: byte_address, a: append_buffer, b: cursor_index },
            Instruction::StoreByte { s: stored_byte, a: byte_base, .. },
            Instruction::LoadWord { d: length, a: length_buffer, offset: length_offset },
            Instruction::AddImmediate { d: incremented_length, a: old_length, immediate: 1 },
            Instruction::StoreWord { s: stored_length, a: length_store_buffer, offset: stored_length_offset },
            Instruction::AddImmediate { d: success_result, a: 0, immediate: 0 },
            result_copy,
            Instruction::CompareWordImmediate { a: tested_result, immediate: 0 },
            Instruction::BranchConditionalForward { condition_bit: 2, .. },
        ] = window else {
            return None;
        };
        let copied_cursor = match cursor_copy {
            Instruction::AddImmediate { d, a, immediate: 0 } => Some((*d, *a)),
            Instruction::Or { a, s, b } if s == b => Some((*a, *s)),
            _ => None,
        };
        let (cursor_index_copy, cursor_source) = copied_cursor?;
        (*success == start + 6
            && *end == start + 16
            && *error_result == Eabi::FIRST_GENERAL_ARGUMENT
            && *success_result == Eabi::FIRST_GENERAL_ARGUMENT
            && copied_from_result(result_copy) == Some(status)
            && *tested_result == status
            && *guard == *compared
            && *guard_buffer == *buffer
            && *cursor_buffer == *buffer
            && *cursor_store_buffer == *buffer
            && *append_buffer == *buffer
            && *length_buffer == *buffer
            && *length_store_buffer == *buffer
            && *guard_offset == *cursor_offset
            && *guard_offset == *stored_cursor_offset
            && *incremented_from == *cursor
            && *stored_cursor == *incremented
            && cursor_source == *cursor
            && cursor_index_copy == *cursor_index
            && *byte_address == *byte_base
            && *stored_byte == command
            && *old_length == *length
            && *stored_length == *incremented_length
            && *length_offset == *stored_length_offset)
            .then_some(start)
    })?;
    if packets[0].0 >= append || packets[1].0 <= append + 18 {
        return None;
    }

    let end = instructions.len();
    let [
        result_copy,
        Instruction::LoadWord { d: command_load, .. },
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::LoadWord { d: status_load, .. },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::BranchToLinkRegister,
    ] = &instructions[end - 7..]
    else {
        return None;
    };
    if *command_load != command
        || *status_load != status
        || !copies_register(result_copy, Eabi::FIRST_GENERAL_ARGUMENT, status)
    {
        return None;
    }
    Some(Transaction {
        append,
        command,
        status,
    })
}

fn rewrite_append(generator: &mut Generator, start: usize) {
    crate::remove_instruction_retargeting_to_next(generator, start + 16);
    crate::remove_instruction_retargeting_to_next(generator, start + 9);
    crate::remove_instruction_retargeting_to_next(generator, start + 6);
    crate::move_instruction_before_retargeting(generator, start + 8, start + 7);
    crate::move_instruction_before_retargeting(generator, start + 13, start + 9);

    let instructions = &mut generator.output.instructions;
    let Instruction::LoadWord { d, .. } = &mut instructions[start] else { unreachable!() };
    *d = 5;
    let Instruction::LoadWord { d, a, .. } = &mut instructions[start + 1] else { unreachable!() };
    *d = 3;
    *a = 5;
    let Instruction::CompareLogicalWordImmediate { a, .. } = &mut instructions[start + 2] else { unreachable!() };
    *a = 3;
    let Instruction::AddImmediate { d, .. } = &mut instructions[start + 4] else { unreachable!() };
    *d = 4;
    let Instruction::AddImmediate { d, a, .. } = &mut instructions[start + 6] else { unreachable!() };
    *d = 0;
    *a = 3;
    let Instruction::Add { d, a, b } = &mut instructions[start + 7] else { unreachable!() };
    *d = 3;
    *a = 5;
    *b = 3;
    let Instruction::StoreWord { s, a, .. } = &mut instructions[start + 8] else { unreachable!() };
    *s = 0;
    *a = 5;
    let Instruction::AddImmediate { d, .. } = &mut instructions[start + 9] else { unreachable!() };
    *d = 4;
    let Instruction::StoreByte { a, .. } = &mut instructions[start + 10] else { unreachable!() };
    *a = 3;
    let Instruction::LoadWord { d, a, .. } = &mut instructions[start + 11] else { unreachable!() };
    *d = 3;
    *a = 5;
    let Instruction::AddImmediate { d, a, .. } = &mut instructions[start + 12] else { unreachable!() };
    *d = 0;
    *a = 3;
    let Instruction::StoreWord { s, a, .. } = &mut instructions[start + 13] else { unreachable!() };
    *s = 0;
    *a = 5;
    let Instruction::CompareWordImmediate { a, .. } = &mut instructions[start + 14] else { unreachable!() };
    *a = 4;
}

fn schedule_entry(generator: &mut Generator, command: u8, status: u8) {
    crate::move_instruction_before_retargeting(generator, 7, 3);
    crate::move_instruction_before_retargeting(generator, 6, 4);
    generator.output.instructions[4] = Instruction::StoreWord {
        s: status,
        a: 1,
        offset: 28,
    };
    generator.output.instructions[5] = Instruction::StoreWord {
        s: command,
        a: 1,
        offset: 24,
    };
}

fn schedule_epilogue(generator: &mut Generator, command: u8, status: u8) {
    let end = generator.output.instructions.len();
    crate::move_instruction_before_retargeting(generator, end - 5, end - 7);
    crate::move_instruction_before_retargeting(generator, end - 4, end - 5);
    let end = generator.output.instructions.len();
    generator.output.instructions[end - 5] = Instruction::LoadWord {
        d: status,
        a: 1,
        offset: 28,
    };
    generator.output.instructions[end - 4] = Instruction::LoadWord {
        d: command,
        a: 1,
        offset: 24,
    };
    let epilogue = end - 7;
    let early_exit = generator
        .output
        .instructions
        .windows(2)
        .position(|window| {
            matches!(window[0], Instruction::OrRecord { a, s: 3, b: 3 } if a == status)
                && matches!(window[1], Instruction::BranchConditionalForward { .. })
        })
        .expect("the transaction owns its initial call-result guard");
    let Instruction::BranchConditionalForward { target, .. } =
        &mut generator.output.instructions[early_exit + 1]
    else {
        unreachable!()
    };
    *target = epilogue;
}

impl Generator {
    pub(crate) fn schedule_structured_saved_value_inlined_byte_append(&mut self) {
        let Some(transaction) = recognize(&self.output.instructions) else {
            return;
        };
        rewrite_append(self, transaction.append);
        schedule_entry(self, transaction.command, transaction.status);
        schedule_epilogue(self, transaction.command, transaction.status);
    }
}
