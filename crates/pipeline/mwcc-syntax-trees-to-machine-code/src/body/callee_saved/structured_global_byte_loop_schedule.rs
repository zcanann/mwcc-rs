//! Final physical issue order for a dense global-byte decoder loop.
//!
//! The semantic layout planner owns this pass.  At this point every retained
//! value already has MWCC's physical home; the remaining work is latency
//! scheduling and the register-copy spelling chosen for those homes.  Every
//! permutation remaps relocations and resolved branch destinations.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_global_byte_loop(&mut self) {
        if !self.structured_global_byte_loop_layout_owner || !decoder_entry(&self.output.instructions)
        {
            return;
        }

        self.permute_global_byte_loop_region(
            0,
            &[0, 4, 1, 2, 3, 5, 7, 6, 10, 8, 11, 12, 9, 13, 14, 16, 15, 18, 17, 19],
        );
        rewrite_decoder_entry(&mut self.output.instructions[..20]);
        self.schedule_decoder_call_arguments();
        self.schedule_decoder_result_test();
        self.spell_decoder_saved_copies();
        self.schedule_decoder_publication();
        self.schedule_decoder_loop_step();
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }

    fn schedule_decoder_call_arguments(&mut self) {
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(window, [
                Instruction::AddImmediate { d: 3, a: 25, immediate: 0 },
                Instruction::LoadWord { d: 4, a: 28, offset: 0 },
                Instruction::LoadWord { d: 5, a: 28, offset: 4 },
                Instruction::LoadWord { d: 6, a: 28, offset: 8 },
                Instruction::LoadWord { d: 7, a: 31, offset: 156 },
                Instruction::BranchAndLink { target },
            ] if target == "THPVideoDecode")
        }) else {
            return;
        };
        self.permute_global_byte_loop_region(start, &[1, 0, 2, 3, 4, 5]);
        // This permutation schedules the first argument load ahead of the
        // original region head. Incoming control flow denotes the transaction
        // entry, not the moved copy's instruction identity.
        for instruction in &mut self.output.instructions[..start] {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == start + 1 =>
                {
                    *target = start;
                }
                _ => {}
            }
        }
        self.output.instructions[start + 1] = Instruction::Or { a: 3, s: 25, b: 25 };
    }

    fn schedule_decoder_result_test(&mut self) {
        let Some(start) = self.output.instructions.windows(3).position(|window| {
            matches!(window, [
                Instruction::StoreWord { s: 3, a: 31, offset: 172 },
                Instruction::CompareWordImmediate { a: 3, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
            ])
        }) else {
            return;
        };
        self.permute_global_byte_loop_region(start, &[1, 0, 2]);
    }

    fn spell_decoder_saved_copies(&mut self) {
        let Some(copy) = self.output.instructions.windows(2).position(|pair| {
            matches!(pair, [
                Instruction::AddImmediate { d: 3, a: 23, immediate: 0 },
                Instruction::BranchAndLink { target },
            ] if target == "OSSuspendThread")
        }) else {
            return;
        };
        self.output.instructions[copy] = Instruction::Or { a: 3, s: 23, b: 23 };
    }

    fn schedule_decoder_publication(&mut self) {
        let Some(start) = self.output.instructions.windows(4).position(|window| {
            matches!(window, [
                Instruction::LoadWord { d: 0, a: 24, offset: 4 },
                Instruction::StoreWord { s: 0, a: 28, offset: 12 },
                Instruction::AddImmediate { d: 3, a: 28, immediate: 0 },
                Instruction::BranchAndLink { target },
            ] if target == "PushDecodedTextureSet")
        }) else {
            return;
        };
        self.permute_global_byte_loop_region(start, &[0, 2, 1, 3]);
        self.output.instructions[start + 1] = Instruction::Or { a: 3, s: 28, b: 28 };
    }

    fn schedule_decoder_loop_step(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(window, [
                Instruction::LoadWord { d: 0, a: 26, offset: 0 },
                Instruction::Add { d: 25, a: 25, b: 0 },
                Instruction::AddImmediate { d: 26, a: 26, immediate: 4 },
                Instruction::AddImmediate { d: 27, a: 27, immediate: 1 },
                Instruction::AddImmediate { d: 30, a: 30, immediate: 1 },
            ])
        }) else {
            return;
        };
        self.permute_global_byte_loop_region(start, &[0, 3, 2, 1, 4]);
    }

    fn permute_global_byte_loop_region(&mut self, start: usize, schedule: &[usize]) {
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

fn decoder_entry(instructions: &[Instruction]) -> bool {
    instructions.len() >= 20
        && matches!(instructions[0], Instruction::MoveFromLinkRegister { d: 0 })
        && matches!(instructions[1], Instruction::StoreWord { s: 0, a: 1, offset: 4 })
        && matches!(instructions[2], Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -56 })
        && matches!(instructions[3], Instruction::StoreMultipleWord { s: 23, a: 1, offset: 20 })
        && matches!(instructions[4], Instruction::AddImmediateShifted { d: 4, a: 0, .. })
        && matches!(instructions[5], Instruction::AddImmediate { d: 31, a: 4, immediate: 0 })
        && matches!(&instructions[14], Instruction::BranchAndLink { target } if target == "PopFreeTextureSet")
        && matches!(instructions[15], Instruction::AddImmediate { d: 28, a: 3, immediate: 0 })
        && matches!(instructions[16], Instruction::AddImmediateShifted { d: 23, a: 0, .. })
        && matches!(instructions[19], Instruction::AddImmediate { d: 27, a: 0, immediate: 0 })
}

fn rewrite_decoder_entry(entry: &mut [Instruction]) {
    entry[6] = Instruction::Or { a: 24, s: 3, b: 3 };
    entry[8] = Instruction::LoadWord { d: 0, a: 31, offset: 108 };
    entry[9] = Instruction::LoadWord { d: 4, a: 3, offset: 0 };
    entry[10] = Instruction::RotateAndMask {
        a: 3,
        s: 0,
        shift: 2,
        begin: 0,
        end: 29,
    };
    entry[11] = Instruction::AddImmediate { d: 25, a: 3, immediate: 8 };
    entry[12] = Instruction::AddImmediate { d: 26, a: 4, immediate: 8 };
    entry[13] = Instruction::Add { d: 25, a: 4, b: 25 };
    entry[15] = Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 };
    entry[18] = Instruction::AddImmediate { d: 23, a: 4, immediate: 0 };
}
