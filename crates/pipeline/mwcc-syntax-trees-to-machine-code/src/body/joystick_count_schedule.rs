//! Final scheduling for the three guarded joystick-count updates.
//!
//! The source has two OR-connected stick-axis guards followed by a third
//! independent guard.  Ordinary structured lowering gives each absolute-value
//! diamond and byte-to-double comparison private temporaries.  MWCC instead
//! keeps the first pair's threshold/global base live, performs each absolute
//! value in place, and shares the reset value across the two stores.  Claim
//! only the complete physical stream so these lifetime choices cannot leak
//! into unrelated conditionals.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_joystick_count_updates(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(93)
            .position(is_unscheduled_joystick_count_updates)
        else {
            return;
        };
        if !has_expected_relocations(self, start) {
            return;
        }

        // The second and third groups load their input before the pooled zero.
        swap_instructions_and_relocations(self, start + 28, start + 29);
        swap_instructions_and_relocations(self, start + 59, start + 60);

        // Calls evaluate the bit-index argument before the player byte.
        swap_instructions_and_relocations(self, start + 51, start + 52);
        swap_instructions_and_relocations(self, start + 82, start + 83);

        set_forward_target(&mut self.output.instructions[start + 8], start + 12);
        set_forward_target(&mut self.output.instructions[start + 31], start + 37);
        set_forward_target(&mut self.output.instructions[start + 62], start + 66);
        self.output.instructions[start + 9] = Instruction::FloatNegate { d: 1, b: 1 };
        self.output.instructions[start + 32] = Instruction::FloatNegate { d: 1, b: 1 };
        self.output.instructions[start + 63] = Instruction::FloatNegate { d: 1, b: 1 };

        set_word_load_destination(&mut self.output.instructions[start + 12], 4);
        set_float_load(&mut self.output.instructions[start + 13], 3, 4);
        self.output.instructions[start + 14] = Instruction::FloatCompareOrdered { a: 1, b: 3 };
        self.output.instructions[start + 37] = Instruction::FloatCompareOrdered { a: 1, b: 3 };
        set_float_load_base(&mut self.output.instructions[start + 22], 4);
        set_float_load_base(&mut self.output.instructions[start + 45], 4);

        set_word_load_destination(&mut self.output.instructions[start + 66], 4);
        set_float_load(&mut self.output.instructions[start + 67], 0, 4);
        self.output.instructions[start + 68] = Instruction::FloatCompareOrdered { a: 1, b: 0 };
        set_float_load_base(&mut self.output.instructions[start + 76], 4);

        // Descending original indices keep the complete-shape mutations above
        // readable; the shared removal helper remaps branches and relocations.
        for relative in [75, 65, 64, 57, 44, 36, 35, 34, 33, 21, 11, 10] {
            self.remove_joystick_instruction(start + relative);
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }

    fn remove_joystick_instruction(&mut self, at: usize) {
        self.output.instructions.remove(at);
        self.labels.removed_retargeting_to_next(at, 1);
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
}

fn has_expected_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| (start..start + 93).contains(&relocation.instruction_index))
        .map(|relocation| relocation.instruction_index - start)
        .collect::<Vec<_>>();
    if relative != [5, 12, 19, 21, 28, 35, 42, 44, 54, 59, 66, 73, 75, 85] {
        return false;
    }
    [28, 59].into_iter().all(|index| {
        schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 5,
            start + index,
        )
    }) && [21, 35, 44, 66, 75].into_iter().all(|index| {
        schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 12,
            start + index,
        )
    }) && [42, 73].into_iter().all(|index| {
        schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 19,
            start + index,
        )
    }) && schedule_relocations::same_relocated_value(
        &generator.output.relocations,
        &generator.output.constants,
        start + 54,
        start + 85,
    )
}

fn set_forward_target(instruction: &mut Instruction, destination: usize) {
    let Instruction::BranchConditionalForward { target, .. } = instruction else {
        unreachable!("the complete joystick-count stream was recognized")
    };
    *target = destination;
}

fn set_word_load_destination(instruction: &mut Instruction, destination: u8) {
    let Instruction::LoadWord { d, .. } = instruction else {
        unreachable!("the complete joystick-count stream was recognized")
    };
    *d = destination;
}

fn set_float_load(instruction: &mut Instruction, destination: u8, base: u8) {
    let Instruction::LoadFloatSingle { d, a, .. } = instruction else {
        unreachable!("the complete joystick-count stream was recognized")
    };
    *d = destination;
    *a = base;
}

fn set_float_load_base(instruction: &mut Instruction, base: u8) {
    let Instruction::LoadFloatSingle { a, .. } = instruction else {
        unreachable!("the complete joystick-count stream was recognized")
    };
    *a = base;
}

fn swap_instructions_and_relocations(generator: &mut Generator, left: usize, right: usize) {
    generator.output.instructions.swap(left, right);
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == left => right,
            index if index == right => left,
            index => index,
        };
    }
}

