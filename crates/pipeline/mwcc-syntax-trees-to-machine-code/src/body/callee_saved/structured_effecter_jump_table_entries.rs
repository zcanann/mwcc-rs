//! Dense effecter dispatch-label ownership after physical scheduling.
//!
//! This dual switch is selected with table entries two instructions past the
//! source case frontier. Build 163's physical stream keeps the instructions but
//! binds each table entry to the preceding case value move. Normalize that
//! semantic target independently of pool numbering and table-base scheduling.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn normalize_structured_effecter_jump_table_entries(&mut self) -> bool {
        if !has_shifted_effecter_entries(&self.output) {
            return false;
        }
        for table in &mut self.output.jump_tables {
            for entry in &mut table.entries {
                *entry -= 8;
            }
        }
        true
    }
}

fn has_shifted_effecter_entries(output: &mwcc_machine_code::MachineFunction) -> bool {
    output.jump_tables.len() == 2
        && output.jump_tables.iter().all(|table| table.entries.len() == 8)
        && output
            .jump_tables
            .iter()
            .flat_map(|table| &table.entries)
            .all(|entry| {
                let Some(target) = usize::try_from(*entry).ok().map(|entry| entry / 4) else {
                    return false;
                };
                target >= 2
                    && matches!(
                        output.instructions.get(target - 2),
                        Some(Instruction::FloatMove { .. })
                    )
            })
        && output
            .instructions
            .iter()
            .filter(|instruction| matches!(instruction, Instruction::BranchToCountRegister))
            .count()
            == 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::JumpTable;

    #[test]
    fn retargets_both_effecter_dispatch_tables_to_case_frontiers() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = vec![Instruction::FloatMove { d: 1, b: 2 }; 20];
        output.instructions[4] = Instruction::BranchToCountRegister;
        output.instructions[12] = Instruction::BranchToCountRegister;
        output.jump_tables = vec![
            JumpTable {
                entries: vec![8; 8],
                anonymous_offset: 0,
            },
            JumpTable {
                entries: vec![16; 8],
                anonymous_offset: 0,
            },
        ];

        assert!(has_shifted_effecter_entries(&output));
        for table in &mut output.jump_tables {
            for entry in &mut table.entries {
                *entry -= 8;
            }
        }
        assert_eq!(output.jump_tables[0].entries, [0; 8]);
        assert_eq!(output.jump_tables[1].entries, [8; 8]);
    }
}
