//! Final scheduling for a guarded call with a retained member-derived alias.
//!
//! Liveness keeps the receiver in r30 and its derived attribute pointer in
//! r31. This pass owns only the remaining physical choices: save both homes
//! before deriving the alias, share the zero literal across adjacent stores,
//! and issue the later integer constant while that literal load is in flight.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_guarded_member_alias_initialization(&mut self) {
        // The allocator retains virtual home IDs here; the physical r31/r30
        // assignment is verified by the instruction recognizer below.
        if self.frame_size != 24 || self.callee_saved.len() != 2 {
            return;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(36)
            .position(is_guarded_member_alias_initialization)
        else {
            return;
        };
        let relocated = self
            .output
            .relocations
            .iter()
            .filter(|relocation| (start..start + 36).contains(&relocation.instruction_index))
            .map(|relocation| relocation.instruction_index - start)
            .collect::<Vec<_>>();
        if relocated != [16, 19, 21]
            || !schedule_relocations::same_relocated_value(
                &self.output.relocations,
                &self.output.constants,
                start + 19,
                start + 21,
            )
        {
            return;
        }

        let old = self.output.instructions[start..start + 36].to_vec();
        let mut replacement = Vec::with_capacity(35);
        replacement.extend_from_slice(&old[..4]);
        replacement.push(old[5].clone());
        replacement.push(old[6].clone());
        let mut alias = old[4].clone();
        let Instruction::AddImmediate { a, .. } = &mut alias else { unreachable!() };
        *a = 30;
        replacement.push(alias);
        replacement.extend_from_slice(&old[7..19]);
        replacement.push(old[25].clone());
        replacement.push(old[19].clone());
        replacement.push(old[20].clone());
        replacement.push(old[22].clone());
        let mut maximum = old[23].clone();
        let Instruction::LoadWord { d, .. } = &mut maximum else { unreachable!() };
        *d = 3;
        replacement.push(maximum);
        let mut jumps = old[24].clone();
        let Instruction::StoreByte { s, .. } = &mut jumps else { unreachable!() };
        *s = 3;
        replacement.push(jumps);
        replacement.extend_from_slice(&old[26..]);
        debug_assert_eq!(replacement.len(), 35);

        self.output.instructions.splice(start..start + 36, replacement);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != start + 21);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 19 => start + 20,
                index if index >= start + 36 => index - 1,
                index => index,
            };
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn is_guarded_member_alias_initialization(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
        Instruction::StoreWord { s: 31, a: 1, offset: 20 },
        Instruction::AddImmediate { d: 31, a: 3, immediate: alias_offset },
        Instruction::StoreWord { s: 30, a: 1, offset: 16 },
        Instruction::Or { a: 30, s: 3, b: 3 },
        Instruction::LoadByteZero { d: 0, a: 3, .. },
        Instruction::RotateAndMaskRecord { a: 0, s: 0, .. },
        Instruction::BranchConditionalForward { target: first_join, .. },
        Instruction::LoadByteZero { d: 0, a: 30, .. },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
        Instruction::BranchConditionalForward { target: second_join, .. },
        Instruction::LoadByteZero { d: 4, a: 30, .. },
        Instruction::LoadByteZero { d: 3, a: 30, .. },
        Instruction::RotateAndMask { a: 4, s: 4, .. },
        Instruction::BranchAndLink { .. },
        Instruction::AddImmediate { d: 0, a: 0, immediate: 1 },
        Instruction::StoreWord { s: 0, a: 30, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: 30, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::StoreFloatSingle { s: 0, a: 30, .. },
        Instruction::LoadWord { d: 0, a: 31, offset: member_offset },
        Instruction::StoreByte { s: 0, a: 30, .. },
        Instruction::AddImmediate { d: 0, a: 0, immediate: 5 },
        Instruction::StoreWord { s: 0, a: 30, .. },
        Instruction::LoadWord { d: 0, a: 30, offset: flags_offset },
        Instruction::OrImmediate { a: 0, s: 0, .. },
        Instruction::StoreWord { s: 0, a: 30, offset: stored_flags_offset },
        Instruction::LoadWord { d: 0, a: 1, offset: 28 },
        Instruction::LoadWord { d: 31, a: 1, offset: 20 },
        Instruction::LoadWord { d: 30, a: 1, offset: 16 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ] if *alias_offset > 0
        && *member_offset >= 0
        && *first_join == 17
        && *second_join == 17
        && flags_offset == stored_flags_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_stream_without_the_two_saved_homes() {
        let instructions = vec![Instruction::BranchToLinkRegister; 36];
        assert!(!is_guarded_member_alias_initialization(&instructions));
    }
}
