//! Final physical schedule for a compact five-word runtime trampoline frame.
//!
//! The contiguous image copy and the following read/write diamond are selected
//! by separate owners. Build 163 overlaps their independent condition with the
//! pool-address latency, uses linkage-first prologue order, and folds adjacent
//! high-half OR operations in each diamond arm. This pass commits that schedule
//! only after the complete allocated region is present.

use super::*;
use mwcc_machine_code::RelocationTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PhysicalPlan {
    condition: usize,
    shifted_or_pairs: [usize; 2],
}

fn relocation_target(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&RelocationTarget> {
    output.relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index && relocation.kind == kind)
            .then_some(&relocation.target)
    })
}

fn has_incoming_branch(instructions: &[Instruction], target: usize) -> bool {
    instructions.iter().any(|instruction| match instruction {
        Instruction::Branch { target: branch_target }
        | Instruction::BranchConditionalForward {
            target: branch_target,
            ..
        } => *branch_target == target,
        _ => false,
    })
}

fn physical_plan(output: &mwcc_machine_code::MachineFunction) -> Option<PhysicalPlan> {
    let instructions = &output.instructions;
    if !matches!(
        instructions.get(0..16),
        Some([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                ..
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate { d: 7, a: 6, .. },
            Instruction::LoadWord {
                d: 6,
                a: 7,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 6,
                a: 1,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 6,
                a: 7,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 6,
                a: 1,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 24,
            },
            Instruction::CompareWordImmediate { immediate: 0, .. },
        ])
    ) {
        return None;
    }
    let RelocationTarget::AnonymousRodataAt(image) =
        relocation_target(output, 2, RelocationKind::Addr16Ha)?
    else {
        return None;
    };
    if !matches!(
        relocation_target(output, 4, RelocationKind::Addr16Lo),
        Some(RelocationTarget::AnonymousRodataAt(low_image)) if low_image == image
    )
        || !matches!(
            instructions.get(16),
            Some(Instruction::BranchConditionalForward { .. })
        )
    {
        return None;
    }

    let shifted_or_pairs: Vec<usize> = instructions
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| match pair {
            [
                Instruction::OrImmediateShifted {
                    a,
                    s,
                    immediate: first,
                },
                Instruction::OrImmediateShifted {
                    a: next_a,
                    s: next_s,
                    immediate: second,
                },
            ] if a == next_a
                && a == next_s
                && a == s
                && *first != 0
                && *second != 0
                && !has_incoming_branch(instructions, index + 1)
                && output
                    .relocations
                    .iter()
                    .all(|relocation| relocation.instruction_index != index + 1) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    let [first_pair, second_pair] = shifted_or_pairs.as_slice() else {
        return None;
    };
    if *first_pair <= 16 || *second_pair <= first_pair + 2 {
        return None;
    }

    let epilogue = instructions.len().checked_sub(4)?;
    if !matches!(
        instructions.get(epilogue..),
        Some([
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ])
    ) {
        return None;
    }

    Some(PhysicalPlan {
        condition: 15,
        shifted_or_pairs: [*first_pair, *second_pair],
    })
}

impl Generator {
    pub(crate) fn finalize_linkage_first_instruction_array_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.frame_size != 32
            || !self.non_leaf
            || !self.callee_saved.is_empty()
            || self.callee_saved_float != 0
            || self.frame_slots.len() != 1
            || !self.frame_slots.values().any(|slot| {
                slot.offset == 8
                    && slot.size == 20
                    && slot.is_array
                    && matches!(slot.value_type, Type::Int | Type::UnsignedInt)
            })
        {
            return;
        }
        let Some(plan) = physical_plan(&self.output) else {
            return;
        };

        for first in plan.shifted_or_pairs.into_iter().rev() {
            let second_immediate = match self.output.instructions[first + 1] {
                Instruction::OrImmediateShifted { immediate, .. } => immediate,
                _ => unreachable!("the instruction-array plan owns an OR pair"),
            };
            let Instruction::OrImmediateShifted { immediate, .. } =
                &mut self.output.instructions[first]
            else {
                unreachable!("the instruction-array plan owns an OR pair")
            };
            *immediate |= second_immediate;
            crate::remove_instruction_retargeting_to_next(self, first + 1);
        }

        let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[3] else {
            unreachable!("the instruction-array plan owns its link store")
        };
        *offset = 4;

        crate::move_instruction_before_retargeting(self, 1, 0);
        crate::move_instruction_before_retargeting(self, 3, 1);
        crate::move_instruction_before_retargeting(self, plan.condition, 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation};

    fn candidate() -> MachineFunction {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 7,
                a: 6,
                immediate: 0,
            },
        ];
        for pair in 0..2i16 {
            instructions.extend([
                Instruction::LoadWord {
                    d: 6,
                    a: 7,
                    offset: pair * 8,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 7,
                    offset: pair * 8 + 4,
                },
                Instruction::StoreWord {
                    s: 6,
                    a: 1,
                    offset: 8 + pair * 8,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 12 + pair * 8,
                },
            ]);
        }
        instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 24,
            },
            Instruction::CompareWordImmediate { a: 5, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 23,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 21,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 0xf000,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::Branch { target: 27 },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 21,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 0xe000,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 8,
            },
            Instruction::BranchAndLink {
                target: "access".into(),
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        MachineFunction {
            instructions,
            relocations: vec![
                Relocation {
                    instruction_index: 2,
                    kind: RelocationKind::Addr16Ha,
                    target: RelocationTarget::AnonymousRodataAt(0),
                },
                Relocation {
                    instruction_index: 4,
                    kind: RelocationKind::Addr16Lo,
                    target: RelocationTarget::AnonymousRodataAt(0),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_the_complete_pool_copy_and_diamond() {
        assert_eq!(
            physical_plan(&candidate()),
            Some(PhysicalPlan {
                condition: 15,
                shifted_or_pairs: [18, 23],
            })
        );
    }

    #[test]
    fn rejects_a_branch_into_the_second_shifted_or() {
        let mut output = candidate();
        output.instructions[16] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 19,
        };

        assert_eq!(physical_plan(&output), None);
    }
}
