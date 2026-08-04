//! Copy encodings inside a recognized guarded callback transaction.
//!
//! A retained receiver copied immediately into `r3` for a one-argument direct
//! call is address preservation, which linkage-first MWCC spells as `mr`.
//! Copies hoisted across independent stores remain `addi`; retaining that
//! distinction here prevents the global materialization convention from
//! flattening two different scheduling purposes into one opcode.

use super::*;

impl Generator {
    pub(crate) fn normalize_guarded_callback_single_argument_receivers(
        &mut self,
        function: &Function,
    ) {
        let Some(plan) = super::structured_guarded_member_lvalue::recognize(function) else {
            return;
        };
        let Some(receiver) = self.output.instructions.windows(2).find_map(|window| {
            matches!(window, [
                Instruction::AddImmediate { d: 4, a, immediate },
                Instruction::LoadWord { a: load_base, offset, .. },
            ] if *immediate == plan.member_offset
                && *a == *load_base
                && *offset == plan.member_offset)
            .then(|| match window[0] {
                Instruction::AddImmediate { a, .. } => a,
                _ => unreachable!(),
            })
        }) else {
            return;
        };
        if let Some(snapshot) = callback_result_snapshot(&self.output.instructions) {
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[snapshot + 2]
            else {
                unreachable!("the callback result comparison was recognized")
            };
            *a = 3;
            crate::remove_instruction_retargeting_to_next(self, snapshot + 1);
        }
        if let Some(reload) = countdown_guard_reload(
            &self.output.instructions,
            receiver,
            plan.member_offset,
        ) {
            crate::remove_instruction_retargeting_to_next(self, reload);
        }
        normalize_instructions(&mut self.output.instructions, receiver);
        schedule_copy_placement(self, receiver);
    }
}

fn callback_result_snapshot(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(3).position(|window| {
        matches!(window, [
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
        ])
    })
}

fn countdown_guard_reload(
    instructions: &[Instruction],
    receiver: u8,
    offset: i16,
) -> Option<usize> {
    instructions.windows(7).enumerate().find_map(|(start, window)| {
        matches!(window, [
            Instruction::AddImmediate { d: 4, a, immediate },
            Instruction::LoadWord { d: 3, a: load_base, offset: load_offset },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 3, a: reload_base, offset: reload_offset },
            Instruction::AddImmediate { d: 0, a: 3, immediate: -1 },
            Instruction::StoreWord { s: 0, a: 4, offset: 0 },
        ] if *a == receiver
            && *immediate == offset
            && *load_base == receiver
            && *load_offset == offset
            && *reload_base == receiver
            && *reload_offset == offset)
        .then_some(start + 4)
    })
}

fn schedule_copy_placement(generator: &mut Generator, receiver: u8) {
    if let Some(start) = initial_zero_cleanup_copy(&generator.output.instructions) {
        crate::move_instruction_before_retargeting(generator, start + 3, start + 1);
    }
    if let Some(start) = state_change_receiver_copy(&generator.output.instructions, receiver) {
        crate::move_instruction_before_retargeting(generator, start + 2, start + 1);
    }
    let mut cursor = 0;
    while let Some(relative) = derived_list_argument_copy(
        &generator.output.instructions[cursor..],
        receiver,
    ) {
        let start = cursor + relative;
        crate::move_instruction_before_retargeting(generator, start + 2, start + 1);
        cursor = start + 4;
    }
    if let Some(start) = zero_return_copy(&generator.output.instructions) {
        crate::move_instruction_before_retargeting(generator, start + 4, start + 1);
    }
}

fn initial_zero_cleanup_copy(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(window, [
            Instruction::AddImmediate { d: zero, a: 0, immediate: 0 },
            Instruction::StoreWord { s: word, a: base, .. },
            Instruction::StoreByte { s: byte, a: byte_base, .. },
            Instruction::AddImmediate { d: 3, a: copy_base, immediate: 0 },
            Instruction::BranchAndLink { .. },
        ] if zero == word && word == byte && base == byte_base && byte_base == copy_base)
    })
}

