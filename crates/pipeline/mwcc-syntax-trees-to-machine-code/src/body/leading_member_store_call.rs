//! Prologue scheduling for a leading member initialization followed by a call.
//!
//! In `object->field = 0; object->method(id, 0);`, GC/2.6 forms the call's
//! receiver and first explicit argument in the LR-save latency slots, then
//! issues the independent zero/store work before the call. The complete
//! prologue/body window is required so ordinary stores and calls keep their
//! source-order lowering.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_leading_member_store_call(&mut self) -> bool {
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
}
