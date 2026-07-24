//! Shared-literal scheduling for structured member scaling.
//!
//! GC/2.6 recognizes two adjacent `member *= same_float` statements as one
//! value graph: it loads the literal once, overlaps both member loads, performs
//! both products, then stores both results. This owner requires the complete
//! two-store/call region before changing instruction count or register roles.

#[allow(unused_imports)]
use super::*;

const SCALE_SCHEDULE: [usize; 9] = [1, 7, 0, 4, 2, 5, 3, 6, 8];

impl Generator {
    pub(super) fn schedule_structured_member_scales_and_compare(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return;
        }
        self.schedule_structured_member_scale_pair();
        self.schedule_structured_member_compare_loads();
    }

    fn schedule_structured_member_scale_pair(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(10)
            .enumerate()
            .find_map(|(start, window)| {
                (is_member_scale_pair(window)
                    && schedule_relocations::same_relocated_value(
                        &self.output.relocations,
                        &self.output.constants,
                        start,
                        start + 4,
                    ))
                .then_some(start)
            })
        else {
            return;
        };

        self.remove_structured_scale_instruction(start + 4);
        let mut current: Vec<usize> = (0..9).collect();
        for (destination, &original) in SCALE_SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("member scale schedule is a permutation");
            if source != destination {
                self.move_structured_scale_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        assign_member_scale_registers(&mut self.output.instructions[start..start + 9]);
    }

    fn schedule_structured_member_compare_loads(&mut self) {
        let Some(start) = self.output.instructions.windows(3).enumerate().find_map(
            |(start, window)| {
                matches!(window, [
                    Instruction::LoadFloatSingle { d: literal, a: 0, .. },
                    Instruction::LoadFloatSingle { d: member, a, .. },
                    Instruction::FloatCompareOrdered { a: compared, b },
                ] if *a != 0 && *compared == *member && *b == *literal)
                .then_some(start)
                .filter(|start| {
                    self.output.relocations.iter().any(|relocation| relocation.instruction_index == *start)
                        && !self.output.relocations.iter().any(|relocation| relocation.instruction_index == *start + 1)
                })
            },
        ) else {
            return;
        };
        self.move_structured_scale_instruction_before(start + 1, start);
    }

    fn remove_structured_scale_instruction(&mut self, at: usize) {
        debug_assert!(!self.output.instructions.iter().any(|instruction| matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target }
                if *target == at
        )));
        self.output.instructions.remove(at);
        self.labels.removed(at, 1);
        self.output.relocations.retain(|relocation| relocation.instruction_index != at);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > at {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } if *target > at => *target -= 1,
                _ => {}
            }
        }
    }

    fn move_structured_scale_instruction_before(&mut self, from: usize, to: usize) {
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


fn is_member_scale_pair(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: literal_0, a: 0, .. },
        Instruction::LoadFloatSingle { d: value_0, a: base_0, offset: offset_0 },
        Instruction::FloatMultiplySingle { d: product_0, a: factor_0a, c: factor_0c },
        Instruction::StoreFloatSingle { s: stored_0, a: store_base_0, offset: store_offset_0 },
        Instruction::LoadFloatSingle { d: literal_1, a: 0, .. },
        Instruction::LoadFloatSingle { d: value_1, a: base_1, offset: offset_1 },
        Instruction::FloatMultiplySingle { d: product_1, a: factor_1a, c: factor_1c },
        Instruction::StoreFloatSingle { s: stored_1, a: store_base_1, offset: store_offset_1 },
        Instruction::Or { a: 3, s: receiver, b },
        Instruction::BranchAndLink { .. },
    ] if product_0 == literal_0 && stored_0 == product_0
        && ((*factor_0a == *literal_0 && *factor_0c == *value_0)
            || (*factor_0c == *literal_0 && *factor_0a == *value_0))
        && product_1 == literal_1 && stored_1 == product_1
        && ((*factor_1a == *literal_1 && *factor_1c == *value_1)
            || (*factor_1c == *literal_1 && *factor_1a == *value_1))
        && base_0 == base_1 && base_1 == store_base_0 && store_base_0 == store_base_1
        && offset_0 == store_offset_0 && offset_1 == store_offset_1 && offset_0 != offset_1
        && receiver == base_0 && b == receiver)
}

fn assign_member_scale_registers(window: &mut [Instruction]) {
    match &mut window[0] {
        Instruction::LoadFloatSingle { d, .. } => *d = 2,
        _ => unreachable!(),
    }
    match &mut window[2] {
        Instruction::LoadFloatSingle { d, .. } => *d = 0,
        _ => unreachable!(),
    }
    match &mut window[3] {
        Instruction::LoadFloatSingle { d, .. } => *d = 3,
        _ => unreachable!(),
    }
    window[4] = Instruction::FloatMultiplySingle { d: 2, a: 2, c: 0 };
    window[5] = Instruction::FloatMultiplySingle { d: 3, a: 3, c: 0 };
    match &mut window[6] {
        Instruction::StoreFloatSingle { s, .. } => *s = 2,
        _ => unreachable!(),
    }
    match &mut window[7] {
        Instruction::StoreFloatSingle { s, .. } => *s = 3,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_distinct_virtuals_for_equal_scale_literals() {
        let instructions = vec![
            Instruction::LoadFloatSingle { d: 34, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 32, offset: 468 },
            Instruction::FloatMultiplySingle { d: 34, a: 34, c: 0 },
            Instruction::StoreFloatSingle { s: 34, a: 32, offset: 468 },
            Instruction::LoadFloatSingle { d: 35, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 32, offset: 476 },
            Instruction::FloatMultiplySingle { d: 35, a: 35, c: 0 },
            Instruction::StoreFloatSingle { s: 35, a: 32, offset: 476 },
            Instruction::Or { a: 3, s: 32, b: 32 },
            Instruction::BranchAndLink { target: "finish".to_string() },
        ];
        assert!(is_member_scale_pair(&instructions));
    }
}
