//! Fold a single-use fixed-bank base adjustment into its transformed load.
//!
//! When a `lis`/`addi` base feeds one load and the immediately following
//! transform either overwrites that base register or proves it dead on every
//! outgoing path, the adjusted address has no other live use. MWCC folds the
//! low adjustment into the load displacement.

#[allow(unused_imports)]
use super::*;
use mwcc_vreg::{register_operands, Class, RegisterRole};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TransformedLoad {
    add: usize,
    folded_offset: i16,
}

fn owns_index(
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
    instruction_index: usize,
) -> bool {
    relocations
        .iter()
        .any(|relocation| relocation.instruction_index == instruction_index)
        || displacements
            .iter()
            .any(|displacement| displacement.instruction_index == instruction_index)
}

fn general_value_dies_before_use_on_all_paths(
    instructions: &[Instruction],
    start: usize,
    register: u8,
) -> bool {
    let mut pending = vec![start];
    let mut visited = vec![false; instructions.len()];
    while let Some(index) = pending.pop() {
        let Some(instruction) = instructions.get(index) else {
            return false;
        };
        if visited[index] {
            continue;
        }
        visited[index] = true;

        let operands = register_operands(instruction);
        if operands.iter().any(|operand| {
            operand.class == Class::General
                && operand.register == register
                && operand.role == RegisterRole::Use
        }) {
            return false;
        }
        if operands.iter().any(|operand| {
            operand.class == Class::General
                && operand.register == register
                && operand.role == RegisterRole::Define
        }) {
            continue;
        }

        match instruction {
            Instruction::BranchConditionalForward { target, .. } => {
                pending.push(index + 1);
                pending.push(*target);
            }
            Instruction::Branch { target } => pending.push(*target),
            Instruction::BranchAndLink { .. }
            | Instruction::BranchExternal { .. }
            | Instruction::BranchConditionalToLinkRegister { .. }
            | Instruction::BranchToLinkRegister
            | Instruction::BranchToLinkRegisterAndLink
            | Instruction::BranchToCountRegister
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::ReturnFromInterrupt
            | Instruction::SystemCall
            | Instruction::VerbatimWord(_) => return false,
            _ => pending.push(index + 1),
        }
    }
    true
}

fn recognize_at(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    displacements: &[mwcc_machine_code::DataSectionDisplacement],
    start: usize,
) -> Option<TransformedLoad> {
    let [Instruction::AddImmediateShifted { d: high, a: 0, .. }, Instruction::AddImmediate {
        d: adjusted,
        a: high_source,
        immediate,
    }, Instruction::LoadWord {
        d: value,
        a: load_base,
        offset,
    }] = instructions.get(start..start + 3)?
    else {
        return None;
    };
    if high != adjusted
        || high != high_source
        || high != load_base
        || high == value
        || (start..start + 3).any(|index| owns_index(relocations, displacements, index))
    {
        return None;
    }
    let transformed_base_is_dead = match instructions.get(start + 3)? {
        Instruction::ShiftLeftImmediate {
            a: transformed,
            s: transformed_value,
            ..
        } => high == transformed && value == transformed_value,
        Instruction::ClearLeftImmediateRecord {
            a: transformed,
            s: transformed_value,
            ..
        }
        | Instruction::AndMaskRecord {
            a: transformed,
            s: transformed_value,
            ..
        } => {
            value == transformed
                && value == transformed_value
                && general_value_dies_before_use_on_all_paths(instructions, start + 4, *high)
        }
        _ => false,
    };
    if !transformed_base_is_dead {
        return None;
    }
    let folded_offset = i32::from(*immediate)
        .checked_add(i32::from(*offset))
        .and_then(|offset| i16::try_from(offset).ok())?;
    Some(TransformedLoad {
        add: start + 1,
        folded_offset,
    })
}

impl Generator {
    pub(crate) fn fold_fixed_bank_transformed_loads(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 4 <= self.output.instructions.len() {
            let Some(plan) = recognize_at(
                &self.output.instructions,
                &self.output.relocations,
                &self.output.data_section_displacements,
                start,
            ) else {
                start += 1;
                continue;
            };

            let Instruction::LoadWord { offset, .. } = &mut self.output.instructions[plan.add + 1]
            else {
                unreachable!("validated fixed-bank load changed form")
            };
            *offset = plan.folded_offset;
            crate::remove_instruction_retargeting_to_next(self, plan.add);
            start += 3;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_base_overwritten_by_the_loaded_value_transform() {
        let instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: -13312,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 24576,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 32,
            },
            Instruction::ShiftLeftImmediate {
                a: 3,
                s: 0,
                shift: 2,
            },
        ];

        assert_eq!(
            recognize_at(&instructions, &[], &[], 0),
            Some(TransformedLoad {
                add: 1,
                folded_offset: 24608,
            })
        );
    }

    #[test]
    fn preserves_a_materialized_base_that_survives_the_transform() {
        let instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: -13312,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 24576,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 32,
            },
            Instruction::ShiftLeftImmediate {
                a: 4,
                s: 0,
                shift: 2,
            },
        ];

        assert_eq!(recognize_at(&instructions, &[], &[], 0), None);
    }

    #[test]
    fn folds_a_base_dead_on_both_sides_of_a_record_transform_branch() {
        let instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: -13312,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 24576,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 32,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 64,
            },
            Instruction::Branch { target: 8 },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(
            recognize_at(&instructions, &[], &[], 0),
            Some(TransformedLoad {
                add: 1,
                folded_offset: 24608,
            })
        );
    }

    #[test]
    fn preserves_a_base_read_on_one_record_transform_path() {
        let mut instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: -13312,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 24576,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 32,
            },
            Instruction::AndMaskRecord {
                a: 0,
                s: 0,
                begin: 31,
                end: 31,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 7,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 64,
            },
            Instruction::Branch { target: 8 },
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: 0,
            },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(recognize_at(&instructions, &[], &[], 0), None);
        instructions[7] = Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        };
        assert!(recognize_at(&instructions, &[], &[], 0).is_some());
    }
}
