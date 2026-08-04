//! Final physical schedule for a dense object-destruction loop.
//!
//! With `-use_lmw_stmw on`, MWCC assigns the receiver, member cursor, loop
//! index, guarded member result, and retained null value across `r27..r31`.
//! The generic allocator finds the same five-value range but ranks the
//! lifetimes in spill order. This owner recognizes the complete destruction
//! transaction, restores MWCC's source-role coloring, forwards the already
//! loaded member pointer into the frame-detach call, and fixes the two
//! independent issue-order pairs.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_dense_destroy_loop(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.behavior.use_lmw_stmw
            || self.frame_size != 32
            || self.callee_saved.len() != 5
            || !dense_destroy_loop(&self.output.instructions)
        {
            return;
        }

        repaint_saved_registers(&mut self.output.instructions);
        for location in self.locations.values_mut() {
            if location.class == ValueClass::General {
                location.register = repaint_saved_register(location.register);
            }
        }
        for register in &mut self.callee_saved {
            *register = repaint_saved_register(*register);
        }

        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[14] else {
            unreachable!("the guarded member base load was recognized")
        };
        *d = 3;
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[15] else {
            unreachable!("the guarded member result load was recognized")
        };
        *a = 3;

        crate::move_instruction_before_retargeting(self, 8, 7);
        crate::remove_instruction_retargeting_to_next(self, 19);
        crate::move_instruction_before_retargeting(self, 33, 32);
    }
}

fn repaint_saved_register(register: u8) -> u8 {
    match register {
        31 => 27,
        30 => 29,
        29 => 31,
        28 => 30,
        27 => 28,
        _ => register,
    }
}

fn repaint_saved_registers(instructions: &mut [Instruction]) {
    for instruction in instructions {
        if matches!(
            instruction,
            Instruction::StoreMultipleWord { .. } | Instruction::LoadMultipleWord { .. }
        ) {
            continue;
        }
        mwcc_vreg::for_each_register(instruction, |_, class, register| {
            if class == mwcc_vreg::Class::General {
                *register = repaint_saved_register(*register);
            }
        });
    }
}

fn dense_destroy_loop(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 36 },
            Instruction::StoreMultipleWord { s: 27, a: 1, offset: 12 },
            Instruction::OrRecord { a: 31, s: 3, b: 3 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord { d: 30, a: 31, offset: 12 },
            Instruction::AddImmediate { d: 29, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 28, a: 0, immediate: 0 },
            Instruction::Branch { .. },
            Instruction::LoadWord { d: 0, a: 30, offset: 92 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::BranchAndLink { target: sync },
            Instruction::LoadWord { d: 27, a: 30, offset: 92 },
            Instruction::LoadWord { d: 27, a: 27, offset: 4 },
            Instruction::CompareLogicalWordImmediate { a: 27, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 3, a: 30, offset: 92 },
            Instruction::BranchAndLink { target: detach },
            Instruction::Or { a: 3, s: 27, b: 27 },
            Instruction::BranchAndLink { target: destroy_frame },
            Instruction::LoadWord { d: 3, a: 30, offset: 92 },
            Instruction::BranchAndLink { target: destroy_object },
            Instruction::StoreWord { s: 29, a: 30, offset: 92 },
            Instruction::AddImmediate { d: 30, a: 30, immediate: 96 },
            Instruction::AddImmediate { d: 28, a: 28, immediate: 1 },
            Instruction::LoadWord { d: 0, a: 31, offset: 8 },
            Instruction::CompareLogicalWord { a: 28, b: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadMultipleWord { d: 27, a: 1, offset: 12 },
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ] if sync == "_rwFrameSyncDirty"
            && detach == "_rwObjectHasFrameSetFrame"
            && destroy_frame == "RwFrameDestroy"
            && destroy_object == "RpLightDestroy"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repaints_the_five_saved_value_roles_without_changing_the_range_marker() {
        let mut instructions = [
            Instruction::StoreMultipleWord { s: 27, a: 1, offset: 12 },
            Instruction::OrRecord { a: 31, s: 3, b: 3 },
            Instruction::LoadWord { d: 30, a: 31, offset: 12 },
            Instruction::StoreWord { s: 29, a: 30, offset: 92 },
            Instruction::AddImmediate { d: 28, a: 28, immediate: 1 },
            Instruction::LoadMultipleWord { d: 27, a: 1, offset: 12 },
        ];

        repaint_saved_registers(&mut instructions);

        assert!(matches!(instructions[0], Instruction::StoreMultipleWord { s: 27, .. }));
        assert!(matches!(instructions[1], Instruction::OrRecord { a: 27, .. }));
        assert!(matches!(instructions[2], Instruction::LoadWord { d: 29, a: 27, .. }));
        assert!(matches!(instructions[3], Instruction::StoreWord { s: 31, a: 29, .. }));
        assert!(matches!(instructions[4], Instruction::AddImmediate { d: 30, a: 30, .. }));
        assert!(matches!(instructions[5], Instruction::LoadMultipleWord { d: 27, .. }));
    }
}
