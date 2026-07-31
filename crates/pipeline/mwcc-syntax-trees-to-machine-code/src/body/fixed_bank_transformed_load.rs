//! Fold a single-use fixed-bank base adjustment into its transformed load.
//!
//! When a `lis`/`addi` base feeds one load and the immediately following
//! transform overwrites that base register, the adjusted address has no other
//! live use.  MWCC folds the low adjustment into the load displacement.

#[allow(unused_imports)]
use super::*;

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
    }, Instruction::ShiftLeftImmediate {
        a: transformed,
        s: transformed_value,
        ..
    }] = instructions.get(start..start + 4)?
    else {
        return None;
    };
    if high != adjusted
        || high != high_source
        || high != load_base
        || high != transformed
        || value != transformed_value
        || high == value
        || (start..start + 3).any(|index| owns_index(relocations, displacements, index))
    {
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
}
