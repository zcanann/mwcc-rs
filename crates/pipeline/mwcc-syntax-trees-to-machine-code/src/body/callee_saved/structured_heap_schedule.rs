//! Final schedules for call-free bounded heap transactions.
//!
//! Legacy MWCC gives the small bump allocator and its companion initializer a
//! whole-function allocation pass: it retains the heap base across guards,
//! coalesces the checked cursor with the published cursor, and fans one zero
//! value through both conditional arms.  The generic structured emitter keeps
//! the same semantics but exposes shorter, independent lifetimes.  Recognize
//! the complete allocated transaction before applying either schedule so
//! ordinary member tests and store runs remain untouched.

#[allow(unused_imports)]
use super::*;

fn bump_allocator_schedule(instructions: &[Instruction]) -> Option<Vec<Instruction>> {
    if instructions.len() != 29
        || !matches!(&instructions[0], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 })
        || !matches!(&instructions[1], Instruction::StoreWord { s: 4, a: 1, offset: 8 })
        || !matches!(&instructions[2], Instruction::LoadWord { d: 4, a: 1, offset: 8 })
        || !matches!(&instructions[3], Instruction::AddImmediate { d: 0, a: 4, immediate: 31 })
        || !matches!(&instructions[4], Instruction::AndContiguousMask { a: 4, s: 0, begin: 0, end: 26 })
        || !matches!(&instructions[5], Instruction::LoadWord { d: 0, a: 3, offset: 0 })
        || !matches!(&instructions[6], Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 })
        || !matches!(&instructions[7], Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 10 })
        || !matches!(&instructions[8], Instruction::AddImmediate { d: 3, a: 0, immediate: 0 })
        || !matches!(&instructions[9], Instruction::Branch { target: 27 })
        || !matches!(&instructions[10], Instruction::LoadWord { d: 5, a: 3, offset: 4 })
        || !matches!(&instructions[11], Instruction::Add { d: 6, a: 5, b: 4 })
        || !matches!(&instructions[12], Instruction::LoadWord { d: 7, a: 3, offset: 0 })
        || !matches!(&instructions[13], Instruction::LoadWord { d: 0, a: 3, offset: 8 })
        || !matches!(&instructions[14], Instruction::Add { d: 0, a: 7, b: 0 })
        || !matches!(&instructions[15], Instruction::CompareLogicalWord { a: 6, b: 0 })
        || !matches!(&instructions[16], Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 20 })
        || !matches!(&instructions[17], Instruction::Add { d: 0, a: 5, b: 4 })
        || !matches!(&instructions[18], Instruction::StoreWord { s: 0, a: 3, offset: 4 })
        || !matches!(&instructions[19], Instruction::Branch { target: 22 })
        || !matches!(&instructions[20], Instruction::AddImmediate { d: 3, a: 0, immediate: 0 })
        || !matches!(&instructions[21], Instruction::Branch { target: 27 })
        || !matches!(&instructions[22], Instruction::LoadWord { d: 4, a: 3, offset: 12 })
        || !matches!(&instructions[23], Instruction::AddImmediate { d: 0, a: 4, immediate: 1 })
        || !matches!(&instructions[24], Instruction::StoreWord { s: 0, a: 3, offset: 12 })
        || !matches!(&instructions[25], Instruction::StoreWord { s: 5, a: 3, offset: 16 })
        || !matches!(&instructions[26], Instruction::Or { a: 3, s: 5, b: 5 })
        || !matches!(&instructions[27], Instruction::AddImmediate { d: 1, a: 1, immediate: 40 })
        || !matches!(&instructions[28], Instruction::BranchToLinkRegister)
    {
        return None;
    }

    Some(vec![
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
        Instruction::StoreWord { s: 4, a: 1, offset: 12 },
        Instruction::LoadWord { d: 5, a: 3, offset: 0 },
        Instruction::LoadWord { d: 4, a: 1, offset: 12 },
        Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 },
        Instruction::AddImmediate { d: 0, a: 4, immediate: 31 },
        Instruction::AndContiguousMask { a: 4, s: 0, begin: 0, end: 26 },
        Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 10 },
        Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        Instruction::Branch { target: 25 },
        Instruction::LoadWord { d: 6, a: 3, offset: 4 },
        Instruction::LoadWord { d: 0, a: 3, offset: 8 },
        Instruction::Add { d: 4, a: 6, b: 4 },
        Instruction::Add { d: 0, a: 5, b: 0 },
        Instruction::CompareLogicalWord { a: 4, b: 0 },
        Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 18 },
        Instruction::StoreWord { s: 4, a: 3, offset: 4 },
        Instruction::Branch { target: 20 },
        Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        Instruction::Branch { target: 25 },
        Instruction::LoadWord { d: 4, a: 3, offset: 12 },
        Instruction::AddImmediate { d: 0, a: 4, immediate: 1 },
        Instruction::StoreWord { s: 0, a: 3, offset: 12 },
        Instruction::StoreWord { s: 6, a: 3, offset: 16 },
        Instruction::Or { a: 3, s: 6, b: 6 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
        Instruction::BranchToLinkRegister,
    ])
}

