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
        normalize_instructions(&mut self.output.instructions, receiver);
    }
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

}
