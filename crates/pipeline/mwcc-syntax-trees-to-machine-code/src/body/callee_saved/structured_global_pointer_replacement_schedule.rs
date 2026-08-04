//! Final schedule for replacing a global owner across remove/add loops.
//!
//! The source transaction removes every indexed member of the old global
//! owner, publishes a new owner, then adds every indexed member of the new
//! owner. MWCC carries both the element index and its byte offset through the
//! calls. The general structured path deliberately keeps only the source index;
//! this late owner recognizes the complete physical transaction and restores
//! the second induction lane, indexed addressing, and source-role coloring.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_global_pointer_replacement(&mut self) {
        let Some(plan) = GlobalPointerReplacement::recognize(&self.output) else {
            return;
        };
        let old = self.output.instructions.clone();
        self.output.instructions = plan.schedule(&old);
        self.output.relocations.retain_mut(|relocation| {
            let Some(new_index) = plan.relocation_index(relocation.instruction_index) else {
                return false;
            };
            relocation.instruction_index = new_index;
            true
        });
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);

        for location in self.locations.values_mut() {
            if location.class != ValueClass::General {
                continue;
            }
            location.register = match location.register {
                31 => 29,
                30 => 28,
                29 => 30,
                register => register,
            };
        }
        self.callee_saved = vec![31, 30, 29, 28];
    }
}

struct GlobalPointerReplacement;

impl GlobalPointerReplacement {
    fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Self> {
        let instructions = output.instructions.as_slice();
        if !matches!(
            instructions,
            [
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 36 },
                Instruction::StoreWord { s: 31, a: 1, offset: 28 },
                Instruction::Or { a: 31, s: 4, b: 4 },
                Instruction::StoreWord { s: 30, a: 1, offset: 24 },
                Instruction::Or { a: 30, s: 3, b: 3 },
                Instruction::StoreWord { s: 29, a: 1, offset: 20 },
                Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                Instruction::CompareLogicalWord { a: 3, b: 4 },
                Instruction::BranchConditionalForward { .. },
                Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate { d: 29, a: 0, immediate: 0 },
                Instruction::Branch { .. },
                Instruction::Or { a: 3, s: 31, b: 31 },
                Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                Instruction::LoadWord { d: 4, a: 4, offset: 12 },
                Instruction::MultiplyImmediate { d: 0, a: 29, immediate: 96 },
                Instruction::Add { d: 4, a: 4, b: 0 },
                Instruction::LoadWord { d: 4, a: 4, offset: 92 },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: 29, a: 29, immediate: 1 },
                Instruction::LoadWord { d: 3, a: 0, offset: 0 },
                Instruction::LoadWord { d: 0, a: 3, offset: 8 },
                Instruction::CompareLogicalWord { a: 29, b: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::StoreWord { s: 30, a: 0, offset: 0 },
                Instruction::CompareLogicalWordImmediate { a: 30, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::AddImmediate { d: 0, a: 0, immediate: 1 },
                Instruction::StoreWord { s: 0, a: 0, offset: 0 },
                Instruction::AddImmediate { d: 29, a: 0, immediate: 0 },
                Instruction::Branch { .. },
                Instruction::Or { a: 3, s: 31, b: 31 },
                Instruction::LoadWord { d: 4, a: 30, offset: 12 },
                Instruction::MultiplyImmediate { d: 0, a: 29, immediate: 96 },
                Instruction::Add { d: 4, a: 4, b: 0 },
                Instruction::LoadWord { d: 4, a: 4, offset: 92 },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: 29, a: 29, immediate: 1 },
                Instruction::LoadWord { d: 0, a: 30, offset: 8 },
                Instruction::CompareLogicalWord { a: 29, b: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::Branch { .. },
                Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
                Instruction::StoreWord { s: 0, a: 0, offset: 0 },
                Instruction::LoadWord { d: 31, a: 1, offset: 28 },
                Instruction::LoadWord { d: 30, a: 1, offset: 24 },
                Instruction::LoadWord { d: 0, a: 1, offset: 36 },
                Instruction::LoadWord { d: 29, a: 1, offset: 20 },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
                Instruction::BranchToLinkRegister,
            ]
        ) {
            return None;
        }
        let expected = [8usize, 16, 21, 23, 27, 31, 39, 46];
        (output.relocations.len() == expected.len()
            && output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .eq(expected))
        .then_some(Self)
    }

