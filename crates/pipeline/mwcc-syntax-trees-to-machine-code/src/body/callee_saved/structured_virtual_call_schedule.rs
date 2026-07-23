//! Virtual-call argument scheduling in structured state bodies.
//!
//! The generic call emitter forms `r3` before loading its vptr. GC/2.6 retains
//! the saved receiver as the dispatch source and uses that independent load to
//! fill the two-immediate argument setup latency. Bodies with two such calls
//! also use the corresponding LR-first epilogue order.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_virtual_calls(&mut self) {
        let deleting_calls = self.schedule_structured_virtual_deletes();
        if deleting_calls >= 2 {
            self.epilogue_lr_before_gprs = true;
        }
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return;
        }
        let starts: Vec<usize> = self
            .output
            .instructions
            .windows(8)
            .enumerate()
            .filter_map(|(start, window)| is_schedulable_virtual_call(window).then_some(start))
            .collect();
        for start in &starts {
            let receiver = match self.output.instructions[*start] {
                Instruction::Or { s, .. } => s,
                _ => unreachable!(),
            };
            match &mut self.output.instructions[*start + 4] {
                Instruction::LoadWord { a, .. } => *a = receiver,
                _ => unreachable!(),
            }
            self.move_structured_virtual_instruction_before(*start + 4, *start + 2);
        }
        if starts.len() >= 2 {
            self.epilogue_lr_before_gprs = true;
        }
    }

    /// Reuse the pointer loaded by a scalar-delete null test as the virtual
    /// receiver. The ordinary expression path conservatively loads the member
    /// once for the condition and once for the call; MWCC keeps it in r3 and
    /// fills the vptr-load latency with the deleting flag.
    fn schedule_structured_virtual_deletes(&mut self) -> usize {
        let mut start = 0;
        let mut count = 0;
        while start + 9 <= self.output.instructions.len() {
            if !is_unscheduled_virtual_delete(&self.output.instructions[start..start + 9]) {
                start += 1;
                continue;
            }
            let (base, offset) = match self.output.instructions[start] {
                Instruction::LoadWord { a, offset, .. } => (a, offset),
                _ => unreachable!(),
            };
            self.output.instructions[start] = Instruction::LoadWord {
                d: 3,
                a: base,
                offset,
            };
            self.output.instructions[start + 1] =
                Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 };
            self.remove_structured_virtual_instruction(start + 4);
            self.move_structured_virtual_instruction_before(start + 4, start + 3);
            count += 1;
            start += 8;
        }
        count
    }

    fn remove_structured_virtual_instruction(&mut self, at: usize) {
        self.output.instructions.remove(at);
        self.labels.removed(at, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != at);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > at {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > at =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }

    fn move_structured_virtual_instruction_before(&mut self, from: usize, to: usize) {
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

fn is_unscheduled_virtual_delete(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: 0, a: condition_base, offset: condition_offset },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
        Instruction::LoadWord { d: 3, a: call_base, offset: call_offset },
        Instruction::LoadWord { d: 12, a: 3, offset: 0 },
        Instruction::LoadWord { d: 12, a: 12, offset: 8 },
        Instruction::MoveToCountRegister { s: 12 },
        Instruction::BranchToCountRegisterAndLink,
    ] if condition_base == call_base && condition_offset == call_offset)
}

fn is_schedulable_virtual_call(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::Or { a: 3, s: receiver, b },
        Instruction::Or { a: 4, .. },
        Instruction::AddImmediate { d: 5, a: 0, .. },
        Instruction::AddImmediate { d: 6, a: 0, .. },
        Instruction::LoadWord { d: 12, a: 3, offset: 0 },
        Instruction::LoadWord { d: 12, a: 12, .. },
        Instruction::MoveToCountRegister { s: 12 },
        Instruction::BranchToCountRegisterAndLink,
    ] if receiver == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_four_argument_virtual_setup() {
        let instructions = vec![
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::Or { a: 4, s: 30, b: 30 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 3 },
            Instruction::AddImmediate { d: 6, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 12, a: 3, offset: 0 },
            Instruction::LoadWord { d: 12, a: 12, offset: 28 },
            Instruction::MoveToCountRegister { s: 12 },
            Instruction::BranchToCountRegisterAndLink,
        ];
        assert!(is_schedulable_virtual_call(&instructions));
    }

    #[test]
    fn recognizes_reloaded_scalar_delete_receiver() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 31, offset: 12 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 9,
            },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 1 },
            Instruction::LoadWord { d: 3, a: 31, offset: 12 },
            Instruction::LoadWord { d: 12, a: 3, offset: 0 },
            Instruction::LoadWord { d: 12, a: 12, offset: 8 },
            Instruction::MoveToCountRegister { s: 12 },
            Instruction::BranchToCountRegisterAndLink,
        ];
        assert!(is_unscheduled_virtual_delete(&instructions));
    }
}
