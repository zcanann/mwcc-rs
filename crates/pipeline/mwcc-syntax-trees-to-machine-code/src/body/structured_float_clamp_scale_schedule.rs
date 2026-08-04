//! Physical schedule for a clamped reciprocal followed by member scaling.
//!
//! MWCC retains the reciprocal numerator in `f1`, uses `f2` for the clamped
//! denominator, and turns the false arm's repeated pool load into an `fmr`.
//! Recognizing the complete clamp and all three consumers keeps this lifetime
//! choice separate from the preceding maximum-selection schedule.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_float_clamp_scale(&mut self) {
        while let Some(plan) = ClampScalePlan::recognize(&self.output) {
            plan.apply(&mut self.output);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClampScalePlan {
    start: usize,
    base: u8,
    first_offset: i16,
    branch_options: u8,
    condition_bit: u8,
}

impl ClampScalePlan {
    fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Self> {
        for start in 0..output.instructions.len() {
            let Some(plan) = Self::recognize_at(output, start) else {
                continue;
            };
            return Some(plan);
        }
        None
    }

    fn recognize_at(
        output: &mwcc_machine_code::MachineFunction,
        start: usize,
    ) -> Option<Self> {
        let [
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            Instruction::BranchConditionalForward { options, condition_bit, target: false_arm },
            Instruction::Branch { target: join },
            Instruction::LoadFloatSingle { d: 2, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatDivideSingle { d: 2, a: 0, b: 2 },
            Instruction::LoadFloatSingle { d: 0, a: base, offset: first_offset },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 },
            Instruction::StoreFloatSingle { s: 0, a: first_store_base, offset: first_store_offset },
            Instruction::LoadFloatSingle { d: 0, a: second_base, offset: second_offset },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 },
            Instruction::StoreFloatSingle { s: 0, a: second_store_base, offset: second_store_offset },
            Instruction::LoadFloatSingle { d: 0, a: third_base, offset: third_offset },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 },
            Instruction::StoreFloatSingle { s: 0, a: third_store_base, offset: third_store_offset },
        ] = output.instructions.get(start..start + 16)?
        else {
            return None;
        };
        if *base == 0
            || *false_arm != start + 4
            || *join != start + 5
            || *first_store_base != *base
            || *first_store_offset != *first_offset
            || *second_base != *base
            || *second_store_base != *base
            || *second_store_offset != *second_offset
            || *third_base != *base
            || *third_store_base != *base
            || *third_store_offset != *third_offset
            || *second_offset != first_offset.checked_add(4)?
            || *third_offset != first_offset.checked_add(8)?
            || relocation_count(output, start) != 1
            || relocation_count(output, start + 4) != 1
            || relocation_count(output, start + 5) != 1
            || !schedule_relocations::same_relocated_value(
                &output.relocations,
                &output.constants,
                start,
                start + 4,
            )
            || schedule_relocations::same_relocated_value(
                &output.relocations,
                &output.constants,
                start,
                start + 5,
            )
        {
            return None;
        }
        Some(Self {
            start,
            base: *base,
            first_offset: *first_offset,
            branch_options: *options,
            condition_bit: *condition_bit,
        })
    }

    fn apply(self, output: &mut mwcc_machine_code::MachineFunction) {
        let false_arm = self.start + 5;
        let join = self.start + 6;
        output.instructions[self.start..self.start + 7].clone_from_slice(&[
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            Instruction::BranchConditionalForward {
                options: self.branch_options,
                condition_bit: self.condition_bit,
                target: false_arm,
            },
            Instruction::Branch { target: join },
            Instruction::FloatMove { d: 2, b: 0 },
            Instruction::FloatDivideSingle { d: 1, a: 1, b: 2 },
        ]);
        for multiply in [self.start + 8, self.start + 11, self.start + 14] {
            let Instruction::FloatMultiplySingle { c, .. } =
                &mut output.instructions[multiply]
            else {
                unreachable!("the clamp scale consumer was recognized")
            };
            *c = 1;
        }
        output
            .relocations
            .retain(|relocation| relocation.instruction_index != self.start + 4);
        for relocation in &mut output.relocations {
            if relocation.instruction_index == self.start + 5 {
                relocation.instruction_index = self.start + 1;
            }
        }
        output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn relocation_count(output: &mwcc_machine_code::MachineFunction, at: usize) -> usize {
    output
        .relocations
        .iter()
        .filter(|relocation| relocation.instruction_index == at)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{PoolConstant, Relocation, RelocationTarget};

    #[test]
    fn schedules_a_clamped_reciprocal_across_three_member_scales() {
        let branch = Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 4,
        };
        let mut instructions = vec![
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            branch,
            Instruction::Branch { target: 5 },
            Instruction::LoadFloatSingle { d: 2, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatDivideSingle { d: 2, a: 0, b: 2 },
        ];
        for offset in [4, 8, 12] {
            instructions.extend([
                Instruction::LoadFloatSingle { d: 0, a: 28, offset },
                Instruction::FloatMultiplySingle { d: 0, a: 0, c: 2 },
                Instruction::StoreFloatSingle { s: 0, a: 28, offset },
            ]);
        }
        let mut output = mwcc_machine_code::MachineFunction {
            instructions,
            relocations: [(0, 0), (4, 0), (5, 1)]
                .into_iter()
                .map(|(instruction_index, constant)| Relocation {
                    instruction_index,
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::Constant(constant),
                })
                .collect(),
            constants: vec![
                PoolConstant {
                    bits: 0.01f32.to_bits().into(),
                    byte_width: 4,
                    static_slot: false,
                    image: false,
                    force_new: false,
                },
                PoolConstant {
                    bits: 1.0f32.to_bits().into(),
                    byte_width: 4,
                    static_slot: false,
                    image: false,
                    force_new: false,
                },
            ],
            ..Default::default()
        };

        let plan = ClampScalePlan::recognize(&output).expect("clamp scale shape");
        plan.apply(&mut output);

        assert!(matches!(
            output.instructions[1],
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 }
        ));
        assert!(matches!(
            output.instructions[5],
            Instruction::FloatMove { d: 2, b: 0 }
        ));
        assert!(matches!(
            output.instructions[6],
            Instruction::FloatDivideSingle { d: 1, a: 1, b: 2 }
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        for multiply in [8, 11, 14] {
            assert!(matches!(
                output.instructions[multiply],
                Instruction::FloatMultiplySingle { c: 1, .. }
            ));
        }
    }
}
