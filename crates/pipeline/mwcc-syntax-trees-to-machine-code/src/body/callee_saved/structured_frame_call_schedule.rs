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
    pub(super) fn normalize_structured_frame_argument_copies(
        &mut self,
        first_saved: u8,
        logical_call_result_homes: &[u8],
        recycled_call_result_homes: &[u8],
    ) {
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
            // An initial definition that remains live beside a later version
            // is a value-preserving call-result copy, not a legacy
            // materialization. Its home therefore retains the logical `mr`
            // encoding even in an otherwise addi-normalized dense frame.
            let value_to_saved_home = a >= first_saved && !logical_call_result_homes.contains(&a);
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
        self.schedule_call_result_member_mask_call(first_saved);
        self.schedule_recycled_call_result_argument(recycled_call_result_homes);

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

    /// A call result written into an already-planned local home is ready before
    /// the following call's ordinary argument forwards. Build 163 issues that
    /// recycled value first, after its defining call-result copy, to extend the
    /// useful latency window. Move it only across dependency-free, relocation-
    /// free argument materializations.
    fn schedule_recycled_call_result_argument(&mut self, recycled_homes: &[u8]) {
        if recycled_homes.is_empty() {
            return;
        }
        let mut call = 0;
        while call < self.output.instructions.len() {
            if !matches!(
                self.output.instructions[call],
                Instruction::BranchAndLink { .. }
            ) {
                call += 1;
                continue;
            }
            let Some((from, to)) = recycled_result_argument_move(
                &self.output.instructions,
                call,
                recycled_homes,
                &self.output.relocations,
            ) else {
                call += 1;
                continue;
            };
            let instruction = self.output.instructions.remove(from);
            self.output.instructions.insert(to, instruction);
            call += 1;
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

    /// A call result saved for the sixth argument leaves an independent member
    /// load and argument forward behind it. Legacy MWCC fills the result-copy
    /// latency with that load, forwards the result, then consumes the load in
    /// the XOR/mask chain.
    fn schedule_call_result_member_mask_call(&mut self, first_saved: u8) {
        let mut start = 0;
        while start + 10 < self.output.instructions.len() {
            if !is_call_result_member_mask_window(
                &self.output.instructions[start..start + 11],
                first_saved,
            ) {
                start += 1;
                continue;
            }
            let window: Vec<_> = self.output.instructions[start..start + 11].to_vec();
            let order = [0, 2, 1, 8, 3, 4, 6, 5, 7, 9, 10];
            for (destination, source) in order.into_iter().enumerate() {
                self.output.instructions[start + destination] = window[source].clone();
            }
            start += 11;
        }
    }
}

fn recycled_result_argument_move(
    instructions: &[Instruction],
    call: usize,
    recycled_homes: &[u8],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<(usize, usize)> {
    let start = instructions[..call]
        .iter()
        .rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .map_or(0, |previous_call| previous_call + 1);
    let candidates: Vec<(usize, u8, u8)> = instructions[start..call]
        .iter()
        .enumerate()
        .filter_map(|(offset, instruction)| match instruction {
            Instruction::AddImmediate {
                d: destination @ 3..=8,
                a: source,
                immediate: 0,
            } if recycled_homes.contains(source) => Some((start + offset, *destination, *source)),
            _ => None,
        })
        .collect();
    let [(from, destination, source)] = candidates.as_slice() else {
        return None;
    };
    let mut to = *from;
    while to > start {
        let previous = to - 1;
        if relocations
            .iter()
            .any(|relocation| relocation.instruction_index == previous)
        {
            break;
        }
        let operands = mwcc_vreg::register_operands(&instructions[previous]);
        if operands.iter().any(|operand| {
            operand.class == mwcc_vreg::Class::General
                && (operand.register == *source || operand.register == *destination)
        }) {
            break;
        }
        if !matches!(defined_general(&instructions[previous]), Some(3..=8)) {
            break;
        }
        to = previous;
    }
    (to < *from).then_some((*from, to))
}

fn is_call_result_member_mask_window(instructions: &[Instruction], first_saved: u8) -> bool {
    let [
        Instruction::BranchAndLink { .. },
        Instruction::AddImmediate {
            d: result_home,
            a: 3,
            immediate: 0,
        },
        Instruction::LoadWord {
            d: 0,
            a: member_base,
            ..
        },
        Instruction::Xor {
            a: 0,
            s: mask_source,
            b: 0,
        },
        Instruction::AndContiguousMask {
            a: masked_value,
            s: 0,
            ..
        },
        Instruction::AddImmediate {
            d: 3,
            a: first_argument,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: masked_argument,
            immediate: 0,
        },
        Instruction::AddImmediate { d: 5, a: 1, .. },
        Instruction::AddImmediate {
            d: 6,
            a: result_argument,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 7,
            a: 0,
            immediate: 1,
        },
        Instruction::BranchAndLink { .. },
    ] = instructions
    else {
        return false;
    };
    *result_home >= first_saved
        && result_home == result_argument
        && masked_value == masked_argument
        && *member_base >= first_saved
        && *mask_source >= first_saved
        && *first_argument >= first_saved
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hoists_a_recycled_result_after_its_definition() {
        let instructions = vec![
            Instruction::BranchAndLink {
                target: "produce".into(),
            },
            Instruction::AddImmediate {
                d: 40,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 41,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 42,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 40,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];
        assert_eq!(
            recycled_result_argument_move(&instructions, 6, &[40], &[]),
            Some((4, 2)),
        );
    }

    #[test]
    fn recognizes_a_saved_result_and_member_mask_call_window() {
        let instructions = vec![
            Instruction::BranchAndLink { target: "length".into() },
            Instruction::AddImmediate { d: 28, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 0, a: 31, offset: 40 },
            Instruction::Xor { a: 0, s: 30, b: 0 },
            Instruction::AndContiguousMask { a: 40, s: 0, begin: 0, end: 15 },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 40, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 1, immediate: 32 },
            Instruction::AddImmediate { d: 6, a: 28, immediate: 0 },
            Instruction::AddImmediate { d: 7, a: 0, immediate: 1 },
            Instruction::BranchAndLink { target: "read".into() },
        ];
        assert!(is_call_result_member_mask_window(&instructions, 28));

        let mut wrong_argument = instructions;
        wrong_argument[8] = Instruction::AddImmediate { d: 6, a: 27, immediate: 0 };
        assert!(!is_call_result_member_mask_window(&wrong_argument, 28));
    }
}
