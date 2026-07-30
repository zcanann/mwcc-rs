//! Hoist a repeatedly compared floating zero out of a call-making loop.
//!
//! Re-emitting the same immutable pool load on each control-flow path hides the
//! value's true loop lifetime from allocation. Build 163 loads it once before
//! the initial jump to the loop latch and retains it in a callee-saved FPR.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

struct LoopFloatZeroPlan {
    entry_branch: usize,
    loads: Vec<usize>,
}

impl Generator {
    pub(crate) fn hoist_structured_loop_float_zero(&mut self) -> bool {
        let Some(plan) = loop_float_zero_plan(&self.output) else {
            return false;
        };
        let shared = self.fresh_virtual_float_preferring(31);

        for &load in &plan.loads {
            let Instruction::LoadFloatSingle { d, .. } = &mut self.output.instructions[load] else {
                unreachable!("loop-zero load changed after recognition")
            };
            let old = *d;
            *d = shared;
            match &mut self.output.instructions[load + 1] {
                Instruction::FloatCompareOrdered { a, b }
                | Instruction::FloatCompareUnordered { a, b } => {
                    if *a == old {
                        *a = shared;
                    } else {
                        debug_assert_eq!(*b, old);
                        *b = shared;
                    }
                }
                _ => unreachable!("loop-zero compare changed after recognition"),
            }
        }

        for &load in plan.loads.iter().skip(1).rev() {
            crate::remove_instruction_retargeting_to_next(self, load);
        }
        move_instruction_before_with_indices(self, plan.loads[0], plan.entry_branch);
        true
    }
}

fn loop_float_zero_plan(output: &mwcc_machine_code::MachineFunction) -> Option<LoopFloatZeroPlan> {
    let instructions = &output.instructions;
    let (entry_branch, loop_end) =
        instructions
            .iter()
            .enumerate()
            .find_map(|(index, instruction)| match instruction {
                Instruction::Branch { target }
                    if *target > index + 1 && *target <= instructions.len() =>
                {
                    Some((index, *target))
                }
                _ => None,
            })?;
    let loop_body = entry_branch + 1;
    if !instructions[loop_end..].iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                if *target == loop_body
        )
    }) || !instructions[loop_body..loop_end]
        .iter()
        .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
    {
        return None;
    }

    let loads: Vec<_> = (loop_body..loop_end.saturating_sub(1))
        .filter(|&index| loop_zero_compare(output, index))
        .collect();
    if loads.len() < 4 {
        return None;
    }
    let first = loads[0];
    if loads.iter().skip(1).any(|&index| {
        !super::super::schedule_relocations::same_target_value(
            &output.relocations,
            &output.constants,
            first,
            index,
        )
    }) {
        return None;
    }
    Some(LoopFloatZeroPlan {
        entry_branch,
        loads,
    })
}

fn loop_zero_compare(output: &mwcc_machine_code::MachineFunction, index: usize) -> bool {
    let Instruction::LoadFloatSingle { d: zero, .. } = output.instructions[index] else {
        return false;
    };
    let compares_loaded_value = matches!(
        output.instructions.get(index + 1),
        Some(
            Instruction::FloatCompareOrdered { a, b }
                | Instruction::FloatCompareUnordered { a, b }
        ) if *a == zero || *b == zero
    );
    compares_loaded_value
        && output.relocations.iter().any(|relocation| {
            relocation.instruction_index == index
                && relocation.kind == RelocationKind::EmbSda21
                && matches!(
                    relocation.target,
                    RelocationTarget::Constant(constant)
                        if output.constants.get(constant).is_some_and(|constant| {
                            constant.byte_width == 4 && constant.bits == 0
                        })
                )
        })
}

fn move_instruction_before_with_indices(generator: &mut Generator, from: usize, to: usize) {
    debug_assert!(to < from);
    let old_len = generator.output.instructions.len();
    let instruction = generator.output.instructions.remove(from);
    generator.output.instructions.insert(to, instruction);
    generator.labels.moved_before(from, to);
    let permutation: Vec<_> = (0..old_len)
        .map(|index| {
            if index == from {
                to
            } else if (to..from).contains(&index) {
                index + 1
            } else {
                index
            }
        })
        .collect();
    crate::remap_instruction_indices(generator, &permutation);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{PoolConstant, Relocation};

    #[test]
    fn recognizes_four_zero_compares_in_a_call_making_loop() {
        let mut output = mwcc_machine_code::MachineFunction::new("loop");
        output.constants.push(PoolConstant {
            bits: 0,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
        });
        output.instructions.push(Instruction::Branch { target: 12 });
        output.instructions.push(Instruction::BranchAndLink {
            target: "consume".into(),
        });
        for register in 1..=4 {
            let load = output.instructions.len();
            output.instructions.push(Instruction::LoadFloatSingle {
                d: register,
                a: 0,
                offset: 0,
            });
            output
                .instructions
                .push(Instruction::FloatCompareOrdered { a: 0, b: register });
            output.relocations.push(Relocation {
                instruction_index: load,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::Constant(0),
            });
        }
        output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 1,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 1,
            },
        ]);

        let plan = loop_float_zero_plan(&output).expect("eligible loop");

        assert_eq!(plan.entry_branch, 0);
        assert_eq!(plan.loads, [2, 4, 6, 8]);
    }

    #[test]
    fn ignores_unresolved_forward_branch_placeholders() {
        let mut output = mwcc_machine_code::MachineFunction::new("partial");
        output.instructions.push(Instruction::Branch {
            target: usize::MAX / 4,
        });
        output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        assert!(loop_float_zero_plan(&output).is_none());
    }
}
