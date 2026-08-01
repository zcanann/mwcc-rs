//! Final physical schedule for a source-proven buffered memory write.
//!
//! The read and write transactions share their frame proof and initial result
//! chain, but differ in their bounds check, transfer packet, and retained-result
//! lifetime. Keeping the write schedule separate makes those differences
//! explicit and lets either topology fail transactionally.

use super::*;
use super::structured_memory_transfer_schedule::{
    allocated_transfer_frame, canonicalize_owner_copies,
    compact_error_dispatch_container_label_count, dense_error_dispatch, retain_initial_results,
};

impl Generator {
    pub(crate) fn finalize_structured_memory_write_frame(&mut self, function: &Function) {
        if !self.structured_memory_write_frame {
            return;
        }
        let original = self.clone();
        if !self.try_finalize_structured_memory_write_frame(function) {
            *self = original;
        }
    }

    fn try_finalize_structured_memory_write_frame(&mut self, function: &Function) -> bool {
        let Some((frame, epilogue)) = allocated_transfer_frame(&self.output.instructions) else {
            return false;
        };
        let owner = 30;
        let owner_home = 31;
        let result = 30;
        self.output.instructions[frame + 2] = Instruction::Or {
            a: owner_home,
            s: 3,
            b: 3,
        };
        self.output.instructions[frame + 3] = Instruction::StoreWord {
            s: result,
            a: 1,
            offset: self.frame_size - 8,
        };
        for instruction in &mut self.output.instructions[frame + 4..epilogue] {
            mwcc_vreg::for_each_register(instruction, |_, class, register| {
                if class == mwcc_vreg::Class::General && *register == owner {
                    *register = owner_home;
                }
            });
        }

        if !retain_initial_results(self, result) {
            return false;
        }
        if !schedule_write_bounds(self, owner_home) {
            return false;
        }
        if !retain_read_buffer_result(self, result) {
            return false;
        }
        if !schedule_target_access_packet(&mut self.output.instructions) {
            return false;
        }
        if !retain_target_access_result(self, result) {
            return false;
        }
        if !preserve_message_result(self, result) {
            return false;
        }
        if !retain_append_result(self, result) {
            return false;
        }

        let Some(dispatch) = dense_error_dispatch(&self.output.instructions) else {
            return false;
        };
        let Instruction::CompareWordImmediate { a, immediate: 0 } =
            &mut self.output.instructions[dispatch - 2]
        else {
            return false;
        };
        *a = result;
        let Instruction::AddImmediate { a, immediate: -1792, .. } =
            &mut self.output.instructions[dispatch]
        else {
            return false;
        };
        *a = result;

        canonicalize_owner_copies(&mut self.output.instructions, owner_home);
        let Some(deferred_labels) = compact_error_dispatch_container_label_count(function) else {
            return false;
        };
        let Some(front_labels) = self.output.anonymous_label_bump.checked_sub(deferred_labels)
        else {
            return false;
        };
        self.output.anonymous_label_bump = front_labels;
        self.output.post_constant_label_bump = self
            .output
            .post_constant_label_bump
            .saturating_add(deferred_labels);
        true
    }
}

fn call(instructions: &[Instruction], expected: &str) -> Option<usize> {
    instructions.iter().position(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { target } if target == expected)
    })
}

fn schedule_write_bounds(generator: &mut Generator, owner: u8) -> bool {
    let Some(start) = generator.output.instructions.windows(7).position(|window| {
        matches!(window[0], Instruction::LoadWord { d: 0, a, offset: 8 } if a == owner)
            && matches!(window[1], Instruction::LoadHalfwordZero { d: 3, a: 1, .. })
            && matches!(window[2], Instruction::AddImmediate { d: 0, a: 3, immediate: 8 })
            && matches!(window[3], Instruction::CompareLogicalWord { a: 0, b: 0 })
            && matches!(window[4], Instruction::BranchConditionalForward { .. })
            && matches!(window[5], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
            && matches!(window[6], Instruction::CompareWordImmediate { a: 0, immediate: 2048 })
    }) else {
        return false;
    };
    let length_offset = match generator.output.instructions[start + 1] {
        Instruction::LoadHalfwordZero { offset, .. } => offset,
        _ => unreachable!(),
    };
    let branch = generator.output.instructions[start + 4].clone();
    generator.output.instructions[start] = Instruction::LoadHalfwordZero {
        d: 4,
        a: 1,
        offset: length_offset,
    };
    generator.output.instructions[start + 1] = Instruction::LoadWord {
        d: 3,
        a: owner,
        offset: 8,
    };
    generator.output.instructions[start + 2] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: 8,
    };
    generator.output.instructions[start + 3] = Instruction::CompareLogicalWord { a: 3, b: 0 };
    generator.output.instructions[start + 4] = branch;
    generator.output.instructions[start + 5] = Instruction::CompareLogicalWordImmediate {
        a: 4,
        immediate: 2048,
    };
    crate::remove_instruction_retargeting_to_next(generator, start + 6);

    let Some(store) = generator.output.instructions.windows(4).position(|window| {
        matches!(window[0], Instruction::CompareWordImmediate { a: 3, immediate: 0 })
            && matches!(window[1], Instruction::BranchConditionalForward { .. })
            && matches!(window[2], Instruction::LoadHalfwordZero { d: 0, a: 1, .. })
            && matches!(window[3], Instruction::StoreWord { s: 0, a: 1, .. })
    }).map(|start| start + 2)
    else {
        return false;
    };
    let length_word_offset = match generator.output.instructions[store + 1] {
        Instruction::StoreWord { offset, .. } => offset,
        _ => unreachable!(),
    };
    let Instruction::CompareWordImmediate { a, immediate: 0 } =
        &mut generator.output.instructions[store - 2]
    else {
        return false;
    };
    *a = 30;
    generator.output.instructions[store] = Instruction::StoreWord {
        s: 4,
        a: 1,
        offset: length_word_offset,
    };
    crate::remove_instruction_retargeting_to_next(generator, store + 1);
    true
}

