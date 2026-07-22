//! Dependency-aware load ordering for floating-point comparisons.
//!
//! Legacy MWCC normally evaluates the value before a pool literal.  When the
//! value is a member behind a freshly loaded pointer, however, it issues the
//! independent literal load between the pointer load and the dependent member
//! load.  Keeping this machine-level schedule separate from comparison
//! selection lets the selector describe source operands while this module
//! fills the measured load-use slot.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, MachineFunction, RelocationTarget};

impl Generator {
    pub(crate) fn schedule_float_literal_in_dependent_load_gap(&mut self) {
        schedule_tail(&mut self.output);
    }
}

fn schedule_tail(output: &mut MachineFunction) {
    let instruction_count = output.instructions.len();
    if instruction_count < 3 {
        return;
    }

    let literal_position = instruction_count - 1;
    let value_position = literal_position - 1;
    let base_position = value_position - 1;
    let value_base = match &output.instructions[value_position] {
        Instruction::LoadFloatSingle { a, .. } | Instruction::LoadFloatDouble { a, .. }
            if *a != 0 =>
        {
            *a
        }
        _ => return,
    };
    if !matches!(
        output.instructions[literal_position],
        Instruction::LoadFloatSingle { a: 0, .. } | Instruction::LoadFloatDouble { a: 0, .. }
    ) || !matches!(
        output.instructions[base_position],
        Instruction::LoadWord { d, .. } if d == value_base
    ) {
        return;
    }

    let literal_has_pool_relocation = output.relocations.iter().any(|relocation| {
        relocation.instruction_index == literal_position
            && matches!(
                relocation.target,
                RelocationTarget::Constant(_) | RelocationTarget::ConstantWithAddend(_, _)
            )
    });
    let value_has_relocation = output
        .relocations
        .iter()
        .any(|relocation| relocation.instruction_index == value_position);
    if !literal_has_pool_relocation || value_has_relocation {
        return;
    }

    output.instructions.swap(value_position, literal_position);
    for relocation in &mut output.relocations {
        if relocation.instruction_index == literal_position {
            relocation.instruction_index = value_position;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    #[test]
    fn pool_load_fills_a_dependent_pointer_load_gap() {
        let mut output = MachineFunction::default();
        output.instructions = vec![
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: 44,
            },
            Instruction::LoadFloatSingle {
                d: 1,
                a: 5,
                offset: 1568,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            },
        ];
        output.relocations.push(Relocation {
            instruction_index: 2,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::Constant(0),
        });

        schedule_tail(&mut output);

        assert!(matches!(
            output.instructions.as_slice(),
            [
                Instruction::LoadWord { d: 5, .. },
                Instruction::LoadFloatSingle { d: 0, a: 0, .. },
                Instruction::LoadFloatSingle { d: 1, a: 5, .. },
            ]
        ));
        assert_eq!(output.relocations[0].instruction_index, 1);
    }

    #[test]
    fn direct_value_load_keeps_legacy_value_first_order() {
        let mut output = MachineFunction::default();
        output.instructions = vec![
            Instruction::LoadFloatSingle {
                d: 1,
                a: 3,
                offset: 0,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            },
        ];
        output.relocations.push(Relocation {
            instruction_index: 1,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::Constant(0),
        });
        let original = output.instructions.clone();

        schedule_tail(&mut output);

        assert_eq!(output.instructions, original);
        assert_eq!(output.relocations[0].instruction_index, 1);
    }
}
