//! Late physical scheduling for formatter character arguments.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

fn external_name(target: &RelocationTarget) -> Option<&str> {
    match target {
        RelocationTarget::External(name) => Some(name),
        _ => None,
    }
}

impl Generator {
    /// Reassociate a saved call result's additive character bias while
    /// overlapping the formatter's destination and packed-string addresses.
    pub(crate) fn schedule_saved_character_formatter_arguments(&mut self) {
        schedule_saved_character_formatter_arguments(&mut self.output);
    }
}

fn schedule_saved_character_formatter_arguments(
    output: &mut mwcc_machine_code::MachineFunction,
) -> bool {
    let Some(start) = output.instructions.windows(8).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate { d: 5, a: 3, .. },
                Instruction::AddImmediate { d: 5, a: 5, .. },
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: 0
                },
                Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0
                },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { .. }
            ]
        )
    }) else {
        return false;
    };
    let first_bias = match &output.instructions[start] {
        Instruction::AddImmediate { immediate, .. } => *immediate,
        _ => unreachable!(),
    };
    let second_bias = match &output.instructions[start + 1] {
        Instruction::AddImmediate { immediate, .. } => *immediate,
        _ => unreachable!(),
    };
    let Some(bias) = first_bias.checked_add(second_bias) else {
        return false;
    };
    let relocated = |index: usize, kind: RelocationKind| {
        output
            .relocations
            .iter()
            .find(|relocation| {
                relocation.instruction_index == index && relocation.kind == kind
            })
            .map(|relocation| relocation.target.clone())
    };
    let (Some(array_high), Some(array_low), Some(string_high), Some(string_low)) = (
        relocated(start + 2, RelocationKind::Addr16Ha),
        relocated(start + 3, RelocationKind::Addr16Lo),
        relocated(start + 4, RelocationKind::Addr16Ha),
        relocated(start + 5, RelocationKind::Addr16Lo),
    )
    else {
        return false;
    };
    if external_name(&array_high) != external_name(&array_low)
        || external_name(&string_high) != external_name(&string_low)
        || external_name(&array_high) == external_name(&string_high)
        || external_name(&array_high).is_none()
        || external_name(&string_high).is_none()
        || output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if (start..start + 6).contains(target)
            )
        })
    {
        return false;
    }

    let Instruction::AddImmediateShifted {
        immediate: array_high_immediate,
        ..
    } = output.instructions[start + 2]
    else {
        unreachable!()
    };
    let Instruction::AddImmediateShifted {
        immediate: string_high_immediate,
        ..
    } = output.instructions[start + 4]
    else {
        unreachable!()
    };
    output.instructions[start..start + 6].clone_from_slice(&[
        Instruction::AddImmediateShifted {
            d: 5,
            a: 0,
            immediate: array_high_immediate,
        },
        Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            immediate: string_high_immediate,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: bias,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        },
        Instruction::move_register(3, 0),
    ]);
    for relocation in &mut output.relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == start + 2 => start,
            index if index == start + 3 => start + 2,
            index if index == start + 4 => start + 1,
            index if index == start + 5 => start + 4,
            index => index,
        };
    }
    true
}

#[cfg(test)]
mod tests {
    use super::schedule_saved_character_formatter_arguments;
    use mwcc_machine_code::{
        Instruction, MachineFunction, Relocation, RelocationKind,
        RelocationTarget,
    };

    #[test]
    fn folds_the_character_bias_while_scheduling_both_addresses() {
        let mut output = MachineFunction::new("probe");
        output.instructions = vec![
            Instruction::AddImmediate {
                d: 5,
                a: 3,
                immediate: -1,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 65,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "format".into(),
            },
        ];
        for (instruction_index, kind, target) in [
            (2, RelocationKind::Addr16Ha, "buffer"),
            (3, RelocationKind::Addr16Lo, "buffer"),
            (4, RelocationKind::Addr16Ha, "@stringBase0"),
            (5, RelocationKind::Addr16Lo, "@stringBase0"),
        ] {
            output.relocations.push(Relocation {
                instruction_index,
                kind,
                target: RelocationTarget::External(target.into()),
            });
        }

        assert!(schedule_saved_character_formatter_arguments(&mut output));
        assert!(matches!(
            output.instructions[3],
            Instruction::AddImmediate {
                d: 5,
                a: 3,
                immediate: 64
            }
        ));
        assert!(matches!(
            output.instructions[5],
            Instruction::Or { a: 3, s: 0, b: 0 }
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [0, 2, 1, 4]
        );
    }
}
