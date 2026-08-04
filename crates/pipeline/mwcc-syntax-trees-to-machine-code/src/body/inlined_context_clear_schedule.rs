//! Final schedule for a saved interrupt state followed by an inlined context clear.
//!
//! `OSGetCurrentContext` is an inline fixed-bank read. Build 163 first loads it
//! through volatile r0 and then publishes it to the saved home, preserving the
//! same selection boundary as the non-inlined call result. Directly selecting
//! the saved home loses that copy and changes every later branch displacement.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InlinedContextClearPlan {
    start: usize,
    saved_interrupt_state: u8,
    saved_current_context: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TailContextRestorePlan {
    start: usize,
    saved_interrupt_state: u8,
    saved_current_context: u8,
}

impl Generator {
    pub(crate) fn schedule_inlined_context_clear_transaction(&mut self) {
        if let Some(plan) = inlined_context_clear_plan(&self.output.instructions) {
            self.schedule_leading_context_clear(plan);
        }
        if let Some(plan) = tail_context_restore_plan(&self.output.instructions) {
            basic_block_schedule::permute_contents(
                &mut self.output,
                plan.start,
                [0, 1, 3, 4, 2, 5, 6, 7, 8, 9, 10, 11, 12],
            );
            self.output.instructions[plan.start + 9] =
                Instruction::move_register(3, plan.saved_current_context);
            self.output.instructions[plan.start + 11] =
                Instruction::move_register(3, plan.saved_interrupt_state);
        }
    }

    fn schedule_leading_context_clear(&mut self, plan: InlinedContextClearPlan) {

        // Split the fixed-bank load from its saved home before permuting. The
        // insertion helper keeps later calls, labels, jump-table entries, and
        // instruction-owned patches synchronized.
        crate::insert_instruction_retargeting(
            self,
            plan.start + 4,
            Instruction::move_register(plan.saved_current_context, GENERAL_SCRATCH),
        );

        // Identities after insertion, excluding the leading disable call:
        // saved-state, bank-base, current-load, new-copy, zero, first-store,
        // second-store, frame-address, guard-load, compare, branch, store,
        // call-address, set-current call.
        let mut identities = vec![0, 1, 2, 13, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        const DESIRED: [usize; 14] = [1, 2, 3, 6, 4, 13, 0, 5, 7, 8, 9, 10, 11, 12];
        for (to, desired) in DESIRED.into_iter().enumerate() {
            let from = identities
                .iter()
                .position(|identity| *identity == desired)
                .expect("the inlined context-clear permutation is complete");
            if from != to {
                crate::move_instruction_before_retargeting(
                    self,
                    plan.start + 1 + from,
                    plan.start + 1 + to,
                );
                let identity = identities.remove(from);
                identities.insert(to, identity);
            }
        }

        const FIXED_BANK: u8 = 6;
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[plan.start + 1]
        else {
            unreachable!("the fixed-bank base was matched")
        };
        *d = FIXED_BANK;
        let Instruction::LoadWord { d, a, .. } =
            &mut self.output.instructions[plan.start + 2]
        else {
            unreachable!("the current-context load was matched")
        };
        *d = GENERAL_SCRATCH;
        *a = FIXED_BANK;
        self.output.instructions[plan.start + 6] =
            Instruction::move_register(plan.saved_current_context, GENERAL_SCRATCH);
        self.output.instructions[plan.start + 7] =
            Instruction::move_register(plan.saved_interrupt_state, 3);
        for index in [plan.start + 9, plan.start + 12] {
            match &mut self.output.instructions[index] {
                Instruction::LoadWord { a, .. } | Instruction::StoreWord { a, .. } => {
                    *a = FIXED_BANK;
                }
                _ => unreachable!("the fixed-bank guard access was matched"),
            }
        }
    }
}

fn inlined_context_clear_plan(instructions: &[Instruction]) -> Option<InlinedContextClearPlan> {
    instructions.windows(14).enumerate().find_map(|(start, window)| {
        let [
            Instruction::BranchAndLink { target: disable },
            Instruction::AddImmediate {
                d: saved_interrupt_state,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: fixed_base,
                a: 0,
                ..
            },
            Instruction::LoadWord {
                d: saved_current_context,
                a: current_base,
                offset: current_offset,
            },
            Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreHalfword {
                s: first_zero,
                a: first_frame,
                offset: first_offset,
            },
            Instruction::StoreHalfword {
                s: second_zero,
                a: second_frame,
                offset: second_offset,
            },
            Instruction::AddImmediate {
                d: 4,
                a: address_frame,
                immediate: address_offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: guard_base,
                offset: guard_offset,
            },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: continuation,
            },
            Instruction::StoreWord {
                s: stored_zero,
                a: store_base,
                offset: store_offset,
            },
            Instruction::AddImmediate {
                d: 3,
                a: call_frame,
                immediate: call_offset,
            },
            Instruction::BranchAndLink { target: set_current },
        ] = window
        else {
            return None;
        };
        (disable == "OSDisableInterrupts"
            && set_current == "OSSetCurrentContext"
            && (14..=31).contains(saved_interrupt_state)
            && (14..=31).contains(saved_current_context)
            && saved_interrupt_state != saved_current_context
            && fixed_base == current_base
            && fixed_base == guard_base
            && fixed_base == store_base
            && *fixed_base == 3
            && guard_offset.checked_sub(*current_offset) == Some(4)
            && guard_offset == store_offset
            && zero == first_zero
            && zero == second_zero
            && zero == stored_zero
            && first_frame == second_frame
            && *first_frame == 1
            && second_offset.checked_sub(*first_offset) == Some(2)
            && *address_frame == 1
            && address_frame == call_frame
            && address_offset == call_offset
            && *continuation == start + 12)
            .then_some(InlinedContextClearPlan {
                start,
                saved_interrupt_state: *saved_interrupt_state,
                saved_current_context: *saved_current_context,
            })
    })
}