fn retain_read_buffer_result(generator: &mut Generator, result: u8) -> bool {
    let Some(call) = call(&generator.output.instructions, "TRKReadBuffer") else {
        return false;
    };
    let Some(Instruction::CompareWordImmediate { a: 3, immediate: 0 }) =
        generator.output.instructions.get(call + 1)
    else {
        return false;
    };
    generator.output.instructions[call + 1] = Instruction::OrRecord {
        a: result,
        s: 3,
        b: 3,
    };
    true
}

fn schedule_target_access_packet(instructions: &mut [Instruction]) -> bool {
    let Some(call) = call(instructions, "TRKTargetAccessMemory") else {
        return false;
    };
    let Some(start) = call.checked_sub(10) else {
        return false;
    };
    let Some(window) = instructions.get(start..=call) else {
        return false;
    };
    if !(matches!(window[0], Instruction::AddImmediate { d: 3, a: 1, .. })
        && matches!(window[1], Instruction::LoadWord { d: 4, a: 1, .. })
        && matches!(window[2], Instruction::AddImmediate { d: 5, a: 1, .. })
        && matches!(window[3], Instruction::LoadByteZero { d: 0, a: 1, .. })
        && matches!(window[4], Instruction::AndMaskRecord { a: 0, s: 0, .. })
        && matches!(window[5], Instruction::BranchConditionalForward { .. })
        && matches!(window[6], Instruction::AddImmediate { d: 6, a: 0, immediate: 0 })
        && matches!(window[7], Instruction::Branch { .. })
        && matches!(window[8], Instruction::AddImmediate { d: 6, a: 0, immediate: 1 })
        && matches!(window[9], Instruction::AddImmediate { d: 7, a: 0, immediate: 0 })
        && matches!(window[10], Instruction::BranchAndLink { .. }))
    {
        return false;
    }
    let buffer_offset = match instructions[start] {
        Instruction::AddImmediate { immediate, .. } => immediate,
        _ => unreachable!(),
    };
    let address_offset = match instructions[start + 1] {
        Instruction::LoadWord { offset, .. } => offset,
        _ => unreachable!(),
    };
    let length_offset = match instructions[start + 2] {
        Instruction::AddImmediate { immediate, .. } => immediate,
        _ => unreachable!(),
    };
    let options_offset = match instructions[start + 3] {
        Instruction::LoadByteZero { offset, .. } => offset,
        _ => unreachable!(),
    };
    instructions[start] = Instruction::LoadByteZero {
        d: 0,
        a: 1,
        offset: options_offset,
    };
    instructions[start + 1] = Instruction::AndMaskRecord {
        a: 0,
        s: 0,
        begin: 28,
        end: 28,
    };
    instructions[start + 2] = Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: start + 5,
    };
    instructions[start + 3] = Instruction::load_immediate(6, 0);
    instructions[start + 4] = Instruction::Branch { target: start + 6 };
    instructions[start + 5] = Instruction::load_immediate(6, 1);
    instructions[start + 6] = Instruction::LoadWord {
        d: 4,
        a: 1,
        offset: address_offset,
    };
    instructions[start + 7] = Instruction::AddImmediate {
        d: 3,
        a: 1,
        immediate: buffer_offset,
    };
    instructions[start + 8] = Instruction::AddImmediate {
        d: 5,
        a: 1,
        immediate: length_offset,
    };
    instructions[start + 9] = Instruction::load_immediate(7, 0);
    true
}

fn retain_target_access_result(generator: &mut Generator, result: u8) -> bool {
    let Some(call) = call(&generator.output.instructions, "TRKTargetAccessMemory") else {
        return false;
    };
    if !matches!(generator.output.instructions.get(call + 1), Some(Instruction::LoadWord { a: 1, .. }))
        || !matches!(generator.output.instructions.get(call + 2), Some(Instruction::StoreHalfword { a: 1, .. }))
        || !matches!(generator.output.instructions.get(call + 3), Some(Instruction::CompareWordImmediate { a: 3, immediate: 0 }))
    {
        return false;
    }
    crate::insert_instruction_retargeting(
        generator,
        call + 1,
        Instruction::Or {
            a: result,
            s: 3,
            b: 3,
        },
    );
    let Instruction::CompareWordImmediate { a, immediate: 0 } =
        &mut generator.output.instructions[call + 4]
    else {
        return false;
    };
    *a = result;
    true
}

fn preserve_message_result(generator: &mut Generator, result: u8) -> bool {
    let Some(call) = call(&generator.output.instructions, "TRKMessageIntoReply") else {
        return false;
    };
    let Some(Instruction::CompareWordImmediate { a, immediate: 0 }) =
        generator.output.instructions.get_mut(call + 1)
    else {
        return false;
    };
    *a = result;
    true
}

fn retain_append_result(generator: &mut Generator, result: u8) -> bool {
    let Some(call) = call(&generator.output.instructions, "TRKAppendBuffer1_ui16") else {
        return false;
    };
    if !matches!(generator.output.instructions.get(call + 1), Some(Instruction::CompareWordImmediate { a: 3, immediate: 0 })) {
        return false;
    }
    crate::insert_instruction_retargeting(
        generator,
        call + 1,
        Instruction::Or {
            a: result,
            s: 3,
            b: 3,
        },
    );
    let Instruction::CompareWordImmediate { a, immediate: 0 } =
        &mut generator.output.instructions[call + 2]
    else {
        return false;
    };
    *a = result;
    true
}
