//! Link-register scheduling around a linkage-first FPR restore helper.
//!
//! The Nintendo build-163 policy issues the independent saved-LR load before
//! setting up and calling `_restfpr_N`, then restores the GPR suffix. This pass
//! runs after allocator-selected FPR frame materialization, when that complete
//! helper epilogue exists.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_allocated_float_helper_epilogue(&mut self) -> bool {
        if self.behavior.saved_float_epilogue_style
            != mwcc_versions::SavedFloatEpilogueStyle::LinkReloadBeforeFinalRestore
        {
            return false;
        }
        let Some((start, reload)) = plan(&self.output.instructions, self.frame_size) else {
            return false;
        };
        // An early return that entered at helper setup must also execute the
        // hoisted LR load. Make the load the block owner before permuting it.
        crate::retarget_instruction_destinations(self, start, reload);
        crate::move_instruction_before_retargeting(self, reload, start);
        true
    }
}

fn plan(instructions: &[Instruction], frame_size: i16) -> Option<(usize, usize)> {
    instructions.windows(7).enumerate().find_map(|(start, window)| {
        matches!(&window, [
            Instruction::AddImmediate { d: 11, a: 1, immediate },
            Instruction::BranchAndLink { target },
            Instruction::LoadMultipleWord { d, a: 1, .. },
            Instruction::LoadWord { d: 0, a: 1, offset },
            Instruction::AddImmediate { d: 1, a: 1, immediate: stack_size },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ] if *immediate == frame_size
            && target.starts_with("_restfpr_")
            && *d >= 14
            && *offset == frame_size + 4
            && *stack_size == frame_size)
            .then_some((start, start + 3))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_float_helper_before_a_gpr_suffix_restore() {
        let instructions = vec![
            Instruction::AddImmediate { d: 11, a: 1, immediate: 168 },
            Instruction::BranchAndLink { target: "_restfpr_20".into() },
            Instruction::LoadMultipleWord { d: 25, a: 1, offset: 44 },
            Instruction::LoadWord { d: 0, a: 1, offset: 172 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 168 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        assert_eq!(plan(&instructions, 168), Some((0, 3)));
    }

    #[test]
    fn rejects_a_plain_call_before_the_epilogue() {
        let mut instructions = vec![
            Instruction::AddImmediate { d: 11, a: 1, immediate: 32 },
            Instruction::BranchAndLink { target: "work".into() },
            Instruction::LoadMultipleWord { d: 30, a: 1, offset: 24 },
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        assert_eq!(plan(&instructions, 32), None);
        if let Instruction::BranchAndLink { target } = &mut instructions[1] {
            *target = "_restfpr_30".into();
        }
        assert_eq!(plan(&instructions, 32), Some((0, 3)));
    }
}
