//! Reuse comparison literals on float-clamp assignment edges.
//!
//! A literal loaded into an FPR for `value < bound` remains available on the
//! fallthrough edge that assigns that same bound to `value`. Optimized MWCC
//! emits an `fmr` there; retaining the literal avoids a redundant pool
//! relocation without extending its lifetime beyond the clamp diamond.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationTarget};

impl Generator {
    pub(crate) fn reuse_structured_float_clamp_literals(&mut self) -> usize {
        let plans = plans(&self.output);
        for plan in &plans {
            self.output.instructions[plan.assignment] = Instruction::FloatMove {
                d: plan.value,
                b: plan.literal,
            };
            self.output.relocations.retain(|relocation| {
                relocation.instruction_index != plan.assignment
            });
        }
        plans.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    assignment: usize,
    value: u8,
    literal: u8,
}

fn plans(output: &MachineFunction) -> Vec<Plan> {
    let mut plans = output
        .instructions
        .windows(4)
        .enumerate()
        .filter_map(|(start, window)| {
            let [
                Instruction::LoadFloatSingle {
                    d: literal,
                    a: 0,
                    offset: 0,
                },
                Instruction::FloatCompareOrdered { a: value, b: compared },
                Instruction::BranchConditionalForward { target, .. },
                Instruction::LoadFloatSingle {
                    d: assigned,
                    a: 0,
                    offset: 0,
                },
            ] = window
            else {
                return None;
            };
            let assignment = start + 3;
            if literal != compared
                || value != assigned
                || *target <= assignment
                || !same_constant_relocation(output, start, assignment)
            {
                return None;
            }
            Some(Plan {
                assignment,
                value: *value,
                literal: *literal,
            })
        })
        .collect::<Vec<_>>();
    plans.extend(
        output
            .instructions
            .windows(7)
            .enumerate()
            .filter_map(|(start, window)| delayed_product_plan(output, start, window)),
    );
    plans.sort_unstable_by_key(|plan| plan.assignment);
    plans.dedup_by_key(|plan| plan.assignment);
    plans
}

/// The effecter product fills both the literal-to-compare and compare-to-branch
/// issue slots.  The literal remains live because neither product instruction
/// nor the intervening member load writes its FPR.
fn delayed_product_plan(
    output: &MachineFunction,
    start: usize,
    window: &[Instruction],
) -> Option<Plan> {
    let [
        Instruction::LoadFloatSingle { d: literal, a: 0, offset: 0 },
        Instruction::FloatMultiplySingle { d: first_product, .. },
        Instruction::LoadFloatSingle { d: third_factor, .. },
        Instruction::FloatCompareOrdered { a: value, b: compared },
        Instruction::FloatMultiplySingle { d: final_product, .. },
        Instruction::BranchConditionalForward { target, .. },
        Instruction::LoadFloatSingle { d: assigned, a: 0, offset: 0 },
    ] = window
    else {
        return None;
    };
    let assignment = start + 6;
    (literal == compared
        && value == assigned
        && first_product != literal
        && third_factor != literal
        && final_product != literal
        && *target > assignment
        && same_constant_relocation(output, start, assignment))
        .then_some(Plan {
            assignment,
            value: *value,
            literal: *literal,
        })
}

fn same_constant_relocation(output: &MachineFunction, left: usize, right: usize) -> bool {
    let constant = |instruction_index| {
        output.relocations.iter().find_map(|relocation| {
            match &relocation.target {
                RelocationTarget::Constant(constant)
                    if relocation.instruction_index == instruction_index
                        && relocation.kind == RelocationKind::EmbSda21 =>
                {
                    Some(*constant)
                }
                _ => None,
            }
        })
    };
    constant(left).is_some_and(|left| constant(right) == Some(left))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    #[test]
    fn replaces_a_clamp_reload_with_the_compared_literal() {
        let mut output = MachineFunction::default();
        let zero = output.intern_constant(u64::from(0.0f32.to_bits()), 4);
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 31, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 5 },
            Instruction::LoadFloatSingle { d: 31, a: 0, offset: 0 },
            Instruction::Branch { target: 6 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 1 },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(zero),
            },
            Relocation {
                instruction_index: 3,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(zero),
            },
        ];

        assert_eq!(
            plans(&output),
            vec![Plan { assignment: 3, value: 31, literal: 0 }]
        );
    }

    #[test]
    fn rejects_a_different_assignment_literal() {
        let mut output = MachineFunction::default();
        let zero = output.intern_constant(u64::from(0.0f32.to_bits()), 4);
        let one = output.intern_constant(u64::from(1.0f32.to_bits()), 4);
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatCompareOrdered { a: 31, b: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 4 },
            Instruction::LoadFloatSingle { d: 31, a: 0, offset: 0 },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(zero),
            },
            Relocation {
                instruction_index: 3,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(one),
            },
        ];

        assert!(plans(&output).is_empty());
    }

    #[test]
    fn reuses_a_literal_across_product_issue_slots() {
        let mut output = MachineFunction::default();
        let zero = output.intern_constant(u64::from(0.0f32.to_bits()), 4);
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 },
            Instruction::LoadFloatSingle { d: 2, a: 30, offset: 260 },
            Instruction::FloatCompareOrdered { a: 31, b: 0 },
            Instruction::FloatMultiplySingle { d: 30, a: 2, c: 1 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 0, target: 8 },
            Instruction::LoadFloatSingle { d: 31, a: 0, offset: 0 },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(zero),
            },
            Relocation {
                instruction_index: 6,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(zero),
            },
        ];

        assert_eq!(
            plans(&output),
            vec![Plan { assignment: 6, value: 31, literal: 0 }]
        );
    }
}
