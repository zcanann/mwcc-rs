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
}
