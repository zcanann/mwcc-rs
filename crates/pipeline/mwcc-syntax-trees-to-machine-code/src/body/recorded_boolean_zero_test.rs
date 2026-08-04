//! Fold a value-producing narrow operation and its zero compare into record form.
//!
//! PowerPC shifts and narrow extensions can produce their value and set CR0 in
//! one instruction. MWCC selects that form across adjacent definition/use
//! boundaries; doing the fold after allocation preserves the chosen physical
//! register while removing the now-redundant compare.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn fold_recorded_boolean_zero_tests(&mut self) {
        if self.output.pre_scheduled {
            return;
        }

        let mut index = 0;
        while index + 1 < self.output.instructions.len() {
            let Some(record) = recorded_zero_test_at(&self.output, index) else {
                index += 1;
                continue;
            };

            // Keep the replacement at the compare's old position, then remove
            // the shift. An entry targeting the shift follows to the fused form;
            // entries targeting the compare were rejected by recognition.
            self.output.instructions[index + 1] = record;
            crate::remove_instruction_retargeting_to_next(self, index);
            index += 1;
        }
    }
}

fn recorded_zero_test_at(
    output: &mwcc_machine_code::MachineFunction,
    index: usize,
) -> Option<Instruction> {
    let [producer, compare] = output.instructions.get(index..index + 2)? else {
        return None;
    };
    if instruction_has_entry(output, index + 1) {
        return None;
    }
    match (producer, compare) {
        (
            Instruction::ShiftRightLogicalImmediate { a, s, shift: 31 },
            Instruction::CompareLogicalWordImmediate {
                a: compared,
                immediate: 0,
            },
        ) if a == compared => Some(Instruction::RotateAndMaskRecord {
            a: *a,
            s: *s,
            shift: 1,
            begin: 31,
            end: 31,
        }),
        (
            Instruction::ClearLeftImmediate { a, s, clear },
            Instruction::CompareLogicalWordImmediate {
                a: compared,
                immediate: 0,
            },
        ) if a == compared && *clear != 0 => Some(Instruction::ClearLeftImmediateRecord {
            a: *a,
            s: *s,
            clear: *clear,
        }),
        (
            Instruction::ExtendSignByte { a, s },
            Instruction::CompareWordImmediate {
                a: compared,
                immediate: 0,
            },
        ) if a == compared => Some(Instruction::ExtendSignByteRecord { a: *a, s: *s }),
        (
            Instruction::ExtendSignHalfword { a, s },
            Instruction::CompareWordImmediate {
                a: compared,
                immediate: 0,
            },
        ) if a == compared => Some(Instruction::ExtendSignHalfwordRecord { a: *a, s: *s }),
        _ => None,
    }
}

fn instruction_has_entry(
    output: &mwcc_machine_code::MachineFunction,
    index: usize,
) -> bool {
    output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                if *target == index
        )
    }) || output.jump_tables.iter().any(|table| {
        table
            .entries
            .iter()
            .any(|entry| *entry as usize == index.saturating_mul(4))
    }) || output
        .relocations
        .iter()
        .any(|relocation| relocation.instruction_index == index)
        || output
            .data_section_displacements
            .iter()
            .any(|displacement| displacement.instruction_index == index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::MachineFunction;

    fn candidate() -> MachineFunction {
        let mut output = MachineFunction::new("boolean");
        output.instructions = vec![
            Instruction::ShiftRightLogicalImmediate {
                a: 4,
                s: 0,
                shift: 31,
            },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 3,
            },
            Instruction::BranchToLinkRegister,
        ];
        output
    }

    #[test]
    fn recognizes_an_adjacent_normalized_boolean_zero_test() {
        assert_eq!(
            recorded_zero_test_at(&candidate(), 0),
            Some(Instruction::RotateAndMaskRecord {
                a: 4,
                s: 0,
                shift: 1,
                begin: 31,
                end: 31,
            })
        );
    }

    #[test]
    fn preserves_a_compare_that_is_a_control_flow_entry() {
        let mut output = candidate();
        output.instructions.push(Instruction::Branch { target: 1 });
        assert_eq!(recorded_zero_test_at(&output, 0), None);
    }

    #[test]
    fn does_not_record_a_wider_unsigned_shift() {
        let mut output = candidate();
        output.instructions[0] = Instruction::ShiftRightLogicalImmediate {
            a: 4,
            s: 0,
            shift: 1,
        };
        assert_eq!(recorded_zero_test_at(&output, 0), None);
    }

    #[test]
    fn records_an_unsigned_narrow_cast_before_its_zero_compare() {
        let mut output = candidate();
        output.instructions[0] = Instruction::ClearLeftImmediate {
            a: 0,
            s: 4,
            clear: 24,
        };
        output.instructions[1] =
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 };
        assert_eq!(
            recorded_zero_test_at(&output, 0),
            Some(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 4,
                clear: 24,
            })
        );
    }

    #[test]
    fn records_a_signed_narrow_cast_before_its_zero_compare() {
        let mut output = candidate();
        output.instructions[0] = Instruction::ExtendSignHalfword { a: 0, s: 4 };
        output.instructions[1] = Instruction::CompareWordImmediate { a: 0, immediate: 0 };
        assert_eq!(
            recorded_zero_test_at(&output, 0),
            Some(Instruction::ExtendSignHalfwordRecord { a: 0, s: 4 })
        );
    }
}
