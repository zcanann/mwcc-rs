//! Fold a materialized one-bit boolean and its zero compare into `srwi.`.
//!
//! The PowerPC record form produces the same CR0 relation as `cmplwi value,0`
//! when a logical right shift by 31 has normalized the value to 0 or 1. MWCC
//! selects that form across an adjacent local definition/use boundary; doing the
//! fold after allocation preserves the local's chosen physical home.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn fold_recorded_boolean_zero_tests(&mut self) {
        if self.output.pre_scheduled {
            return;
        }

        let mut index = 0;
        while index + 1 < self.output.instructions.len() {
            let Some(record) = recorded_shift_at(&self.output, index) else {
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

fn recorded_shift_at(
    output: &mwcc_machine_code::MachineFunction,
    index: usize,
) -> Option<Instruction> {
    let [
        Instruction::ShiftRightLogicalImmediate { a, s, shift: 31 },
        Instruction::CompareLogicalWordImmediate {
            a: compared,
            immediate: 0,
        },
    ] = output.instructions.get(index..index + 2)?
    else {
        return None;
    };
    if a != compared || instruction_has_entry(output, index + 1) {
        return None;
    }
    Some(Instruction::RotateAndMaskRecord {
        a: *a,
        s: *s,
        shift: 1,
        begin: 31,
        end: 31,
    })
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
            recorded_shift_at(&candidate(), 0),
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
        assert_eq!(recorded_shift_at(&output, 0), None);
    }

    #[test]
    fn does_not_record_a_wider_unsigned_shift() {
        let mut output = candidate();
        output.instructions[0] = Instruction::ShiftRightLogicalImmediate {
            a: 4,
            s: 0,
            shift: 1,
        };
        assert_eq!(recorded_shift_at(&output, 0), None);
    }
}
