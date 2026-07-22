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
        let Some(store) = self
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::StoreByte { a: 1, .. }))
        else {
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

    /// Linkage-first MWCC spells both saved-register forwards and values parked
    /// in saved homes as `addi ..., 0`; the newer allocator uses the `mr` alias.
    /// Keep that generation-specific materialization local to dense frames.
    /// A copy at the start of a conditional arm is control-flow forwarding,
    /// rather than straight-line materialization, and retains the `mr` form.
    pub(super) fn normalize_structured_frame_argument_copies(&mut self, first_saved: u8) {
        for index in 0..self.output.instructions.len() {
            if index > 0
                && matches!(
                    self.output.instructions[index - 1],
                    Instruction::BranchConditionalForward { .. }
                )
            {
                continue;
            }
            let Instruction::Or { a, s, b } = self.output.instructions[index] else {
                continue;
            };
            let saved_to_argument = (3..=8).contains(&a) && s >= first_saved;
            let value_to_saved_home = a >= first_saved;
            if s == b && (saved_to_argument || value_to_saved_home) {
                self.output.instructions[index] = Instruction::AddImmediate {
                    d: a,
                    a: s,
                    immediate: 0,
                };
            }
        }

        self.schedule_biased_scaled_member_call();
        self.schedule_shifted_member_mask_call();

        // When a saved value and a frame address are the final independent
        // arguments of a dense-frame call, build 163 forwards the saved value
        // first. Selection discovers the frame address first from source order;
        // rotate only this dependency-free adjacent pair.
        let mut index = 0;
        while index + 1 < self.output.instructions.len() {
            let frame_address = matches!(
                self.output.instructions[index],
                Instruction::AddImmediate { d: 3..=8, a: 1, .. }
            );
            let saved_forward = matches!(
                self.output.instructions[index + 1],
                Instruction::AddImmediate {
                    d: 3..=8,
                    a,
                    immediate: 0
                } if a >= first_saved
            );
            if frame_address && saved_forward {
                self.output.instructions.swap(index, index + 1);
                index += 2;
            } else {
                index += 1;
            }
        }
    }

    /// A member load feeding the first argument of a call is independent of the
    /// biased scaled sum feeding its second argument. Build 163 issues both
    /// member loads together, hiding the second load's latency behind the sum.
    fn schedule_biased_scaled_member_call(&mut self) {
        let mut call = 7;
        while call < self.output.instructions.len() {
            if !matches!(
                self.output.instructions[call],
                Instruction::BranchAndLink { .. }
            ) {
                call += 1;
                continue;
            }
            let start = call - 7;
            let (
                Instruction::LoadWord {
                    d: 0,
                    a: member_base,
                    ..
                },
                Instruction::Add { d: sum, b: 0, .. },
                Instruction::AddImmediate {
                    d: 0,
                    a: biased_sum,
                    ..
                },
                Instruction::ShiftLeftImmediate {
                    a: scaled, s: 0, ..
                },
                Instruction::AddImmediate {
                    d: tailed,
                    a: tailed_source,
                    ..
                },
                Instruction::LoadWord {
                    d: 3,
                    a: argument_base,
                    ..
                },
                copy,
                Instruction::BranchAndLink { .. },
            ) = (
                &self.output.instructions[start],
                &self.output.instructions[start + 1],
                &self.output.instructions[start + 2],
                &self.output.instructions[start + 3],
                &self.output.instructions[start + 4],
                &self.output.instructions[start + 5],
                &self.output.instructions[start + 6],
                &self.output.instructions[call],
            )
            else {
                call += 1;
                continue;
            };
            let copy_matches = matches!(copy,
                Instruction::Or { a: 4, s, b } if s == sum && b == sum)
                || matches!(copy,
                    Instruction::AddImmediate { d: 4, a, immediate: 0 } if a == sum);
            if member_base != argument_base
                || sum != biased_sum
                || sum != scaled
                || sum != tailed
                || sum != tailed_source
                || !copy_matches
            {
                call += 1;
                continue;
            }

            let state_load = self.output.instructions.remove(start + 5);
            self.output.instructions.insert(start + 1, state_load);
            call += 1;
        }
    }

    /// Preserve an endangered later call argument in the independent issue slot
    /// between a shift and the XOR that consumes it. This is the measured CARD
    /// response schedule: `lwz; slwi; addi arg; xor; clrrwi`.
    fn schedule_shifted_member_mask_call(&mut self) {
        let mut index = 0;
        while index + 4 < self.output.instructions.len() {
            let shifted = match &self.output.instructions[index + 1] {
                Instruction::ShiftLeftImmediate { a, .. } => *a,
                _ => {
                    index += 1;
                    continue;
                }
            };
            let matches = matches!(self.output.instructions[index], Instruction::LoadWord { d: 0, .. })
                && matches!(self.output.instructions[index + 2], Instruction::Xor { a: 0, s, b: 0 } if s == shifted)
                && matches!(self.output.instructions[index + 3], Instruction::AndContiguousMask { a, s: 0, .. } if a == shifted)
                && matches!(self.output.instructions[index + 4], Instruction::AddImmediate { d: 6, a: 3, immediate: 0 });
            if matches {
                let argument = self.output.instructions.remove(index + 4);
                self.output.instructions.insert(index + 2, argument);
                index += 5;
            } else {
                index += 1;
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
