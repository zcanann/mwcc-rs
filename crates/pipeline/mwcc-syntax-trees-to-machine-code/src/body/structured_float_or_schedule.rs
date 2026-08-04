//! Build-163 register schedules for floating-point `||` boundaries.
//!
//! MWCC retains each group's value operands but rematerializes the shared zero
//! literal when control advances to the second group. Keeping this policy out
//! of the general condition cache preserves its dominance rules and confines
//! the physical register permutation to the complete measured shape.
//!
//! A homogeneous chain of member-vs-literal alternatives has a different
//! policy: the common literal remains in `f1`, the first member remains in
//! `f2`, and later members use `f0`. This is safe across the short-circuit
//! edges because every path reaching a later comparison has executed the first
//! literal load, while the decisive true edges leave the chain entirely.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_float_or_groups(&mut self) {
        while let Some(plan) = chained_float_literal_or(&self.output) {
            plan.apply(self);
        }

        let Some(start) = self
            .output
            .instructions
            .windows(20)
            .position(is_coalesced_float_or_groups)
        else {
            return;
        };
        let zero_index = start + 1;
        let insertion = start + 11;
        let zero_relocations: Vec<_> = self
            .output
            .relocations
            .iter()
            .filter(|relocation| relocation.instruction_index == zero_index)
            .cloned()
            .collect();
        if zero_relocations.is_empty() {
            return;
        }

        let zero_load = self.output.instructions[zero_index].clone();
        self.output.instructions.insert(insertion, zero_load);
        self.labels.inserted(insertion, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= insertion {
                relocation.instruction_index += 1;
            }
        }
        for mut relocation in zero_relocations {
            relocation.instruction_index = insertion;
            self.output.relocations.push(relocation);
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > insertion =>
                {
                    *target += 1;
                }
                _ => {}
            }
        }

        // MWCC's two persistent operands occupy f2 (member value) and f1
        // (zero), leaving f0 for the stack value and destructive sums.
        for (index, destination) in [(start, 2), (start + 1, 1), (insertion, 1)] {
            match &mut self.output.instructions[index] {
                Instruction::LoadFloatSingle { d, .. } => *d = destination,
                _ => unreachable!(),
            }
        }
        for index in [start + 2, start + 12] {
            match &mut self.output.instructions[index] {
                Instruction::FloatCompareOrdered { a, b } => {
                    *a = 2;
                    *b = 1;
                }
                _ => unreachable!(),
            }
        }
        for index in [start + 4, start + 14] {
            match &mut self.output.instructions[index] {
                Instruction::LoadFloatSingle { d, .. } => *d = 0,
                _ => unreachable!(),
            }
        }
        for index in [start + 5, start + 8, start + 15, start + 18] {
            match &mut self.output.instructions[index] {
                Instruction::FloatCompareOrdered { a, b } => {
                    *a = 0;
                    *b = 1;
                }
                _ => unreachable!(),
            }
        }
        for index in [start + 7, start + 17] {
            match &mut self.output.instructions[index] {
                Instruction::FloatAddSingle { d, a, b } => {
                    *d = 0;
                    *a = 0;
                    *b = 2;
                }
                _ => unreachable!(),
            }
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChainedFloatLiteralOr {
    start: usize,
    terms: usize,
}

impl ChainedFloatLiteralOr {
    fn apply(self, generator: &mut Generator) {
        let first_value = self.start;
        let first_literal = self.start + 1;
        let first_compare = self.start + 2;
        let Instruction::LoadFloatSingle { d, .. } =
            &mut generator.output.instructions[first_value]
        else {
            unreachable!("the chained float OR value was recognized")
        };
        *d = 2;
        let Instruction::LoadFloatSingle { d, .. } =
            &mut generator.output.instructions[first_literal]
        else {
            unreachable!("the chained float OR literal was recognized")
        };
        *d = 1;
        generator.output.instructions[first_compare] =
            Instruction::FloatCompareOrdered { a: 2, b: 1 };

        for term in 1..self.terms {
            let value = self.start + term * 4;
            let compare = value + 2;
            let Instruction::LoadFloatSingle { d, .. } =
                &mut generator.output.instructions[value]
            else {
                unreachable!("the chained float OR value was recognized")
            };
            *d = 0;
            generator.output.instructions[compare] =
                Instruction::FloatCompareOrdered { a: 0, b: 1 };
        }

        // Remove from the tail so the indices above remain those of the
        // recognized stream. The common removal helper updates branches,
        // labels, relocations, and every other instruction-index owner.
        for term in (1..self.terms).rev() {
            crate::remove_instruction_retargeting_to_next(
                generator,
                self.start + term * 4 + 1,
            );
        }
    }
}

fn chained_float_literal_or(
    output: &mwcc_machine_code::MachineFunction,
) -> Option<ChainedFloatLiteralOr> {
    for start in 0..output.instructions.len() {
        let Some((base, first_offset, options, condition_bit)) =
            chained_float_literal_term(output, start)
        else {
            continue;
        };
        let first_literal = start + 1;
        let mut terms = 1usize;
        loop {
            let next = start + terms * 4;
            let Some((next_base, next_offset, next_options, next_condition_bit)) =
                chained_float_literal_term(output, next)
            else {
                break;
            };
            if next_base != base
                || next_offset != first_offset.checked_add((terms as i16) * 4)?
                || next_condition_bit != condition_bit
                || !schedule_relocations::same_relocated_value(
                    &output.relocations,
                    &output.constants,
                    first_literal,
                    next + 1,
                )
            {
                break;
            }
            terms += 1;
            if next_options == (options ^ 8) {
                break;
            }
            if next_options != options {
                terms -= 1;
                break;
            }
        }
        if terms < 3 {
            continue;
        }
        let end = start + terms * 4;
        let intermediate_branches_match = (0..terms - 1).all(|term| {
            matches!(
                output.instructions[start + term * 4 + 3],
                Instruction::BranchConditionalForward {
                    options: branch_options,
                    condition_bit: branch_bit,
                    target,
                } if branch_options == options
                    && branch_bit == condition_bit
                    && target == end
            )
        });
        let final_branch_matches = matches!(
            output.instructions[end - 1],
            Instruction::BranchConditionalForward {
                options: branch_options,
                condition_bit: branch_bit,
                target,
            } if branch_options == (options ^ 8)
                && branch_bit == condition_bit
                && target > end
        );
        if intermediate_branches_match && final_branch_matches {
            return Some(ChainedFloatLiteralOr { start, terms });
        }
    }
    None
}

fn chained_float_literal_term(
    output: &mwcc_machine_code::MachineFunction,
    start: usize,
) -> Option<(u8, i16, u8, u8)> {
    let [value, literal, compare, branch] = output.instructions.get(start..start + 4)? else {
        return None;
    };
    let Instruction::LoadFloatSingle {
        d: 1,
        a: base,
        offset,
    } = value
    else {
        return None;
    };
    if *base == 0
        || !matches!(
            literal,
            Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            }
        )
        || !matches!(compare, Instruction::FloatCompareOrdered { a: 1, b: 0 })
    {
        return None;
    }
    let literal_is_pool_load = output.relocations.iter().any(|relocation| {
        relocation.instruction_index == start + 1
            && relocation.kind == RelocationKind::EmbSda21
    });
    if !literal_is_pool_load {
        return None;
    }
    let Instruction::BranchConditionalForward {
        options,
        condition_bit,
        ..
    } = branch
    else {
        return None;
    };
    Some((*base, *offset, *options, *condition_bit))
}

