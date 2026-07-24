//! Final schedules for values retained across short-circuit guard terms.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Keep one storage byte live through adjacent short-circuit bit tests.
    /// Both terms branch to the same false edge, so the first load remains
    /// available on the only path that reaches the second mask.
    pub(crate) fn schedule_shared_guard_storage_byte(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(6)
            .position(is_reloaded_guard_storage_byte)
        else {
            return;
        };
        match &mut self.output.instructions[start] {
            Instruction::LoadByteZero { d, .. } => *d = Eabi::FIRST_GENERAL_ARGUMENT,
            _ => unreachable!(),
        }
        for index in [start + 1, start + 4] {
            match &mut self.output.instructions[index] {
                Instruction::RotateAndMaskRecord { s, .. } => {
                    *s = Eabi::FIRST_GENERAL_ARGUMENT
                }
                _ => unreachable!(),
            }
        }
        self.remove_structured_condition_instruction(start + 3);
    }

    /// Retain a tested bitfield storage byte through a pure guard and delay an
    /// unrelated shared-global load until its first dependent term.
    ///
    /// MWCC assigns the storage byte a nonvolatile argument register because
    /// the taken arm later updates the same field.  The generic allocator
    /// instead preloads the shared global, discards the byte after its test,
    /// and reloads it for the update.  Recognizing the complete guard/update
    /// region lets this final pass exchange those lifetimes without extending
    /// either value across the call at the end of the arm.
    pub(crate) fn schedule_guarded_bitfield_storage_cache(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(25)
            .position(is_guarded_bitfield_storage_cache)
        else {
            return;
        };
        if !self
            .output
            .relocations
            .iter()
            .any(|relocation| relocation.instruction_index == start)
        {
            return;
        }

        let retained = 5;
        let shared_global = 6;
        match &mut self.output.instructions[start + 1] {
            Instruction::LoadByteZero { d, .. } => *d = retained,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 2] {
            Instruction::RotateAndMaskRecord { s, .. } => *s = retained,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start] {
            Instruction::LoadWord { d, .. } => *d = shared_global,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 9] {
            Instruction::LoadFloatSingle { a, .. } => *a = shared_global,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 15] {
            Instruction::LoadWord { a, .. } => *a = shared_global,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 19] {
            Instruction::AddImmediate { d, .. } => *d = 0,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 20] {
            Instruction::RotateAndMaskInsert { a, s, .. } => {
                *a = retained;
                *s = 0;
            }
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 21] {
            Instruction::StoreByte { s, .. } => *s = retained,
            _ => unreachable!(),
        }
        // Start the call's first literal argument before the independent timer
        // store, filling the store's issue slot without changing memory order.
        self.output.instructions.swap(start + 23, start + 24);

        // The retained byte makes the arm's reload redundant.
        self.remove_structured_condition_instruction(start + 18);

        // Fill the first two guard terms before materializing the global base.
        // `permutation[old] = new`; relocation and branch destinations follow
        // the same permutation as the instruction stream.
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        permutation[start] = start + 7;
        for (old, destination) in permutation
            .iter_mut()
            .enumerate()
            .take(start + 8)
            .skip(start + 1)
        {
            *destination = old - 1;
        }
        self.output.instructions[start..start + 8].rotate_left(1);
        crate::remap_instruction_indices(self, &permutation);
        // Relocation records follow scheduled instruction order in MWCC's
        // object stream, not the pre-schedule discovery order.
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn is_reloaded_guard_storage_byte(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadByteZero { d: 0, a: first_base, offset: first_offset },
        Instruction::RotateAndMaskRecord { a: 0, s: 0, .. },
        Instruction::BranchConditionalForward { target: first_target, .. },
        Instruction::LoadByteZero { d: 0, a: second_base, offset: second_offset },
        Instruction::RotateAndMaskRecord { a: 0, s: 0, .. },
        Instruction::BranchConditionalForward { target: second_target, .. },
    ] if first_base == second_base
        && first_offset == second_offset
        && first_target == second_target)
}

fn is_guarded_bitfield_storage_cache(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: global, a: 0, .. },
        Instruction::LoadByteZero { d: 0, a: receiver, offset: storage_offset },
        Instruction::RotateAndMaskRecord { a: 0, s: 0, .. },
        Instruction::BranchConditionalForward { .. },
        _, _, _,
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadFloatSingle { .. },
        Instruction::LoadFloatSingle { a: float_base, .. },
        _, _, _,
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadByteZero { a: timer_base, offset: timer_offset, .. },
        Instruction::LoadWord { a: threshold_base, .. },
        _,
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadByteZero { d: 0, a: reload_base, offset: reload_offset },
        Instruction::AddImmediate { d: inserted, a: 0, immediate: 1 },
        Instruction::RotateAndMaskInsert { a: 0, s, .. },
        Instruction::StoreByte { s: 0, a: store_base, offset: store_offset },
        Instruction::AddImmediate { d: 0, a: 0, immediate: 254 },
        Instruction::StoreByte { s: 0, a: timer_store_base, offset: timer_store_offset },
        Instruction::AddImmediate { d: 4, a: 0, .. },
    ] if global == float_base
        && global == threshold_base
        && receiver == timer_base
        && receiver == reload_base
        && receiver == store_base
        && storage_offset == reload_offset
        && storage_offset == store_offset
        && timer_base == timer_store_base
        && timer_offset == timer_store_offset
        && inserted == s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_reloaded_guarded_bitfield_with_a_hoisted_global() {
        let instructions = [
            Instruction::LoadWord { d: 5, a: 0, offset: 0 },
            Instruction::LoadByteZero { d: 0, a: 3, offset: 8730 },
            Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 29, begin: 31, end: 31 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 30 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 132 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 30 },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: 1572 },
            Instruction::LoadFloatSingle { d: 0, a: 5, offset: 136 },
            Instruction::FloatNegate { d: 0, b: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::ConditionRegisterOr { d: 2, a: 0, b: 2 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 30 },
            Instruction::LoadByteZero { d: 4, a: 3, offset: 1649 },
            Instruction::LoadWord { d: 0, a: 5, offset: 140 },
            Instruction::CompareWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 30 },
            Instruction::LoadByteZero { d: 0, a: 3, offset: 8730 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::RotateAndMaskInsert { a: 0, s: 4, shift: 3, begin: 28, end: 28 },
            Instruction::StoreByte { s: 0, a: 3, offset: 8730 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 254 },
            Instruction::StoreByte { s: 0, a: 3, offset: 1649 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 150 },
        ];

        assert!(is_guarded_bitfield_storage_cache(&instructions));
    }

    #[test]
    fn recognizes_adjacent_masks_of_one_guard_storage_byte() {
        let instructions = [
            Instruction::LoadByteZero { d: 0, a: 31, offset: 8729 },
            Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 26,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 8,
            },
            Instruction::LoadByteZero { d: 0, a: 31, offset: 8729 },
            Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 30,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 8,
            },
        ];

        assert!(is_reloaded_guard_storage_byte(&instructions));
    }
}
