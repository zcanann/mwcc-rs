//! Prologue scheduling for a leading member initialization followed by a call.
//!
//! In `object->field = 0; object->method(id, 0);`, GC/2.6 forms the call's
//! receiver and first explicit argument in the LR-save latency slots, then
//! issues the independent zero/store work before the call. Build 163 applies
//! the same principle to `object->field = value; value->method(object)`: it
//! saves the cyclic argument source in the first linkage slot before performing
//! the member store and the two-register swap. Complete prologue/body windows
//! keep these schedules out of ordinary store and call lowering.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_leading_member_store_call(&mut self) -> bool {
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            let Some(start) = self
                .output
                .instructions
                .windows(8)
                .position(is_linkage_first_member_store_swapped_call)
            else {
                return false;
            };
            self.move_member_store_call_instruction_before(start + 4, start + 1);
            return true;
        }
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return false;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(9)
            .position(is_leading_member_store_call)
        else {
            return false;
        };

        // Original: frame, LR, LR-save, zero, store, receiver, id, null, call.
        // Scheduled: frame, LR, receiver, id, LR-save, zero, null, store, call.
        let schedule = [0usize, 1, 5, 6, 2, 3, 7, 4, 8];
        let mut current: Vec<usize> = (0..9).collect();
        for (destination, &original) in schedule.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("member-store call schedule is a permutation");
            if source != destination {
                self.move_member_store_call_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        match &mut self.output.instructions[start + 7] {
            Instruction::StoreWord { a, .. } => *a = 3,
            _ => unreachable!(),
        }
        true
    }

    fn move_member_store_call_instruction_before(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == from {
                to
            } else if (to..from).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
    }
}

fn is_linkage_first_member_store_swapped_call(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, .. },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
        Instruction::StoreWord { s: 4, a: 3, .. },
        Instruction::Or { a: 5, s: 3, b: 3 },
        Instruction::AddImmediate { d: 3, a: 4, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: 5, immediate: 0 },
        Instruction::BranchAndLink { .. },
    ])
}

fn is_leading_member_store_call(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, .. },
        Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
        Instruction::StoreWord { s: 0, a: store_base, .. },
        Instruction::Or { a: 3, s: receiver, b },
        Instruction::AddImmediate { d: 4, a: 0, .. },
        Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
        Instruction::BranchAndLink { .. },
    ] if store_base == receiver && b == receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_null_member_initialization_before_call() {
        let instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 4, offset: 560 },
            Instruction::Or { a: 3, s: 4, b: 4 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 2 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
            Instruction::BranchAndLink { target: "start".to_string() },
        ];
        assert!(is_leading_member_store_call(&instructions));
    }

    #[test]
    fn recognizes_a_member_store_before_a_cyclic_argument_swap() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
            Instruction::StoreWord { s: 4, a: 3, offset: 0 },
            Instruction::Or { a: 5, s: 3, b: 3 },
            Instruction::AddImmediate { d: 3, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 5, immediate: 0 },
            Instruction::BranchAndLink { target: "addClient".to_string() },
        ];
        assert!(is_linkage_first_member_store_swapped_call(&instructions));
    }
}
