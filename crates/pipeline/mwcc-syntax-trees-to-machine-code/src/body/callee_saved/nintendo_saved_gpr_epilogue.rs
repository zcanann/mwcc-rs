//! Nintendo build-163 saved-GPR teardown policy.
//!
//! Most linkage-first frames are normalized by their frame owner. Some bodies
//! retain the earlier `mflr; stw; stwu` prologue, however, while still using
//! Nintendo's derived build-163 epilogue policy. Keep that generation rule in
//! one final physical-stream pass instead of teaching each body owner about
//! compiler-version teardown ordering.

use super::*;

impl Generator {
    /// Restore every trailing saved GPR and the caller stack before writing LR.
    ///
    /// The ordinary scheduler can place `mtlr` inside the saved-register run
    /// after it hoists the independent LR load. Nintendo's build 163 completes
    /// that run and releases the frame first. Only a canonical trailing packet
    /// is changed; computation or control flow between `mtlr` and teardown is
    /// deliberately left to its owning lowering path.
    pub(crate) fn normalize_nintendo_saved_gpr_epilogue(&mut self) {
        if !self.behavior.structured_saved_gpr_stack_first
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterAfterStackRestore
        {
            return;
        }
        let Some((mut link_restore, stack_restore)) =
            nintendo_saved_gpr_epilogue_packet(&self.output.instructions, self.frame_size)
        else {
            return;
        };
        while link_restore < stack_restore {
            crate::move_instruction_before_retargeting(
                self,
                link_restore + 1,
                link_restore,
            );
            link_restore += 1;
        }
    }
}

fn nintendo_saved_gpr_epilogue_packet(
    instructions: &[Instruction],
    frame_size: i16,
) -> Option<(usize, usize)> {
    let link_restore = instructions
        .iter()
        .rposition(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { s: 0 }))?;
    let stack_restore = instructions[link_restore + 1..]
        .iter()
        .position(|instruction| {
            matches!(instruction,
                Instruction::AddImmediate { d: 1, a: 1, immediate }
                    if *immediate == frame_size)
        })
        .map(|offset| link_restore + 1 + offset)?;
    if !matches!(
        instructions.get(stack_restore + 1),
        Some(Instruction::BranchToLinkRegister)
    ) {
        return None;
    }
    let intervening = &instructions[link_restore + 1..stack_restore];
    if intervening.is_empty()
        || intervening.iter().any(|instruction| {
            !matches!(instruction,
                Instruction::LoadWord { d: 14..=31, a: 1, .. })
        })
    {
        return None;
    }
    let has_saved_restore_before_link = instructions[..link_restore]
        .iter()
        .rev()
        .take_while(|instruction| {
            matches!(instruction, Instruction::LoadWord { d: 14..=31, a: 1, .. })
        })
        .next()
        .is_some();
    has_saved_restore_before_link.then_some((link_restore, stack_restore))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_lr_write_stranded_inside_the_saved_gpr_tail() {
        let instructions = vec![
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::LoadWord { d: 31, a: 1, offset: 28 },
            Instruction::LoadWord { d: 30, a: 1, offset: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::LoadWord { d: 29, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(
            nintendo_saved_gpr_epilogue_packet(&instructions, 32),
            Some((3, 5))
        );
    }

    #[test]
    fn rejects_computation_between_the_lr_write_and_teardown() {
        let instructions = vec![
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::load_immediate(3, 1),
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(nintendo_saved_gpr_epilogue_packet(&instructions, 16), None);
    }
}