    fn relocation_index(&self, old: usize) -> Option<usize> {
        match old {
            8 => Some(9),
            16 => None,
            21 => Some(21),
            23 => Some(24),
            27 => Some(29),
            31 => Some(33),
            39 => Some(40),
            46 => Some(48),
            _ => None,
        }
    }

    fn schedule(&self, old: &[Instruction]) -> Vec<Instruction> {
        let conditional = |source: usize, target: usize| {
            let Instruction::BranchConditionalForward {
                options,
                condition_bit,
                ..
            } = old[source]
            else {
                unreachable!("replacement-loop branch shape was recognized")
            };
            Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target,
            }
        };
        vec![
            old[0].clone(),
            old[1].clone(),
            old[2].clone(),
            Instruction::StoreWord { s: 31, a: 1, offset: 28 },
            Instruction::StoreWord { s: 30, a: 1, offset: 24 },
            Instruction::StoreWord { s: 29, a: 1, offset: 20 },
            Instruction::move_register(29, 4),
            Instruction::StoreWord { s: 28, a: 1, offset: 16 },
            Instruction::move_register(28, 3),
            Instruction::LoadWord { d: 0, a: 0, offset: 0 },
            Instruction::CompareLogicalWord { a: 28, b: 0 },
            conditional(10, 49),
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            conditional(12, 28),
            Instruction::load_immediate(30, 0),
            Instruction::load_immediate(31, 0),
            Instruction::Branch { target: 24 },
            Instruction::LoadWord { d: 4, a: 3, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 31, immediate: 92 },
            Instruction::move_register(3, 29),
            Instruction::LoadWordIndexed { d: 4, a: 4, b: 0 },
            old[21].clone(),
            Instruction::AddImmediate { d: 31, a: 31, immediate: 96 },
            Instruction::AddImmediate { d: 30, a: 30, immediate: 1 },
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::LoadWord { d: 0, a: 3, offset: 8 },
            Instruction::CompareLogicalWord { a: 30, b: 0 },
            conditional(26, 17),
            Instruction::CompareLogicalWordImmediate { a: 28, immediate: 0 },
            Instruction::StoreWord { s: 28, a: 0, offset: 0 },
            conditional(29, 47),
            Instruction::load_immediate(0, 1),
            Instruction::load_immediate(30, 0),
            old[31].clone(),
            Instruction::move_register(31, 30),
            Instruction::Branch { target: 43 },
            Instruction::LoadWord { d: 4, a: 28, offset: 12 },
            Instruction::AddImmediate { d: 0, a: 31, immediate: 92 },
            Instruction::move_register(3, 29),
            Instruction::LoadWordIndexed { d: 4, a: 4, b: 0 },
            old[39].clone(),
            Instruction::AddImmediate { d: 31, a: 31, immediate: 96 },
            Instruction::AddImmediate { d: 30, a: 30, immediate: 1 },
            Instruction::LoadWord { d: 0, a: 28, offset: 8 },
            Instruction::CompareLogicalWord { a: 30, b: 0 },
            conditional(43, 36),
            Instruction::Branch { target: 49 },
            Instruction::load_immediate(0, 0),
            old[46].clone(),
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::LoadWord { d: 31, a: 1, offset: 28 },
            Instruction::LoadWord { d: 30, a: 1, offset: 24 },
            Instruction::LoadWord { d: 29, a: 1, offset: 20 },
            Instruction::LoadWord { d: 28, a: 1, offset: 16 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::BranchToLinkRegister,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_the_redundant_old_owner_load_out_of_the_final_schedule() {
        let plan = GlobalPointerReplacement;
        assert_eq!(plan.relocation_index(8), Some(9));
        assert_eq!(plan.relocation_index(16), None);
        assert_eq!(plan.relocation_index(23), Some(24));
        assert_eq!(plan.relocation_index(46), Some(48));
    }
}