fn is_absolute_value_diamond(window: &[Instruction], start: usize, base: u8) -> Option<i16> {
    match &window[start..start + 7] {
        [
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::LoadFloatSingle { d: 1, a, offset },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, .. },
            Instruction::FloatNegate { d: 0, b: 1 },
            Instruction::Branch { .. },
            Instruction::FloatMove { d: 0, b: 1 },
        ] if *a == base => Some(*offset),
        _ => None,
    }
}

fn is_byte_limit_test(
    window: &[Instruction],
    start: usize,
    base: u8,
    branch_options: u8,
) -> Option<(i16, i16)> {
    match &window[start..start + 11] {
        [
            Instruction::LoadByteZero { d: 3, a: count_base, offset: count_offset },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 0x4330 },
            Instruction::LoadFloatDouble { d: 2, a: 0, .. },
            Instruction::StoreWord { s: 3, a: 1, offset: 20 },
            Instruction::LoadWord { d: 3, a: 0, .. },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: limit_offset },
            Instruction::StoreWord { s: 0, a: 1, offset: 16 },
            Instruction::LoadFloatDouble { d: 1, a: 1, offset: 16 },
            Instruction::FloatSubtractSingle { d: 1, a: 1, b: 2 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::BranchConditionalForward { options, condition_bit: 0, .. },
        ] if *count_base == base && *options == branch_options => {
            Some((*count_offset, *limit_offset))
        }
        _ => None,
    }
}

fn is_call_and_reset(
    window: &[Instruction],
    start: usize,
    base: u8,
    two_stores: bool,
) -> Option<(i16, i16, i16)> {
    let width = if two_stores { 8 } else { 6 };
    let slice = &window[start..start + width];
    match (two_stores, slice) {
        (true, [
            Instruction::LoadByteZero { d: 3, a: player_base, offset: player_offset },
            Instruction::LoadByteZero { d: 4, a: flag_base, offset: flag_offset },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 29, begin: 31, end: 31 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 254 },
            Instruction::StoreByte { s: 0, a: first_store_base, offset: first_store },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 254 },
            Instruction::StoreByte { s: 0, a: second_store_base, offset: second_store },
        ]) if *player_base == base && *flag_base == base
            && *first_store_base == base && *second_store_base == base => {
            Some((*player_offset, *flag_offset, *first_store - *second_store))
        }
        (false, [
            Instruction::LoadByteZero { d: 3, a: player_base, offset: player_offset },
            Instruction::LoadByteZero { d: 4, a: flag_base, offset: flag_offset },
            Instruction::RotateAndMask { a: 4, s: 4, shift: 29, begin: 31, end: 31 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 254 },
            Instruction::StoreByte { s: 0, a: store_base, offset: store_offset },
        ]) if *player_base == base && *flag_base == base && *store_base == base => {
            Some((*player_offset, *flag_offset, *store_offset))
        }
        _ => None,
    }
}

fn is_unscheduled_joystick_count_updates(window: &[Instruction]) -> bool {
    let Some(first_input) = is_absolute_value_diamond(window, 5, 31) else { return false };
    let Some(second_input) = is_absolute_value_diamond(window, 28, 31) else { return false };
    let Some(third_input) = is_absolute_value_diamond(window, 59, 31) else { return false };
    let Some((first_count, first_limit)) = is_byte_limit_test(window, 17, 31, 12) else {
        return false;
    };
    let Some((second_count, second_limit)) = is_byte_limit_test(window, 40, 31, 4) else {
        return false;
    };
    let Some((third_count, third_limit)) = is_byte_limit_test(window, 71, 31, 4) else {
        return false;
    };
    let Some((first_player, first_flag, store_delta)) = is_call_and_reset(window, 51, 31, true)
    else {
        return false;
    };
    let Some((third_player, third_flag, third_store)) = is_call_and_reset(window, 82, 31, false)
    else {
        return false;
    };

    matches!(window, [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::StoreWord { s: 31, a: 1, offset: 28 },
        Instruction::LoadWord { d: 31, a: 3, .. },
        ..,
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::LoadWord { d: 31, a: 1, offset: 28 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ])
        && matches!(&window[12..17], [
            Instruction::LoadWord { d: 3, a: 0, .. },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: first_threshold },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, .. },
        ] if matches!(&window[35..40], [
            Instruction::LoadWord { d: 3, a: 0, .. },
            Instruction::LoadFloatSingle { d: 1, a: 3, offset: second_threshold },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, .. },
        ] if first_threshold == second_threshold))
        && matches!(&window[66..71], [
            Instruction::LoadWord { d: 3, a: 0, .. },
            Instruction::LoadFloatSingle { d: 1, a: 3, .. },
            Instruction::FloatCompareOrdered { a: 0, b: 1 },
            Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, .. },
        ])
        && second_input == first_input + 4
        && first_count + 1 == second_count
        && second_count + 1 == third_count
        && first_limit == second_limit
        && second_limit == third_limit
        && first_player == third_player
        && first_flag == third_flag
        && store_delta == 1
        && third_store == third_count
        && third_input != first_input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_partial_joystick_count_stream() {
        let instructions = vec![Instruction::BranchToLinkRegister; 93];
        assert!(!is_unscheduled_joystick_count_updates(&instructions));
    }
}
