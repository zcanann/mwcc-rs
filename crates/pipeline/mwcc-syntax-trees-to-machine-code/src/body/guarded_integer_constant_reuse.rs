//! Retained integer constants shared by a direct store and one guarded store.
//!
//! Ordinary store lowering materializes each constant into r0. MWCC recognizes
//! a direct store followed by a zero test and a guarded store of the same value,
//! keeps that value in r4 across the condition, and removes the arm-local
//! rematerialization. A call immediately before the region proves r4 is fresh;
//! calls on both outgoing paths prove the retained value dies in the region.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Plan {
    first_load: usize,
    condition_load: usize,
    second_load: usize,
    saved_result: Option<usize>,
    scratch: u8,
    immediate: i16,
}

impl Generator {
    pub(crate) fn reuse_guarded_integer_constant(&mut self) {
        while let Some(plan) = recognize(&self.output) {
            let retained = Eabi::FIRST_GENERAL_ARGUMENT + 1;
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[plan.first_load]
            else {
                unreachable!("the guarded constant plan owns its first load");
            };
            *d = retained;
            for store in [plan.first_load + 1, plan.second_load + 1] {
                let Instruction::StoreWord { s, .. } = &mut self.output.instructions[store] else {
                    unreachable!("the guarded constant plan owns both stores");
                };
                *s = retained;
            }
            crate::remove_instruction_retargeting_to_next(self, plan.second_load);

            if let Some(saved) = plan.saved_result {
                // `saved; li; stw` -> `li; stw; saved`. The retained constant
                // store fills the call-result copy's issue slot.
                move_instruction_before(self, plan.first_load, saved);
                move_instruction_before(self, plan.first_load + 1, plan.first_load);
                let saved = plan.first_load + 1;
                let destination = match self.output.instructions[saved] {
                    Instruction::AddImmediate {
                        d,
                        a: Eabi::FIRST_GENERAL_ARGUMENT,
                        immediate: 0,
                    }
                    | Instruction::Or {
                        a: d,
                        s: Eabi::FIRST_GENERAL_ARGUMENT,
                        b: Eabi::FIRST_GENERAL_ARGUMENT,
                    } => d,
                    _ => unreachable!("the guarded plan owns the saved result copy"),
                };
                // This is a control-flow-preservation copy, not a straight-line
                // integer materialization; build 163 also spells it as `mr`.
                self.output.instructions[saved] =
                    Instruction::move_register(destination, Eabi::FIRST_GENERAL_ARGUMENT);
            } else {
                // With no result home competing for the issue slot, MWCC starts
                // the condition's independent SDA load before materializing and
                // storing the retained value.
                move_instruction_before(self, plan.condition_load, plan.first_load);
            }
        }
    }
}

fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Plan> {
    output
        .instructions
        .windows(7)
        .enumerate()
        .find_map(|(first_load, window)| {
            let [
                Instruction::AddImmediate {
                    d: scratch,
                    a: 0,
                    immediate,
                },
                Instruction::StoreWord {
                    s: first_source,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: condition,
                    a: 0,
                    offset: 0,
                },
                comparison,
                Instruction::BranchConditionalForward { target, .. },
                Instruction::AddImmediate {
                    d: repeated_scratch,
                    a: 0,
                    immediate: repeated_immediate,
                },
                Instruction::StoreWord {
                    s: second_source,
                    a: 0,
                    offset: 0,
                },
            ] = window
            else {
                return None;
            };
            let zero_compare = matches!(
                comparison,
                Instruction::CompareWordImmediate {
                    a,
                    immediate: 0
                } if a == condition
            ) || matches!(
                comparison,
                Instruction::CompareLogicalWordImmediate { a, immediate: 0 }
                    if a == condition
            );
            if *scratch != 0
                || scratch != first_source
                || scratch != condition
                || scratch != repeated_scratch
                || scratch != second_source
                || immediate != repeated_immediate
                || !zero_compare
                || !((first_load + 7)..=(first_load + 8)).contains(target)
                || *target >= output.instructions.len()
            {
                return None;
            }
            let saved_result = saved_result_before(&output.instructions, first_load)?;
            if saved_result.is_none()
                && !matches!(
                    output.instructions.get(first_load.wrapping_sub(1)),
                    Some(Instruction::BranchAndLink { .. })
                )
            {
                return None;
            }
            if has_alternate_entry(
                &output.instructions,
                first_load..first_load + 7,
                first_load + 4,
            ) || !has_expected_sda_relocations(output, first_load)
                || !dies_at_call(&output.instructions, first_load + 7, 4)
                || !dies_at_call(&output.instructions, *target, 4)
            {
                return None;
            }
            Some(Plan {
                first_load,
                condition_load: first_load + 2,
                second_load: first_load + 5,
                saved_result,
                scratch: *scratch,
                immediate: *immediate,
            })
        })
}