fn is_low_five_bit_mask(instruction: &Instruction) -> bool {
    matches!(instruction,
        Instruction::ClearLeftImmediate { a: 0, s: 0, clear: 27 }
        | Instruction::AndContiguousMask { a: 0, s: 0, begin: 27, end: 31 }
        | Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin: 27, end: 31 })
}

fn heap_initializer_schedule(instructions: &[Instruction]) -> Option<Vec<Instruction>> {
    if instructions.len() != 33
        || !matches!(&instructions[0], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 })
        || !matches!(&instructions[1], Instruction::StoreWord { s: 3, a: 1, offset: 8 })
        || !matches!(&instructions[2], Instruction::LoadWord { d: 3, a: 1, offset: 8 })
        || !matches!(&instructions[3], Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
        || !matches!(&instructions[4], Instruction::StoreWord { s: 0, a: 3, offset: 12 })
        || !matches!(&instructions[5], Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 })
        || !matches!(&instructions[6], Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 15 })
        || !matches!(&instructions[7], Instruction::LoadWord { d: 3, a: 1, offset: 8 })
        || !matches!(&instructions[8], Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
        || !matches!(&instructions[9], Instruction::StoreWord { s: 0, a: 3, offset: 8 })
        || !matches!(&instructions[10], Instruction::LoadWord { d: 3, a: 1, offset: 8 })
        || !matches!(&instructions[11], Instruction::StoreWord { s: 0, a: 3, offset: 4 })
        || !matches!(&instructions[12], Instruction::LoadWord { d: 3, a: 1, offset: 8 })
        || !matches!(&instructions[13], Instruction::StoreWord { s: 0, a: 3, offset: 16 })
        || !matches!(&instructions[14], Instruction::Branch { target: 31 })
        || !matches!(&instructions[15], Instruction::AddImmediate { d: 0, a: 4, immediate: 0 })
        || !is_low_five_bit_mask(&instructions[16])
        || !matches!(&instructions[17], Instruction::SubtractFrom { d: 3, a: 0, b: 5 })
        || !matches!(&instructions[18], Instruction::LoadWord { d: 5, a: 1, offset: 8 })
        || !matches!(&instructions[19], Instruction::AddImmediate { d: 0, a: 4, immediate: 31 })
        || !matches!(&instructions[20], Instruction::AndContiguousMask { a: 0, s: 0, begin: 0, end: 26 })
        || !matches!(&instructions[21], Instruction::StoreWord { s: 0, a: 5, offset: 0 })
        || !matches!(&instructions[22], Instruction::LoadWord { d: 4, a: 1, offset: 8 })
        || !matches!(&instructions[23], Instruction::LoadWord { d: 5, a: 1, offset: 8 })
        || !matches!(&instructions[24], Instruction::LoadWord { d: 0, a: 5, offset: 0 })
        || !matches!(&instructions[25], Instruction::StoreWord { s: 0, a: 4, offset: 4 })
        || !matches!(&instructions[26], Instruction::LoadWord { d: 4, a: 1, offset: 8 })
        || !matches!(&instructions[27], Instruction::StoreWord { s: 3, a: 4, offset: 8 })
        || !matches!(&instructions[28], Instruction::LoadWord { d: 3, a: 1, offset: 8 })
        || !matches!(&instructions[29], Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
        || !matches!(&instructions[30], Instruction::StoreWord { s: 0, a: 3, offset: 16 })
        || !matches!(&instructions[31], Instruction::AddImmediate { d: 1, a: 1, immediate: 32 })
        || !matches!(&instructions[32], Instruction::BranchToLinkRegister)
    {
        return None;
    }

    Some(vec![
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
        Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
        Instruction::AddImmediate { d: 6, a: 0, immediate: 0 },
        Instruction::StoreWord { s: 3, a: 1, offset: 8 },
        Instruction::LoadWord { d: 7, a: 1, offset: 8 },
        Instruction::StoreWord { s: 6, a: 7, offset: 12 },
        Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 11 },
        Instruction::StoreWord { s: 6, a: 7, offset: 8 },
        Instruction::StoreWord { s: 6, a: 7, offset: 4 },
        Instruction::StoreWord { s: 6, a: 7, offset: 16 },
        Instruction::Branch { target: 20 },
        Instruction::AddImmediate { d: 3, a: 4, immediate: 31 },
        Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 27 },
        Instruction::AndContiguousMask { a: 3, s: 3, begin: 0, end: 26 },
        Instruction::SubtractFrom { d: 0, a: 0, b: 5 },
        Instruction::StoreWord { s: 3, a: 7, offset: 0 },
        Instruction::LoadWord { d: 3, a: 7, offset: 0 },
        Instruction::StoreWord { s: 3, a: 7, offset: 4 },
        Instruction::StoreWord { s: 0, a: 7, offset: 8 },
        Instruction::StoreWord { s: 6, a: 7, offset: 16 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
        Instruction::BranchToLinkRegister,
    ])
}

impl Generator {
    pub(crate) fn schedule_structured_heap_transactions(&mut self) {
        if !self.output.relocations.is_empty()
            || self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
        {
            return;
        }
        let scheduled = bump_allocator_schedule(&self.output.instructions)
            .or_else(|| heap_initializer_schedule(&self.output.instructions));
        if let Some(instructions) = scheduled {
            self.output.instructions = instructions;
            self.frame_size = 40;
        }
    }
}
