//! Latency schedule for a member-bound call with two saved object parameters.
//!
//! The entry MIN/MAX comparison can issue its independent member loads between
//! linkage and saved-home stores. The matching epilogue starts the link reload
//! before restoring either saved object home.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_member_bound_call(&mut self) {
        let Some((start, saved_right, saved_left)) =
            member_bound_call_entry(&self.output.instructions)
        else {
            return;
        };

        self.move_instruction_before(start + 7, start + 2);
        self.move_instruction_before(start + 8, start + 4);
        self.move_instruction_before(start + 9, start + 7);

        retain_member_bound_call_arguments(
            &mut self.output.instructions,
            start + 10,
            saved_right,
            saved_left,
        );
        // This function owns its measured linkage latency slots. Generic
        // post-allocation linkage scheduling must leave the selected entry
        // order intact; the physical epilogue order is finalized after all
        // other allocation-sensitive normalizers.
        self.owns_link_register_schedule = true;
    }

    pub(crate) fn finalize_structured_member_bound_call_epilogue(&mut self) {
        if !self.owns_link_register_schedule {
            return;
        }
        for epilogue in 0..self.output.instructions.len().saturating_sub(5) {
            let window = &self.output.instructions[epilogue..epilogue + 6];
            if is_member_bound_call_epilogue(window, [31, 0, 30]) {
                self.output.instructions.swap(epilogue, epilogue + 1);
                return;
            }
            if is_member_bound_call_epilogue(window, [31, 30, 0]) {
                self.output.instructions[epilogue..epilogue + 3].rotate_right(1);
                return;
            }
        }
    }
}

fn member_bound_call_entry(instructions: &[Instruction]) -> Option<(usize, u8, u8)> {
    instructions.windows(10).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    ..
                },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, .. },
                Instruction::StoreWord {
                    s: saved_right,
                    a: 1,
                    ..
                },
                Instruction::Or {
                    a: copied_right,
                    s: 4,
                    b: 4,
                },
                Instruction::StoreWord {
                    s: saved_left,
                    a: 1,
                    ..
                },
                Instruction::Or {
                    a: copied_left,
                    s: 3,
                    b: 3,
                },
                Instruction::LoadWord {
                    d: bound,
                    a: 4,
                    offset: right_offset,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: left_offset,
                },
                Instruction::CompareLogicalWord { a: 0, b: compared },
            ] if saved_right == copied_right
                && saved_left == copied_left
                && bound == compared
                && right_offset == left_offset =>
            {
                Some((start, *saved_right, *saved_left))
            }
            _ => None,
        }
    })
}

fn retain_member_bound_call_arguments(
    instructions: &mut [Instruction],
    body_start: usize,
    saved_right: u8,
    saved_left: u8,
) {
    let Some(call) = instructions[body_start..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .map(|relative| body_start + relative)
    else {
        return;
    };
    let Some(argument_pair) = instructions[body_start..call]
        .windows(2)
        .position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadWord { d: 3, a: 3, .. },
                    Instruction::LoadWord { d: 4, a: 4, .. },
                ]
            )
        })
        .map(|relative| body_start + relative)
    else {
        return;
    };
    if let Instruction::LoadWord { a, .. } = &mut instructions[argument_pair] {
        *a = saved_left;
    }
    if let Instruction::LoadWord { a, .. } = &mut instructions[argument_pair + 1] {
        *a = saved_right;
    }
}

fn is_member_bound_call_epilogue(window: &[Instruction], homes: [u8; 3]) -> bool {
    matches!(
        window,
        [
            Instruction::LoadWord { d: first, a: 1, .. },
            Instruction::LoadWord { d: second, a: 1, .. },
            Instruction::LoadWord { d: third, a: 1, .. },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, .. },
            Instruction::BranchToLinkRegister,
        ] if [*first, *second, *third] == homes
    )
}
