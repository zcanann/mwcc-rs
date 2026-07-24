//! Legacy prologue and epilogue scheduling for saved-receiver array replacement.

#[allow(unused_imports)]
use super::*;

fn schedule_saved_receiver_array_release(
    instructions: &mut [Instruction],
    callee_saved: &[u8],
) -> bool {
    if callee_saved.len() != 2 || instructions.len() < 15 {
        return false;
    }
    let (
        Instruction::StoreWord { s: first_saved, .. },
        Instruction::StoreWord {
            s: second_saved, ..
        },
    ) = (&instructions[3], &instructions[5])
    else {
        return false;
    };
    let (first_saved, second_saved) = (*first_saved, *second_saved);
    if first_saved == second_saved {
        return false;
    }
    if !matches!(&instructions[..9], [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, .. },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
        Instruction::StoreWord { s: first_store, a: 1, .. },
        Instruction::AddImmediate { d: first_copy, a: 4, immediate: 0 },
        Instruction::StoreWord { s: second_store, a: 1, .. },
        Instruction::AddImmediate { d: second_copy, a: 3, immediate: 0 },
        Instruction::LoadWord { d: 3, a: member_base, .. },
        Instruction::BranchAndLink { target },
    ] if *first_store == first_saved
        && *first_copy == first_saved
        && *second_store == second_saved
        && *second_copy == second_saved
        && *member_base == second_saved
        && target == "__dla__FPv")
    {
        return false;
    }
    let end = instructions.len();
    if !matches!(&instructions[end - 6..], [
        Instruction::LoadWord { d: 0, a: 1, .. },
        Instruction::LoadWord { d: first_load, a: 1, .. },
        Instruction::LoadWord { d: second_load, a: 1, .. },
        Instruction::AddImmediate { d: 1, a: 1, .. },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ] if *first_load == first_saved && *second_load == second_saved)
    {
        return false;
    }

    instructions[4] = Instruction::move_register(first_saved, 4);
    instructions[6] = Instruction::move_register(second_saved, 3);
    let Instruction::LoadWord { a, .. } = &mut instructions[7] else {
        unreachable!("the array-release prefix was checked above")
    };
    *a = 3;
    instructions.swap(end - 3, end - 2);
    true
}

impl Generator {
    /// The receiver is still live in entry r3 when the first operation releases
    /// one of its array members. Legacy MWCC reads that member through r3, uses
    /// logical `mr` copies for both saved parameters, and restores LR before
    /// the final stack adjustment.
    pub(crate) fn schedule_saved_receiver_array_release_frame(&mut self) {
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            schedule_saved_receiver_array_release(
                &mut self.output.instructions,
                &self.callee_saved,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_a_two_home_array_release_frame() {
        let mut instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::AddImmediate {
                d: 31,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord { s: 30, a: 1, offset: 16 },
            Instruction::AddImmediate {
                d: 30,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord { d: 3, a: 30, offset: 28 },
            Instruction::BranchAndLink {
                target: "__dla__FPv".into(),
            },
            Instruction::BranchAndLink {
                target: "allocate".into(),
            },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 30, a: 1, offset: 16 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];

        assert!(schedule_saved_receiver_array_release(
            &mut instructions,
            &[31, 30],
        ));
        assert!(matches!(
            instructions[4],
            Instruction::Or { a: 31, s: 4, b: 4 }
        ));
        assert!(matches!(
            instructions[6],
            Instruction::Or { a: 30, s: 3, b: 3 }
        ));
        assert!(matches!(
            instructions[7],
            Instruction::LoadWord { d: 3, a: 3, .. }
        ));
        assert!(matches!(
            instructions[13],
            Instruction::MoveToLinkRegister { s: 0 }
        ));
    }
}