fn state_change_receiver_copy(instructions: &[Instruction], receiver: u8) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(window, [
            Instruction::AddImmediate { d: state, a: 0, immediate: 6 },
            Instruction::StoreByte { s: stored, a, .. },
            Instruction::AddImmediate { d: 3, a: copy, immediate: 0 },
            Instruction::BranchAndLink { .. },
        ] if state == stored && *a == receiver && *copy == receiver)
    })
}

fn derived_list_argument_copy(instructions: &[Instruction], receiver: u8) -> Option<usize> {
    instructions.windows(4).position(|window| {
        matches!(window, [
            Instruction::LoadWord { d: 3, a, .. },
            Instruction::AddImmediate { d: 3, a: 3, immediate },
            Instruction::AddImmediate { d: 4, a: copy, immediate: 0 },
            Instruction::BranchAndLink { .. },
        ] if *a == receiver && *copy == receiver && *immediate != 0)
    })
}

fn zero_return_copy(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(5).position(|window| {
        matches!(window, [
            Instruction::AddImmediate { d: zero, a: 0, immediate: 0 },
            Instruction::StoreWord { s: first, .. },
            Instruction::StoreByte { s: second, .. },
            Instruction::StoreWord { s: third, .. },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        ] if zero == first && first == second && second == third)
    })
}

fn normalize_instructions(instructions: &mut [Instruction], receiver: u8) {
    for index in 0..instructions.len().saturating_sub(1) {
        let is_store_hoist = index > 0
            && matches!(
                instructions[index - 1],
                Instruction::StoreByte { a, .. } if a == receiver
            );
        if is_store_hoist {
            continue;
        }
        let matches = matches!(
            &instructions[index..index + 2],
            [
                Instruction::AddImmediate { d: 3, a, immediate: 0 },
                Instruction::BranchAndLink { .. },
            ] if *a == receiver
        );
        if matches {
            instructions[index] = Instruction::move_register(3, receiver);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_adjacent_and_store_hoisted_receiver_copies() {
        let mut instructions = vec![
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::BranchAndLink { target: "first".into() },
            Instruction::StoreByte { s: 0, a: 30, offset: 72 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::BranchAndLink { target: "second".into() },
        ];
        normalize_instructions(&mut instructions, 30);
        assert!(matches!(instructions[0], Instruction::Or { a: 3, s: 30, b: 30 }));
        assert!(matches!(instructions[3], Instruction::AddImmediate { d: 3, a: 30, immediate: 0 }));
    }

    #[test]
    fn recognizes_independent_copy_placement_packets() {
        let cleanup = vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 31, offset: 12 },
            Instruction::StoreByte { s: 0, a: 31, offset: 3 },
            Instruction::AddImmediate { d: 3, a: 31, immediate: 0 },
            Instruction::BranchAndLink { target: "cleanup".into() },
        ];
        assert_eq!(initial_zero_cleanup_copy(&cleanup), Some(0));

        let state = vec![
            Instruction::load_immediate(0, 6),
            Instruction::StoreByte { s: 0, a: 30, offset: 72 },
            Instruction::AddImmediate { d: 3, a: 30, immediate: 0 },
            Instruction::BranchAndLink { target: "cut".into() },
        ];
        assert_eq!(state_change_receiver_copy(&state, 30), Some(0));

        let list = vec![
            Instruction::LoadWord { d: 3, a: 30, offset: 4 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 8 },
            Instruction::AddImmediate { d: 4, a: 30, immediate: 0 },
            Instruction::BranchAndLink { target: "add".into() },
        ];
        assert_eq!(derived_list_argument_copy(&list, 30), Some(0));
    }

    #[test]
    fn recognizes_redundant_callback_and_countdown_copies() {
        let callback = vec![
            Instruction::BranchToLinkRegisterAndLink,
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
        ];
        assert_eq!(callback_result_snapshot(&callback), Some(0));

        let countdown = vec![
            Instruction::AddImmediate { d: 4, a: 30, immediate: 52 },
            Instruction::LoadWord { d: 3, a: 30, offset: 52 },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 1, target: 7 },
            Instruction::LoadWord { d: 3, a: 30, offset: 52 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: -1 },
            Instruction::StoreWord { s: 0, a: 4, offset: 0 },
        ];
        assert_eq!(countdown_guard_reload(&countdown, 30, 52), Some(4));
    }

}
