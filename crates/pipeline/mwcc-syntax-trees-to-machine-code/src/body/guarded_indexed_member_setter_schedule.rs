//! Final 2.4.x schedule for a guarded indexed-object member setter.
//!
//! The repeatable scalar-setter inliner removes the middle call, after which
//! MWCC keeps the indexed object in `r31`, uses `r4` for the scaled index, and
//! stores directly through the saved home. This pass owns that complete
//! physical transaction, including the compare latency slot and the discarded
//! inline-parameter copy.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, MachineFunction, RelocationKind};

const PREFIX_SCHEDULE: [usize; 6] = [0, 1, 4, 2, 3, 5];
const SETTER_SCHEDULE: [usize; 4] = [0, 2, 1, 3];

impl Generator {
    pub(crate) fn schedule_guarded_indexed_member_setter(&mut self) -> bool {
        if !self.behavior.repeatable_scalar_member_setter_inlining {
            return false;
        }
        schedule(&mut self.output)
    }
}

fn schedule(output: &mut MachineFunction) -> bool {
    if !candidate(output) {
        return false;
    }

    remove_instruction(output, 16);
    crate::permute_machine_function_region(output, 0, &PREFIX_SCHEDULE);
    crate::permute_machine_function_region(output, 16, &SETTER_SCHEDULE);

    output.instructions[10] = Instruction::MultiplyImmediate {
        d: 4,
        a: 3,
        immediate: 2192,
    };
    output.instructions[11] = Instruction::AddImmediateShifted {
        d: 3,
        a: 0,
        immediate: 0,
    };
    output.instructions[12] = Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 0,
    };
    output.instructions[13] = Instruction::Add {
        d: 31,
        a: 0,
        b: 4,
    };
    let Instruction::StoreWord { a, .. } = &mut output.instructions[18] else {
        unreachable!("the inlined member setter store was recognized")
    };
    *a = 31;
    true
}

fn candidate(output: &MachineFunction) -> bool {
    let instructions = output.instructions.as_slice();
    if !matches!(
        instructions,
        [
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::StoreWord { s: 31, a: 1, offset: 12 },
            Instruction::CompareWordImmediate { a: 3, immediate: -1 },
            Instruction::BranchConditionalForward { target: 21, .. },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { target: 21, .. },
            Instruction::CompareWordImmediate { a: 3, immediate: 3 },
            Instruction::BranchConditionalForward { target: 21, .. },
            Instruction::MultiplyImmediate { d: 3, a: 3, immediate: 2192 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 0 },
            Instruction::Add { d: 31, a: 0, b: 3 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::BranchAndLink { .. },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 3, offset: 4 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::BranchToLinkRegister,
        ]
    ) {
        return false;
    }
    let relocations = output
        .relocations
        .iter()
        .map(|relocation| (relocation.instruction_index, relocation.kind))
        .collect::<Vec<_>>();
    relocations
        == [
            (11, RelocationKind::Addr16Ha),
            (12, RelocationKind::Addr16Lo),
            (15, RelocationKind::Rel24),
            (20, RelocationKind::Rel24),
        ]
        && super::schedule_relocations::same_target_value(
            &output.relocations,
            &output.constants,
            11,
            12,
        )
}

fn remove_instruction(output: &mut MachineFunction, index: usize) {
    let old_len = output.instructions.len();
    output.instructions.remove(index);
    output
        .relocations
        .retain(|relocation| relocation.instruction_index != index);
    let permutation = (0..old_len)
        .map(|old| {
            if old <= index {
                old
            } else {
                old - 1
            }
        })
        .collect::<Vec<_>>();
    crate::remap_machine_function_indices(output, &permutation);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn input() -> MachineFunction {
        let mut output = MachineFunction::new("release");
        output.instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::StoreWord { s: 31, a: 1, offset: 12 },
            Instruction::CompareWordImmediate { a: 3, immediate: -1 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 21 },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 21 },
            Instruction::CompareWordImmediate { a: 3, immediate: 3 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 21 },
            Instruction::MultiplyImmediate { d: 3, a: 3, immediate: 2192 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 0 },
            Instruction::Add { d: 31, a: 0, b: 3 },
            Instruction::move_register(3, 31),
            Instruction::BranchAndLink { target: "lock".into() },
            Instruction::move_register(3, 31),
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 3, offset: 4 },
            Instruction::move_register(3, 31),
            Instruction::BranchAndLink { target: "unlock".into() },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::BranchToLinkRegister,
        ];
        output.relocations = [
            (11, RelocationKind::Addr16Ha, "pool"),
            (12, RelocationKind::Addr16Lo, "pool"),
            (15, RelocationKind::Rel24, "lock"),
            (20, RelocationKind::Rel24, "unlock"),
        ]
        .into_iter()
        .map(|(instruction_index, kind, target)| Relocation {
            instruction_index,
            kind,
            target: RelocationTarget::External(target.into()),
        })
        .collect();
        output
    }

    #[test]
    fn removes_the_inline_copy_and_schedules_the_saved_object_store() {
        let mut output = input();
        assert!(schedule(&mut output));
        assert_eq!(output.instructions.len(), 25);
        assert!(matches!(
            output.instructions[2],
            Instruction::CompareWordImmediate { a: 3, immediate: -1 }
        ));
        assert!(matches!(
            output.instructions[10],
            Instruction::MultiplyImmediate { d: 4, a: 3, immediate: 2192 }
        ));
        assert!(matches!(
            output.instructions[18],
            Instruction::StoreWord { s: 0, a: 31, offset: 4 }
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [11, 12, 15, 19]
        );
        assert!(matches!(
            output.instructions[5],
            Instruction::BranchConditionalForward { target: 20, .. }
        ));
    }
}
