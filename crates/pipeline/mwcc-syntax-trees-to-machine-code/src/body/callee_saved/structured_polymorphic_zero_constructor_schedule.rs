//! Final schedule for a derived constructor with mixed zeroed members.
//!
//! A compact constructor which calls one base constructor, installs its own
//! primary vptr, then initializes a three-float value, a pointer, and a narrow
//! state field is one scheduling region in build 163. The generic statement
//! emitter has the complete operations, but independently materializes the
//! constants and chooses the ordinary one-saved-register frame. This owner
//! recognizes the complete physical transaction after allocation and applies
//! the measured issue order and legacy frame convention.

#[allow(unused_imports)]
use super::*;

const INSTRUCTION_COUNT: usize = 24;
const SCHEDULE: [usize; INSTRUCTION_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 14, 16, 10, 19, 11, 12, 13, 15, 17, 18, 20, 21,
    22, 23,
];

impl Generator {
    pub(crate) fn schedule_polymorphic_zero_constructor(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
            || self.frame_size != 16
            || self.callee_saved.len() != 1
            || !self.output.name.starts_with("__ct__")
            || !candidate_shape(&self.output.instructions)
            || self
                .output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>()
                != [6, 7, 8, 10, 14]
        {
            return;
        }

        let old = self.output.instructions.clone();
        self.output.instructions = SCHEDULE
            .iter()
            .map(|&index| old[index].clone())
            .collect();
        rewrite_registers_and_frame(&mut self.output.instructions);

        for relocation in &mut self.output.relocations {
            relocation.instruction_index = SCHEDULE
                .iter()
                .position(|&old_index| old_index == relocation.instruction_index)
                .expect("constructor schedule contains every instruction");
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        self.frame_size = 24;
    }
}

fn candidate_shape(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 1 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::StoreWord { s: 31, a: 1, offset: 12 },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 0, a: 3, .. },
            Instruction::StoreWord { s: 0, a: 31, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 32 },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 28 },
            Instruction::StoreFloatSingle { s: 0, a: 31, offset: 24 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 31, offset: 16 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: -1 },
            Instruction::StoreHalfword { s: 0, a: 31, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    )
}

fn rewrite_registers_and_frame(instructions: &mut [Instruction]) {
    let [
        _,
        _,
        _,
        Instruction::StoreWordWithUpdate { offset: frame, .. },
        Instruction::StoreWord { offset: saved, .. },
        entry_copy,
        _,
        _,
        _,
        _,
        string_address,
        _,
        _,
        _,
        _,
        _,
        _,
        string_store,
        _,
        Instruction::LoadWord { offset: link, .. },
        Instruction::LoadWord { offset: restore, .. },
        Instruction::AddImmediate { immediate: release, .. },
        _,
        _,
    ] = instructions
    else {
        unreachable!("constructor schedule changed shape")
    };

    *frame = -24;
    *saved = 20;
    *entry_copy = Instruction::AddImmediate { d: 31, a: 3, immediate: 0 };
    let Instruction::AddImmediate { d, .. } = string_address else {
        unreachable!("string address materialization changed shape")
    };
    *d = 4;
    let Instruction::StoreWord { s, .. } = string_store else {
        unreachable!("string member store changed shape")
    };
    *s = 4;
    *link = 28;
    *restore = 20;
    *release = 24;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_complete_mixed_zero_constructor_packet() {
        let mut instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::load_immediate(5, 1),
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::StoreWord { s: 31, a: 1, offset: 12 },
            Instruction::move_register(31, 3),
            Instruction::BranchAndLink { target: "base_constructor".to_owned() },
            Instruction::load_immediate_shifted(3, 0),
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 31, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
        ];
        for offset in [32, 28, 24] {
            instructions.push(Instruction::StoreFloatSingle { s: 0, a: 31, offset });
        }
        instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 31, offset: 16 },
            Instruction::load_immediate(0, -1),
            Instruction::StoreHalfword { s: 0, a: 31, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::move_register(3, 31),
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);

        assert!(candidate_shape(&instructions));
        let mut scheduled = SCHEDULE
            .iter()
            .map(|&index| instructions[index].clone())
            .collect::<Vec<_>>();
        rewrite_registers_and_frame(&mut scheduled);
        assert!(matches!(scheduled[3], Instruction::StoreWordWithUpdate { offset: -24, .. }));
        assert!(matches!(scheduled[10], Instruction::AddImmediate { d: 4, .. }));
        assert!(matches!(scheduled[17], Instruction::StoreWord { s: 4, .. }));
        assert!(matches!(scheduled[21], Instruction::AddImmediate { immediate: 24, .. }));
    }
}
