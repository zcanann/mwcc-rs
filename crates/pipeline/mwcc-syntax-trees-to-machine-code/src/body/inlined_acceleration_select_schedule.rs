//! Final lifetime scheduling for an inlined acceleration selector.
//!
//! MWCC coalesces the caller's selected acceleration/target lanes with the
//! scalar parameters of a sole-use helper.  Keeping the velocity member and
//! zero literal live through the nested clamp avoids four member reloads and
//! two literal reloads.  This pass owns only the complete measured physical
//! region produced by that semantic inline composition.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_inlined_acceleration_select(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(44)
            .position(is_unscheduled_acceleration_select)
        else {
            return;
        };
        let leading_literal = if matches!(
            self.output.instructions[start],
            Instruction::LoadFloatSingle { a: 0, .. }
        ) {
            start
        } else {
            start + 1
        };
        let literal_indices = [leading_literal, start + 13, start + 15, start + 22, start + 26];
        if !literal_indices.iter().all(|index| {
            self.output
                .relocations
                .iter()
                .any(|relocation| relocation.instruction_index == *index)
        }) {
            return;
        }

        // The sibling source shape can initially emit the literal before the
        // stick member.  MWCC orders the member first in both cases.
        if matches!(
            self.output.instructions[start],
            Instruction::LoadFloatSingle { a: 0, .. }
        ) {
            self.output.instructions.swap(start, start + 1);
            for relocation in &mut self.output.relocations {
                relocation.instruction_index = match relocation.instruction_index {
                    index if index == start => start + 1,
                    index if index == start + 1 => start,
                    index => index,
                };
            }
        }

        self.output.instructions[start + 10] = Instruction::FloatMultiplySingle {
            d: 2,
            a: 4,
            c: 2,
        };
        self.output.instructions[start + 11] = Instruction::FloatMultiplySingle {
            d: 4,
            a: 4,
            c: 3,
        };
        set_float_load_destination(&mut self.output.instructions[start + 13], 4);
        self.output.instructions[start + 14] = Instruction::FloatMove { d: 2, b: 4 };
        set_float_load_destination(&mut self.output.instructions[start + 15], 1);
        self.output.instructions[start + 16] = Instruction::FloatCompareUnordered { a: 4, b: 1 };
        self.output.instructions[start + 19] = Instruction::FloatNegate { d: 2, b: 0 };
        self.output.instructions[start + 23] = Instruction::FloatMultiplySingle {
            d: 0,
            a: 3,
            c: 2,
        };
        self.output.instructions[start + 24] = Instruction::FloatCompareOrdered { a: 0, b: 1 };
        self.output.instructions[start + 27] = Instruction::FloatCompareOrdered { a: 2, b: 1 };
        self.output.instructions[start + 30] = Instruction::FloatAddSingle { d: 0, a: 3, b: 2 };
        self.output.instructions[start + 31] = Instruction::FloatCompareOrdered { a: 0, b: 4 };
        match &mut self.output.instructions[start + 32] {
            Instruction::BranchConditionalForward { target, .. } => *target = start + 42,
            _ => unreachable!("the complete acceleration-select region was recognized"),
        }
        self.output.instructions[start + 34] = Instruction::FloatSubtractSingle { d: 2, a: 4, b: 3 };
        self.output.instructions[start + 37] = Instruction::FloatAddSingle { d: 0, a: 3, b: 2 };
        self.output.instructions[start + 38] = Instruction::FloatCompareOrdered { a: 0, b: 4 };
        self.output.instructions[start + 41] = Instruction::FloatSubtractSingle { d: 2, a: 4, b: 3 };
        match &mut self.output.instructions[start + 42] {
            Instruction::StoreFloatSingle { s, .. } => *s = 2,
            _ => unreachable!("the complete acceleration-select region was recognized"),
        }

        // Remove from the end so the measured indices above stay valid while
        // rewriting. Branch, label, and relocation indices are remapped by the
        // shared structured-condition removal primitive.
        for relative in [40, 36, 33, 29, 26, 22] {
            self.remove_structured_condition_instruction(start + relative);
        }
        // The generic nested-if stream's positive clamp skips to the following
        // unconditional join. MWCC folds that edge directly to the final store.
        let join_target = match self.output.instructions[start + 31] {
            Instruction::Branch { target } => target,
            _ => unreachable!("the complete acceleration-select region was recognized"),
        };
        match &mut self.output.instructions[start + 29] {
            Instruction::BranchConditionalForward { target, .. } => *target = join_target,
            _ => unreachable!("the complete acceleration-select region was recognized"),
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn set_float_load_destination(instruction: &mut Instruction, destination: u8) {
    match instruction {
        Instruction::LoadFloatSingle { d, .. } => *d = destination,
        _ => unreachable!("the complete acceleration-select region was recognized"),
    }
}

fn is_unscheduled_acceleration_select(window: &[Instruction]) -> bool {
    let leading_pair = matches!(&window[..2], [
        Instruction::LoadFloatSingle { d: 4, a: 3, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
    ] | [
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::LoadFloatSingle { d: 4, a: 3, .. },
    ]);
    leading_pair
        && matches!(&window[2..], [
            Instruction::FloatCompareOrdered { a: 4, b: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::FloatNegate { d: 0, b: 4 },
            Instruction::Branch { .. },
            Instruction::FloatMove { d: 0, b: 4 },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::ConditionRegisterOr { .. },
            Instruction::BranchConditionalForward { .. },
            Instruction::FloatMultiplySingle { d: 1, a: 4, c: 2 },
            Instruction::FloatMultiplySingle { d: 2, a: 4, c: 3 },
            Instruction::Branch { .. },
            Instruction::LoadFloatSingle { d: 2, a: 0, .. },
            Instruction::FloatMove { d: 1, b: 2 },
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::FloatCompareUnordered { a: 2, b: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: member_offset },
            Instruction::FloatNegate { d: 1, b: 0 },
            Instruction::Branch { .. },
            Instruction::LoadFloatSingle { d: 3, a: 3, offset: retained_offset },
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::FloatMultiplySingle { d: 3, a: 3, c: 1 },
            Instruction::FloatCompareOrdered { a: 3, b: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: reload_one },
            Instruction::FloatAddSingle { d: 0, a: 0, b: 1 },
            Instruction::FloatCompareOrdered { a: 0, b: 2 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: reload_two },
            Instruction::FloatSubtractSingle { d: 1, a: 2, b: 0 },
            Instruction::Branch { .. },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: reload_three },
            Instruction::FloatAddSingle { d: 0, a: 0, b: 1 },
            Instruction::FloatCompareOrdered { a: 0, b: 2 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: reload_four },
            Instruction::FloatSubtractSingle { d: 1, a: 2, b: 0 },
            Instruction::StoreFloatSingle { s: 1, a: 3, .. },
            Instruction::BranchToLinkRegister,
        ] if member_offset == retained_offset
            && retained_offset == reload_one
            && reload_one == reload_two
            && reload_two == reload_three
            && reload_three == reload_four)
}