fn tail_context_restore_plan(instructions: &[Instruction]) -> Option<TailContextRestorePlan> {
    instructions.windows(13).enumerate().find_map(|(start, window)| {
        let [
            Instruction::AddImmediate {
                d: zero,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreHalfword {
                s: first_zero,
                a: first_frame,
                offset: first_offset,
            },
            Instruction::StoreHalfword {
                s: second_zero,
                a: second_frame,
                offset: second_offset,
            },
            Instruction::AddImmediateShifted {
                d: fixed_base,
                a: 0,
                ..
            },
            Instruction::AddImmediate {
                d: 4,
                a: address_frame,
                ..
            },
            Instruction::LoadWord {
                d: 0,
                a: loaded_base,
                offset: guard_offset,
            },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: continuation,
            },
            Instruction::StoreWord {
                s: stored_zero,
                a: stored_base,
                offset: stored_offset,
            },
            Instruction::AddImmediate {
                d: 3,
                a: saved_current_context,
                immediate: 0,
            },
            Instruction::BranchAndLink { target: set_current },
            Instruction::AddImmediate {
                d: 3,
                a: saved_interrupt_state,
                immediate: 0,
            },
            Instruction::BranchAndLink { target: restore_interrupts },
        ] = window
        else {
            return None;
        };
        (zero == first_zero
            && zero == second_zero
            && zero == stored_zero
            && first_frame == second_frame
            && first_frame == address_frame
            && *first_frame == 1
            && second_offset.checked_sub(*first_offset) == Some(2)
            && fixed_base == loaded_base
            && fixed_base == stored_base
            && guard_offset == stored_offset
            && *continuation == start + 9
            && (14..=31).contains(saved_current_context)
            && (14..=31).contains(saved_interrupt_state)
            && saved_current_context != saved_interrupt_state
            && set_current == "OSSetCurrentContext"
            && restore_interrupts == "OSRestoreInterrupts")
            .then_some(TailContextRestorePlan {
                start,
                saved_interrupt_state: *saved_interrupt_state,
                saved_current_context: *saved_current_context,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_saved_interrupt_state_and_inlined_context_clear() {
        let instructions = [
            Instruction::BranchAndLink { target: "OSDisableInterrupts".into() },
            Instruction::AddImmediate { d: 29, a: 3, immediate: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: -32768 },
            Instruction::LoadWord { d: 30, a: 3, offset: 212 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
            Instruction::StoreHalfword { s: 5, a: 1, offset: 432 },
            Instruction::StoreHalfword { s: 5, a: 1, offset: 434 },
            Instruction::AddImmediate { d: 4, a: 1, immediate: 16 },
            Instruction::LoadWord { d: 0, a: 3, offset: 216 },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::StoreWord { s: 5, a: 3, offset: 216 },
            Instruction::AddImmediate { d: 3, a: 1, immediate: 16 },
            Instruction::BranchAndLink { target: "OSSetCurrentContext".into() },
        ];
        assert_eq!(
            inlined_context_clear_plan(&instructions),
            Some(InlinedContextClearPlan {
                start: 0,
                saved_interrupt_state: 29,
                saved_current_context: 30,
            })
        );
    }

    #[test]
    fn recognizes_a_tail_clear_and_saved_context_restore() {
        let instructions = [
            Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
            Instruction::StoreHalfword { s: 5, a: 1, offset: 432 },
            Instruction::StoreHalfword { s: 5, a: 1, offset: 434 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: -32768 },
            Instruction::AddImmediate { d: 4, a: 1, immediate: 16 },
            Instruction::LoadWord { d: 0, a: 3, offset: 216 },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 9,
            },
            Instruction::StoreWord { s: 5, a: 3, offset: 216 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::BranchAndLink { target: "OSSetCurrentContext".into() },
            Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
            Instruction::BranchAndLink { target: "OSRestoreInterrupts".into() },
        ];
        assert_eq!(
            tail_context_restore_plan(&instructions),
            Some(TailContextRestorePlan {
                start: 0,
                saved_interrupt_state: 29,
                saved_current_context: 30,
            })
        );
    }
}
