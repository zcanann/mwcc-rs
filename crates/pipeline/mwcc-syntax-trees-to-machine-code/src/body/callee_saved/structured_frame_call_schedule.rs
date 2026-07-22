//! Call-argument scheduling after a frame-array byte store.
//!
//! Dense saved-home frames expose MWCC's argument-order heuristic: the first
//! argument fills the byte-store latency slot, later register forwards issue
//! next, and the two computed/address arguments finish immediately before the
//! call. This pass recognizes that dependency-complete window and permutes it
//! without changing instruction or relocation counts.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_frame_store_call(&mut self) {
        let Some(store) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreByte { a: 1, .. })
        }) else {
            return;
        };
        let Some(call) = self.output.instructions[store + 1..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|offset| store + 1 + offset)
        else {
            return;
        };
        let setup = &self.output.instructions[store + 1..call];
        let mut by_destination = std::collections::HashMap::new();
        for instruction in setup {
            let Some(destination) = defined_general(instruction) else {
                return;
            };
            if by_destination
                .insert(destination, instruction.clone())
                .is_some()
            {
                return;
            }
        }
        if !(3..=8).all(|destination| by_destination.contains_key(&destination))
            || by_destination.len() != 6
        {
            return;
        }

        let store_instruction = self.output.instructions[store].clone();
        let mut scheduled = Vec::with_capacity(7);
        let order: &[u8] = match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                scheduled.push(by_destination.remove(&3).expect("gated"));
                scheduled.push(store_instruction);
                &[6, 7, 8, 4, 5]
            }
            FrameConvention::LinkageFirst => {
                scheduled.push(store_instruction);
                &[3, 5, 6, 7, 8, 4]
            }
        };
        for &destination in order {
            scheduled.push(by_destination.remove(&destination).expect("gated"));
        }
        self.output.instructions.splice(store..call, scheduled);
    }

    /// Linkage-first MWCC spells saved-register forwards into call argument
    /// registers as `addi ..., 0`; the newer allocator uses the `mr` alias.
    /// Keep that generation-specific materialization local to dense frames.
    pub(super) fn normalize_structured_frame_argument_copies(&mut self, first_saved: u8) {
        for instruction in &mut self.output.instructions {
            let Instruction::Or { a, s, b } = instruction else {
                continue;
            };
            if *s == *b && (3..=8).contains(a) && *s >= first_saved {
                *instruction = Instruction::AddImmediate {
                    d: *a,
                    a: *s,
                    immediate: 0,
                };
            }
        }
    }
}

fn defined_general(instruction: &Instruction) -> Option<u8> {
    mwcc_vreg::register_operands(instruction)
        .into_iter()
        .find(|operand| {
            operand.role == mwcc_vreg::RegisterRole::Define
                && operand.class == mwcc_vreg::Class::General
        })
        .map(|operand| operand.register)
}
