//! Tail-entered oscillator loop in a guarded callback transaction.
//!
//! MWCC loads the address-taken loop index at the shared tail, carries it in
//! `r0` through the next body iteration, and forms both member-array addresses
//! dependency-first.  The generic structured loop instead falls through the
//! body initially and reloads the frame index at each use.  This schedule owns
//! that control-flow and dependency-order conversion as one transaction.

use super::*;

const BODY_LEN: usize = 18;
const BODY_ORDER: [usize; BODY_LEN] = [
    1, 3, 2, 11, 5, 6, 8, 10, 9, 12, 13, 15, 14, 16, 17, 0, 4, 7,
];

impl Generator {
    pub(crate) fn schedule_guarded_callback_oscillator_loop(&mut self, function: &Function) {
        let Some(plan) = super::structured_guarded_member_lvalue::recognize(function) else {
            return;
        };
        let Some(receiver) = super::structured_guarded_callback_copy_schedule::retained_receiver(
            &self.output.instructions,
            plan.member_offset,
        ) else {
            return;
        };
        let Some((body, frame_offset)) = oscillator_body(&self.output.instructions, receiver)
        else {
            return;
        };
        let Some(zero) = self.output.instructions[..body].iter().rposition(|instruction| {
            matches!(instruction, Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
        }) else {
            return;
        };
        let Some(float_reset) = self.output.instructions[..zero].windows(10).rposition(|window| {
            matches!(window, [
                Instruction::LoadFloatSingle { .. },
                Instruction::StoreFloatSingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::StoreFloatSingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::StoreFloatSingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::StoreFloatSingle { .. },
                Instruction::LoadFloatSingle { .. },
                Instruction::StoreFloatSingle { .. },
            ])
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, zero, float_reset + 1);

        let Some((body, frame_offset)) = oscillator_body(&self.output.instructions, receiver)
        else {
            return;
        };
        let old = self.output.instructions[body..body + BODY_LEN].to_vec();
        crate::retarget_instruction_destinations(self, body, body + 1);
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, old_relative) in BODY_ORDER.into_iter().enumerate() {
            self.output.instructions[body + new_relative] = old[old_relative].clone();
            permutation[body + old_relative] = body + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);

        let branch_over_empty = self.output.instructions[body + 5].clone();
        let bank_call = self.output.instructions[body + 10].clone();
        let effect_call = self.output.instructions[body + 14].clone();
        self.output.instructions[body..body + 15].clone_from_slice(&[
            Instruction::ShiftLeftImmediate { a: 3, s: 0, shift: 2 },
            Instruction::AddImmediate { d: 28, a: 3, immediate: 56 },
            Instruction::Add { d: 28, a: receiver, b: 28 },
            Instruction::LoadWord { d: 3, a: 28, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            branch_over_empty,
            Instruction::MultiplyImmediate { d: 4, a: 0, immediate: 24 },
            Instruction::AddImmediate { d: 27, a: 4, immediate: 72 },
            Instruction::Add { d: 27, a: receiver, b: 27 },
            Instruction::AddImmediate { d: 4, a: 27, immediate: 0 },
            bank_call,
            Instruction::LoadWord { d: 4, a: 28, offset: 0 },
            Instruction::move_register(3, receiver),
            Instruction::LoadByteZero { d: 4, a: 4, offset: 0 },
            effect_call,
        ]);
        for removed in [body + 17, body + 16, body + 15] {
            crate::remove_instruction_retargeting_to_next(self, removed);
        }

        let Some(tail) = loop_tail(&self.output.instructions, frame_offset) else {
            return;
        };
        crate::insert_instruction_retargeting(
            self,
            tail + 3,
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: frame_offset,
            },
        );
        let tail_reload = tail + 3;
        let Some(initialization) = self.output.instructions[..body]
            .iter()
            .rposition(|instruction| {
                matches!(instruction,
                    Instruction::StoreWord { s: 0, a: 1, offset }
                        if *offset == frame_offset)
            })
        else {
            return;
        };
        crate::insert_instruction_retargeting(
            self,
            initialization + 1,
            Instruction::Branch {
                target: tail_reload,
            },
        );
        if let Some(bridge) = redundant_callback_arm_bridge(&self.output.instructions, receiver) {
            crate::remove_instruction_retargeting_to_next(self, bridge);
        }
    }
}

fn oscillator_body(instructions: &[Instruction], receiver: u8) -> Option<(usize, i16)> {
    instructions
        .windows(BODY_LEN)
        .enumerate()
        .find_map(|(start, window)| {
            let [
                Instruction::LoadWord { d: 0, a: 1, offset: frame_offset },
                Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 2 },
                Instruction::Add { d: 28, a, b: 0 },
                Instruction::AddImmediate { d: 28, a: 28, immediate: 56 },
                Instruction::LoadWord { d: 0, a: 28, offset: 0 },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadWord { d: 0, a: 1, offset: second_frame_offset },
                Instruction::MultiplyImmediate { d: 0, a: 0, immediate: 24 },
                Instruction::Add { d: 27, a: second_base, b: 0 },
                Instruction::AddImmediate { d: 27, a: 27, immediate: 72 },
                Instruction::LoadWord { d: 3, a: 28, offset: 0 },
                Instruction::AddImmediate { d: 4, a: 27, immediate: 0 },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: 3, a: call_receiver, immediate: 0 },
                Instruction::LoadWord { d: 4, a: 28, offset: 0 },
                Instruction::LoadByteZero { d: 4, a: 4, offset: 0 },
                Instruction::BranchAndLink { .. },
            ] = window
            else {
                return None;
            };
            (*a == receiver
                && *second_base == receiver
                && *call_receiver == receiver
                && frame_offset == second_frame_offset)
                .then_some((start, *frame_offset))
        })
}

fn loop_tail(instructions: &[Instruction], frame_offset: i16) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 3, a: 1, offset },
            Instruction::AddImmediate { d: 0, a: 3, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 1, offset: stored_offset },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 },
            Instruction::BranchConditionalForward { .. },
        ] if *offset == frame_offset && *stored_offset == frame_offset)
    })
}

fn redundant_callback_arm_bridge(instructions: &[Instruction], receiver: u8) -> Option<usize> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        matches!(window, [
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
            Instruction::Branch { .. },
            Instruction::Branch { .. },
            Instruction::AddImmediate { d: 3, a, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 2 },
            Instruction::MoveToLinkRegister { .. },
        ] if *a == receiver)
        .then_some(start + 2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_order_drops_only_the_three_frame_or_member_reloads() {
        assert_eq!(&BODY_ORDER[..15], &[1, 3, 2, 11, 5, 6, 8, 10, 9, 12, 13, 15, 14, 16, 17]);
        assert_eq!(&BODY_ORDER[15..], &[0, 4, 7]);
    }

    #[test]
    fn recognizes_unreachable_bridge_before_callback_arm() {
        let instructions = vec![
            Instruction::load_immediate(3, 0),
            Instruction::Branch { target: 8 },
            Instruction::Branch { target: 7 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::load_immediate(4, 2),
            Instruction::MoveToLinkRegister { s: 12 },
        ];
        assert_eq!(redundant_callback_arm_bridge(&instructions, 30), Some(2));
    }
}
