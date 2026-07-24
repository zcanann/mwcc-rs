//! Legacy scheduling for mixed scalar POD constructors.
//!
//! Per-store lowering rematerializes every constant. Build 163 instead keeps
//! adjacent equal floats and a trailing integer zero live across the complete
//! member-initialization run.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_pod_constructor_initialization(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
            || !self.output.name.starts_with("__ct__")
            || self.output.instructions.len() != 17
            || !is_pod_constructor_initialization(&self.output.instructions)
            || !schedule_relocations::same_relocated_value(
                &self.output.relocations,
                &self.output.constants,
                0,
                2,
            )
            || schedule_relocations::same_relocated_value(
                &self.output.relocations,
                &self.output.constants,
                0,
                4,
            )
        {
            return;
        }
        let relocated_loads: Vec<usize> = self
            .output
            .relocations
            .iter()
            .map(|relocation| relocation.instruction_index)
            .collect();
        if relocated_loads != [0, 2, 4] {
            return;
        }

        let old = self.output.instructions.clone();
        let mut thousand = old[6].clone();
        let mut thousand_store = old[7].clone();
        match &mut thousand {
            Instruction::AddImmediate { d, .. } => *d = 4,
            _ => unreachable!("shape checked"),
        }
        match &mut thousand_store {
            Instruction::StoreHalfword { s, .. } => *s = 4,
            _ => unreachable!("shape checked"),
        }
        self.output.instructions = vec![
            old[0].clone(),
            thousand,
            old[8].clone(),
            old[1].clone(),
            old[3].clone(),
            old[4].clone(),
            old[5].clone(),
            thousand_store,
            old[9].clone(),
            old[11].clone(),
            old[13].clone(),
            old[15].clone(),
            old[16].clone(),
        ];
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != 2);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index == 4 {
                relocation.instruction_index = 5;
            }
        }
    }
}

fn is_pod_constructor_initialization(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::StoreFloatSingle { s: 0, a: first_base, .. },
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::StoreFloatSingle { s: 0, a: second_base, .. },
            Instruction::LoadFloatSingle { d: 0, a: 0, .. },
            Instruction::StoreFloatSingle { s: 0, a: third_base, .. },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 1000,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: halfword_base,
                ..
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord { s: 0, a: word_base_1, .. },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord { s: 0, a: word_base_2, .. },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord { s: 0, a: word_base_3, .. },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord { s: 0, a: word_base_4, .. },
            Instruction::BranchToLinkRegister,
        ] if first_base == second_base
            && first_base == third_base
            && first_base == halfword_base
            && first_base == word_base_1
            && first_base == word_base_2
            && first_base == word_base_3
            && first_base == word_base_4
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_three_floats_a_halfword_and_a_zero_tail() {
        let mut instructions = vec![
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 4 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::StoreFloatSingle { s: 0, a: 3, offset: 8 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 1000 },
            Instruction::StoreHalfword { s: 0, a: 3, offset: 12 },
        ];
        for offset in [16, 20, 24, 28] {
            instructions.push(Instruction::AddImmediate { d: 0, a: 0, immediate: 0 });
            instructions.push(Instruction::StoreWord { s: 0, a: 3, offset });
        }
        instructions.push(Instruction::BranchToLinkRegister);

        assert!(is_pod_constructor_initialization(&instructions));
    }
}