fn saved_result_before(instructions: &[Instruction], first_load: usize) -> Option<Option<usize>> {
    let saved = first_load.checked_sub(1)?;
    let destination = match instructions[saved] {
        Instruction::AddImmediate {
            d,
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            immediate: 0,
        }
        | Instruction::Or {
            a: d,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            b: Eabi::FIRST_GENERAL_ARGUMENT,
        } if (14..=31).contains(&d) => d,
        _ => return Some(None),
    };
    let call = saved.checked_sub(1)?;
    matches!(instructions[call], Instruction::BranchAndLink { .. })
        .then_some(Some(saved))
        .filter(|_| destination >= 14)
}

fn has_expected_sda_relocations(
    output: &mwcc_machine_code::MachineFunction,
    first_load: usize,
) -> bool {
    let indices = [first_load + 1, first_load + 2, first_load + 6];
    let targets: Vec<_> = output
        .relocations
        .iter()
        .filter(|relocation| (first_load..first_load + 7).contains(&relocation.instruction_index))
        .collect();
    if targets.len() != indices.len() {
        return false;
    }
    let resolved: Option<Vec<&str>> = indices
        .iter()
        .map(|&index| {
            output.relocations.iter().find_map(|relocation| {
                if relocation.instruction_index != index
                    || relocation.kind != RelocationKind::EmbSda21
                {
                    return None;
                }
                let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target
                else {
                    return None;
                };
                Some(target.as_str())
            })
        })
        .collect();
    resolved.is_some_and(|targets| targets[0] != targets[2])
}

fn has_alternate_entry(
    instructions: &[Instruction],
    region: std::ops::Range<usize>,
    owned_branch: usize,
) -> bool {
    instructions.iter().enumerate().any(|(index, instruction)| {
        index != owned_branch
            && matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if region.contains(target)
            )
    })
}

fn dies_at_call(instructions: &[Instruction], start: usize, retained: u8) -> bool {
    for instruction in instructions.iter().skip(start).take(3) {
        if matches!(instruction, Instruction::BranchAndLink { .. }) {
            return true;
        }
        if matches!(
            instruction,
            Instruction::BranchConditionalForward { .. } | Instruction::Branch { .. }
        ) || mwcc_vreg::register_operands(instruction)
            .iter()
            .any(|operand| {
                operand.class == mwcc_vreg::Class::General && operand.register == retained
            })
        {
            return false;
        }
    }
    false
}

fn move_instruction_before(generator: &mut Generator, from: usize, to: usize) {
    debug_assert!(to < from);
    let instruction = generator.output.instructions.remove(from);
    generator.output.instructions.insert(to, instruction);
    generator.labels.moved_before(from, to);
    let permutation: Vec<_> = (0..generator.output.instructions.len())
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
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn sda(index: usize, target: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::External(target.into()),
        }
    }

    fn pause_output() -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::BranchAndLink {
                    target: "disable".into(),
                },
                Instruction::load_immediate(0, 1),
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target: 8,
                },
                Instruction::load_immediate(0, 1),
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::BranchAndLink {
                    target: "restore".into(),
                },
            ],
            relocations: vec![sda(2, "pause"), sda(3, "executing"), sda(7, "pausing")],
            ..Default::default()
        }
    }

    fn resume_output() -> mwcc_machine_code::MachineFunction {
        mwcc_machine_code::MachineFunction {
            instructions: vec![
                Instruction::BranchAndLink {
                    target: "disable".into(),
                },
                Instruction::AddImmediate {
                    d: 31,
                    a: 3,
                    immediate: 0,
                },
                Instruction::load_immediate(0, 0),
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::CompareWordImmediate { a: 0, immediate: 0 },
                Instruction::BranchConditionalForward {
                    options: 12,
                    condition_bit: 2,
                    target: 10,
                },
                Instruction::load_immediate(0, 0),
                Instruction::StoreWord {
                    s: 0,
                    a: 0,
                    offset: 0,
                },
                Instruction::BranchAndLink {
                    target: "ready".into(),
                },
                Instruction::move_register(3, 31),
                Instruction::BranchAndLink {
                    target: "restore".into(),
                },
            ],
            relocations: vec![sda(3, "pause"), sda(4, "pausing"), sda(8, "pausing")],
            ..Default::default()
        }
    }

    #[test]
    fn recognizes_a_guarded_repeated_constant_after_a_call() {
        assert_eq!(
            recognize(&pause_output()),
            Some(Plan {
                first_load: 1,
                condition_load: 3,
                second_load: 6,
                saved_result: None,
                scratch: 0,
                immediate: 1,
            })
        );
    }

    #[test]
    fn rejects_a_region_without_calls_killing_the_retained_register() {
        let mut output = pause_output();
        output.instructions[8] = Instruction::BranchToLinkRegister;
        assert_eq!(recognize(&output), None);
    }

    #[test]
    fn recognizes_the_saved_result_variant() {
        assert_eq!(
            recognize(&resume_output()),
            Some(Plan {
                first_load: 2,
                condition_load: 4,
                second_load: 7,
                saved_result: Some(1),
                scratch: 0,
                immediate: 0,
            })
        );
    }
}