fn is_coalesced_float_or_groups(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::LoadFloatSingle { d: 1, .. },
        Instruction::LoadFloatSingle { d: 0, a: 0, .. },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { target: second_group_a, .. },
        Instruction::LoadFloatSingle { d: 2, a: stack_a, offset: stack_offset_a },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::BranchConditionalForward { target: second_group_b, .. },
        Instruction::FloatAddSingle { d: 2, a: 2, b: 1 },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::ConditionRegisterOr { .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::BranchConditionalForward { target: exit_a, .. },
        Instruction::LoadFloatSingle { d: 2, a: stack_b, offset: stack_offset_b },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::BranchConditionalForward { target: exit_b, .. },
        Instruction::FloatAddSingle { d: 1, a: 2, b: 1 },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::ConditionRegisterOr { .. },
        Instruction::BranchConditionalForward { .. },
    ] if second_group_a == second_group_b
        && stack_a == stack_b
        && stack_offset_a == stack_offset_b
        && exit_a == exit_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{PoolConstant, Relocation, RelocationTarget};

    #[test]
    fn recognizes_a_homogeneous_three_member_literal_or_chain() {
        let branch = |options, target| Instruction::BranchConditionalForward {
            options,
            condition_bit: 1,
            target,
        };
        let mut instructions = Vec::new();
        for (term, offset) in [4, 8, 12].into_iter().enumerate() {
            instructions.extend([
                Instruction::LoadFloatSingle {
                    d: 1,
                    a: 29,
                    offset,
                },
                Instruction::LoadFloatSingle {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::FloatCompareOrdered { a: 1, b: 0 },
                branch(if term == 2 { 4 } else { 12 }, if term == 2 { 15 } else { 12 }),
            ]);
        }
        instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 1,
            },
        ]);
        let output = mwcc_machine_code::MachineFunction {
            instructions,
            relocations: [1usize, 5, 9]
                .into_iter()
                .map(|instruction_index| Relocation {
                    instruction_index,
                    kind: RelocationKind::EmbSda21,
                    target: RelocationTarget::Constant(0),
                })
                .collect(),
            constants: vec![PoolConstant {
                bits: 1.0f32.to_bits().into(),
                byte_width: 4,
                static_slot: false,
                image: false,
                force_new: false,
            }],
            ..Default::default()
        };

        assert_eq!(
            chained_float_literal_or(&output),
            Some(ChainedFloatLiteralOr { start: 0, terms: 3 })
        );
    }

    #[test]
    fn recognizes_two_coalesced_float_or_groups() {
        let branch = |target| Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target,
        };
        let instructions = [
            Instruction::LoadFloatSingle { d: 1, a: 31, offset: 252 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            branch(11),
            Instruction::LoadFloatSingle { d: 2, a: 1, offset: 24 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            branch(11),
            Instruction::FloatAddSingle { d: 2, a: 2, b: 1 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 },
            branch(21),
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            branch(21),
            Instruction::LoadFloatSingle { d: 2, a: 1, offset: 24 },
            Instruction::FloatCompareOrdered { a: 2, b: 0 },
            branch(21),
            Instruction::FloatAddSingle { d: 1, a: 2, b: 1 },
            Instruction::FloatCompareOrdered { a: 1, b: 0 },
            Instruction::ConditionRegisterOr { d: 2, a: 0, b: 2 },
            branch(20),
        ];
        assert!(is_coalesced_float_or_groups(&instructions));
    }
}
