//! Tail scheduling for object-state transfers.
//!
//! The post-transfer calls prefer register moves for retained objects. The
//! guarded item attachment overlaps its bit transfer and its following item
//! lookup with independent call arguments, then fills the indirect-call setup
//! latency at the function tail.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_tail_schedule(&mut self, function: &Function) {
        if !is_unused_array_state_transfer(function) {
            return;
        }

        schedule_post_transfer_receivers(&mut self.output.instructions);
        schedule_item_attachment(&mut self.output.instructions);
        schedule_item_followup_call(&mut self.output.instructions);
        schedule_terminal_receiver(&mut self.output.instructions);
        schedule_indirect_tail_call(&mut self.output.instructions);
    }
}

fn schedule_post_transfer_receivers(instructions: &mut [Instruction]) {
    if let Some(start) = instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: 30,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 29,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    }) {
        instructions[start] = Instruction::move_register(3, 30);
        instructions[start + 2] = Instruction::move_register(3, 29);
    }

    if let Some(start) = instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: 31,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 29,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 30,
                    immediate: 0,
                },
                Instruction::LoadFloatSingle { d: 1, a: 31, .. },
            ]
        )
    }) {
        instructions[start + 3] = Instruction::move_register(3, 30);
    }
}

fn schedule_item_attachment(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadByteZero { d: 3, a: 31, .. },
                Instruction::LoadByteZero { d: 0, a: 29, .. },
                Instruction::RotateAndMaskInsert {
                    a: 0,
                    s: 3,
                    shift: 0,
                    begin: 27,
                    end: 27,
                },
                Instruction::StoreByte { s: 0, a: 29, .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 30,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 0,
                    immediate: 1,
                },
            ]
        )
    }) else {
        return;
    };
    let original = instructions[start..start + 6].to_vec();
    let mut source_load = original[0].clone();
    let mut bit_insert = original[2].clone();
    match &mut source_load {
        Instruction::LoadByteZero { d, .. } => *d = 5,
        _ => unreachable!("the item bit source load was matched"),
    }
    match &mut bit_insert {
        Instruction::RotateAndMaskInsert { s, .. } => *s = 5,
        _ => unreachable!("the item bit insert was matched"),
    }
    instructions[start..start + 6].clone_from_slice(&[
        original[4].clone(),
        original[5].clone(),
        source_load,
        original[1].clone(),
        bit_insert,
        original[3].clone(),
    ]);
}

fn schedule_item_followup_call(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(6).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord { d: 3, a: 31, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 30,
                    immediate: 0,
                },
                Instruction::LoadWord { d: 5, a: 29, .. },
                Instruction::LoadWord {
                    d: 5,
                    a: 5,
                    offset: 8,
                },
                Instruction::LoadByteZero { d: 5, a: 5, .. },
                Instruction::BranchAndLink { .. },
            ]
        )
    }) else {
        return;
    };
    let original = instructions[start..start + 6].to_vec();
    instructions[start..start + 6].clone_from_slice(&[
        original[2].clone(),
        Instruction::move_register(4, 30),
        original[0].clone(),
        original[3].clone(),
        original[4].clone(),
        original[5].clone(),
    ]);
}

fn schedule_terminal_receiver(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(4).position(|window| {
        matches!(
            window,
            [
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 27,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 12,
                    a: 28,
                    immediate: 0,
                },
            ]
        )
    }) else {
        return;
    };
    instructions[start + 1] = Instruction::move_register(3, 27);
}

fn schedule_indirect_tail_call(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(3).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 12,
                    a: 28,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 30,
                    immediate: 0,
                },
                Instruction::MoveToLinkRegister { s: 12 },
            ]
        )
    }) else {
        return;
    };
    instructions.swap(start + 1, start + 2);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_an_incomplete_indirect_tail_unchanged() {
        let mut instructions = [
            Instruction::AddImmediate {
                d: 12,
                a: 28,
                immediate: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
        ];
        let original = instructions.clone();

        schedule_indirect_tail_call(&mut instructions);

        assert_eq!(instructions, original);
    }
}
