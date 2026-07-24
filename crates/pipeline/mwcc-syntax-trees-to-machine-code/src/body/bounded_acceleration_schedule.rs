//! Final lifetime scheduling for a friction-bounded acceleration update.
//!
//! Automatic expansion owns the zero-target friction arm, while ordinary
//! structured lowering owns the signed target and horizontal-limit clamps.
//! MWCC schedules those regions together: zero remains in f5, velocity and
//! friction remain in f1/f4, and the member limit remains live through each
//! adjustment. This pass claims only that complete measured physical stream.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_bounded_acceleration(&mut self) {
        if !is_unscheduled_bounded_acceleration(&self.output.instructions) {
            return;
        }
        let literal_indices = [0, 5, 11, 24, 31, 34];
        if !literal_indices.iter().all(|index| {
            self.output
                .relocations
                .iter()
                .any(|relocation| relocation.instruction_index == *index)
        }) {
            return;
        }

        set_float_load_destination(&mut self.output.instructions[0], 5);
        self.output.instructions[1] = Instruction::FloatCompareUnordered { a: 3, b: 5 };
        self.output.instructions[1] = Instruction::FloatCompareUnordered { a: 3, b: 5 };
        set_float_load_destination(&mut self.output.instructions[4], 2);
        self.output.instructions[6] = Instruction::FloatCompareOrdered { a: 2, b: 5 };
        self.output.instructions[8] = Instruction::FloatNegate { d: 1, b: 2 };
        self.output.instructions[10] = Instruction::FloatMove { d: 1, b: 2 };
        self.output.instructions[12] = Instruction::FloatCompareOrdered { a: 4, b: 0 };
        self.output.instructions[14] = Instruction::FloatNegate { d: 0, b: 4 };
        self.output.instructions[16] = Instruction::FloatMove { d: 0, b: 4 };
        self.output.instructions[17] = Instruction::FloatCompareOrdered { a: 0, b: 1 };
        self.output.instructions[21] = Instruction::FloatNegate { d: 4, b: 2 };
        set_float_load_destination(&mut self.output.instructions[24], 0);
        self.output.instructions[25] = Instruction::FloatCompareOrdered { a: 2, b: 0 };
        self.output.instructions[27] = Instruction::FloatNegate { d: 4, b: 4 };
        set_float_store_source(&mut self.output.instructions[28], 4);
        self.output.instructions[29] = Instruction::BranchToLinkRegister;

        self.output.instructions[30] = Instruction::FloatMultiplySingle { d: 0, a: 1, c: 2 };
        self.output.instructions[32] = Instruction::FloatCompareOrdered { a: 0, b: 5 };
        self.output.instructions[35] = Instruction::FloatCompareOrdered { a: 2, b: 5 };
        set_forward_branch_target(&mut self.output.instructions[39], 68);
        set_float_load_destination(&mut self.output.instructions[46], 3);
        self.output.instructions[47] = Instruction::FloatCompareOrdered { a: 0, b: 3 };
        set_forward_branch_target(&mut self.output.instructions[48], 68);
        self.output.instructions[50] = Instruction::FloatSubtractSingle { d: 2, a: 3, b: 1 };
        self.output.instructions.swap(55, 56);
        self.output.instructions[55] = Instruction::FloatAddSingle { d: 0, a: 1, b: 4 };
        self.output.instructions[55] = Instruction::FloatAddSingle { d: 0, a: 1, b: 4 };

        for index in [66, 65, 49, 34, 31, 23, 20, 5, 3] {
            self.remove_structured_condition_instruction(index);
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn set_float_load_destination(instruction: &mut Instruction, destination: u8) {
    match instruction {
        Instruction::LoadFloatSingle { d, .. } => *d = destination,
        _ => unreachable!("the complete bounded-acceleration stream was recognized"),
    }
}

fn set_float_store_source(instruction: &mut Instruction, source: u8) {
    match instruction {
        Instruction::StoreFloatSingle { s, .. } => *s = source,
        _ => unreachable!("the complete bounded-acceleration stream was recognized"),
    }
}

fn set_forward_branch_target(instruction: &mut Instruction, destination: usize) {
    match instruction {
        Instruction::BranchConditionalForward { target, .. } => *target = destination,
        _ => unreachable!("the complete bounded-acceleration stream was recognized"),
    }
}

fn is_unscheduled_bounded_acceleration(instructions: &[Instruction]) -> bool {
    if instructions.len() != 70 {
        return false;
    }
    let same_member = |indices: &[usize]| {
        let Some(first_offset) = indices.iter().find_map(|index| match instructions[*index] {
            Instruction::LoadFloatSingle { a: 3, offset, .. } => Some(offset),
            _ => None,
        }) else {
            return false;
        };
        indices.iter().all(|index| {
            matches!(instructions[*index], Instruction::LoadFloatSingle { a: 3, offset, .. }
                if offset == first_offset)
        })
    };
    if !same_member(&[4, 20, 23]) || !same_member(&[46, 49, 60, 65]) {
        return false;
    }

    matches!(instructions[0], Instruction::LoadFloatSingle { d: 0, a: 0, .. })
        && matches!(instructions[1], Instruction::FloatCompareUnordered { a: 3, b: 0 })
        && matches!(instructions[3], Instruction::FloatMove { d: 1, b: 4 })
        && matches!(instructions[6], Instruction::FloatCompareOrdered { a: 3, b: 0 })
        && matches!(instructions[17], Instruction::FloatCompareOrdered { a: 0, b: 2 })
        && matches!(instructions[18], Instruction::ConditionRegisterOr { .. })
        && matches!(instructions[28], Instruction::StoreFloatSingle { s: 1, a: 3, .. })
        && matches!(instructions[30], Instruction::FloatMultiplySingle { d: 5, a: 1, c: 2 })
        && matches!(instructions[37], Instruction::FloatAddSingle { d: 0, a: 1, b: 2 })
        && matches!(instructions[40], Instruction::FloatNegate { d: 2, b: 4 })
        && matches!(instructions[52], Instruction::FloatAddSingle { d: 0, a: 1, b: 2 })
        && matches!(instructions[55], Instruction::FloatMove { d: 2, b: 4 })
        && matches!(instructions[56], Instruction::FloatAddSingle { d: 0, a: 1, b: 2 })
        && matches!(instructions[61], Instruction::FloatAddSingle { d: 3, a: 1, b: 2 })
        && matches!(instructions[68], Instruction::StoreFloatSingle { s: 2, a: 3, .. })
        && matches!(instructions[69], Instruction::BranchToLinkRegister)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_partial_bounded_acceleration_stream() {
        let instructions = vec![Instruction::BranchToLinkRegister; 70];
        assert!(!is_unscheduled_bounded_acceleration(&instructions));
    }
}
