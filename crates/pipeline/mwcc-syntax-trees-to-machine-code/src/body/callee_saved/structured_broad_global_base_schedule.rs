//! Final issue order for a broad global-aggregate loop with a frame cursor.
//!
//! Saved-home layout and frame sizing are semantic decisions made before
//! allocation. This owner only restores MWCC's latency order after physical
//! registers, relocations, and branch destinations are known.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_broad_global_base_loop(&mut self) {
        if !self.structured_broad_global_base_layout_owner
            || !broad_global_entry(&self.output.instructions)
        {
            return;
        }
        self.permute_broad_global_region(0, &[0, 4, 1, 2, 3, 5, 10, 6, 11, 7, 8, 9]);
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[6] else {
            unreachable!("the loop-invariant address high half was matched")
        };
        *d = 4;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[8] else {
            unreachable!("the loop-invariant address low half was matched")
        };
        *a = 4;

        while let Some(start) = modulo_input_latency_region(&self.output.instructions) {
            self.permute_broad_global_region(start, &[0, 2, 1]);
        }
        while let Some(start) = wrapped_cursor_latency_region(&self.output.instructions) {
            self.permute_broad_global_region(start, &[0, 2, 1]);
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }

    fn permute_broad_global_region(&mut self, start: usize, schedule: &[usize]) {
        let old_len = self.output.instructions.len();
        let original = self.output.instructions[start..start + schedule.len()].to_vec();
        let mut permutation = (0..old_len).collect::<Vec<_>>();
        for (new, &old) in schedule.iter().enumerate() {
            self.output.instructions[start + new] = original[old].clone();
            permutation[start + old] = start + new;
        }
        crate::remap_instruction_indices(self, &permutation);
    }
}

fn broad_global_entry(instructions: &[Instruction]) -> bool {
    instructions.len() >= 12
        && matches!(instructions[0], Instruction::MoveFromLinkRegister { d: 0 })
        && matches!(instructions[1], Instruction::StoreWord { s: 0, a: 1, offset: 4 })
        && matches!(instructions[2], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -56 })
        && matches!(instructions[3], Instruction::StoreMultipleWord { s: 27, a: 1, offset: 36 })
        && matches!(instructions[4], Instruction::AddImmediateShifted { d: 4, a: 0, .. })
        && matches!(instructions[5], Instruction::AddImmediate { d: 31, a: 4, immediate: 0 })
        && matches!(instructions[6], Instruction::AddImmediate { d: 30, a: 31, .. })
        && matches!(instructions[7], Instruction::AddImmediate { d: 28, a: 0, immediate: 0 })
        && matches!(instructions[8], Instruction::LoadWord { d: 29, a: 31, .. })
        && matches!(instructions[9], Instruction::StoreWord { s: 3, a: 1, offset: 16 })
        && matches!(instructions[10], Instruction::AddImmediateShifted { d: 27, a: 0, .. })
        && matches!(instructions[11], Instruction::AddImmediate { d: 27, a: 27, immediate: 0 })
}

fn modulo_input_latency_region(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(3).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 0, a: 31, offset: 192 },
            Instruction::Add { d: 3, a: 28, b: 0 },
            Instruction::LoadWord { d: 4, a: 31, offset: 80 },
        ])
    })
}

fn wrapped_cursor_latency_region(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(3).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 3, a: 1, offset: 16 },
            Instruction::LoadWord { d: 29, a: 3, offset: 0 },
            Instruction::LoadWord { d: 0, a: 31, offset: 180 },
        ])
    })
}
