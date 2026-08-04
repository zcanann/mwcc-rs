//! Issue scheduling for calls with a byte argument and two member addresses.
//!
//! Build 163 loads an independent byte argument before forming r3/r4 address
//! arguments. When a saved float literal is also initialized beside the first
//! call, one address calculation fills the literal-load latency slot.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_byte_call_arguments(&mut self) -> usize {
        let mut scheduled = 0;
        if let Some((literal, address)) = float_literal_interleave(&self.output.instructions) {
            crate::move_instruction_before_retargeting(self, address, literal);
            scheduled += 1;
        }
        let calls = byte_argument_calls(&self.output.instructions);
        for (start, byte) in calls {
            // The address calculation may be the entry of a guarded join.  The
            // byte argument becomes the call window's new semantic entry, so
            // incoming edges must execute it too.
            crate::retarget_instruction_destinations(self, start, byte);
            crate::move_instruction_before_retargeting(self, byte, start);
            scheduled += 1;
        }
        scheduled
    }
}

fn float_literal_interleave(instructions: &[Instruction]) -> Option<(usize, usize)> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        matches!(&window, [
            Instruction::LoadFloatSingle { d: first, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: second, a: 0, offset: 0 },
            Instruction::AddImmediate { d: 3, a: owner, .. },
            Instruction::AddImmediate { d: 4, a: address_owner, .. },
            Instruction::LoadByteZero { d: 5, a: byte_owner, .. },
            Instruction::BranchAndLink { .. },
        ] if *first != 0
            && *second != 0
            && owner == address_owner
            && owner == byte_owner)
        .then_some((start + 1, start + 2))
    })
}

fn byte_argument_calls(instructions: &[Instruction]) -> Vec<(usize, usize)> {
    instructions
        .windows(4)
        .enumerate()
        .filter_map(|(start, window)| {
            matches!(&window, [
                Instruction::AddImmediate { d: 3, a: owner, .. },
                Instruction::AddImmediate { d: 4, a: address_owner, .. },
                Instruction::LoadByteZero { d: 5, a: byte_owner, .. },
                Instruction::BranchAndLink { .. },
            ] if owner == address_owner && owner == byte_owner)
            .then_some((start, start + 2))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_saved_literal_latency_slot_before_a_byte_call() {
        let instructions = vec![
            Instruction::LoadFloatSingle { d: 31, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 28, a: 0, offset: 0 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 212 },
            Instruction::AddImmediate { d: 4, a: 30, immediate: 188 },
            Instruction::LoadByteZero { d: 5, a: 30, offset: 185 },
            Instruction::BranchAndLink { target: "sample".into() },
        ];
        assert_eq!(float_literal_interleave(&instructions), Some((1, 2)));
    }

    #[test]
    fn finds_a_byte_argument_issued_after_two_addresses() {
        let instructions = vec![
            Instruction::AddImmediate { d: 3, a: 30, immediate: 212 },
            Instruction::AddImmediate { d: 4, a: 30, immediate: 188 },
            Instruction::LoadByteZero { d: 5, a: 30, offset: 185 },
            Instruction::BranchAndLink { target: "sample".into() },
        ];
        assert_eq!(byte_argument_calls(&instructions), vec![(0, 2)]);
    }
}
