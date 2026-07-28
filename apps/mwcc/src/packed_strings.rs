//! Translation-unit packed string storage.
//!
//! `-str pool` gives all literals one `@stringBaseN` object. This module owns
//! byte interning and offsets; the driver remains responsible for source-order
//! scheduling and relocation rewriting.

use mwcc_machine_code::{
    Instruction, MachineFunction, RelocationKind, RelocationTarget,
};

#[derive(Default)]
pub(crate) struct PackedStrings {
    offsets: std::collections::HashMap<Vec<u8>, u32>,
    bytes: Vec<u8>,
}

impl PackedStrings {
    pub(crate) fn intern(&mut self, literal: &[u8]) -> u32 {
        if let Some(offset) = self.offsets.get(literal) {
            return *offset;
        }
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(literal);
        self.bytes.push(0);
        self.offsets.insert(literal.to_vec(), offset);
        offset
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

/// Mainline MWCC materializes an interior packed-string address as
/// `lis/addi @stringBase0`, followed by a separate `addi` for the byte offset.
/// Turn the temporary relocation addend into that explicit instruction after
/// the translation-unit interner has assigned final offsets.
pub(crate) fn materialize_function_offsets(function: &mut MachineFunction, base: &str) {
    let mut pairs = Vec::new();
    for (low_relocation_index, low) in function.relocations.iter().enumerate() {
        let RelocationTarget::ExternalWithAddend(target, addend) = &low.target else {
            continue;
        };
        if target != base || *addend == 0 || low.kind != RelocationKind::Addr16Lo {
            continue;
        }
        let Ok(immediate) = i16::try_from(*addend) else {
            continue;
        };
        let Some(Instruction::AddImmediate { d, .. }) =
            function.instructions.get(low.instruction_index)
        else {
            continue;
        };
        let Some(high_relocation_index) =
            function.relocations.iter().enumerate().rev().find_map(|(index, high)| {
                (high.instruction_index < low.instruction_index
                    && high.kind == RelocationKind::Addr16Ha
                    && matches!(
                        &high.target,
                        RelocationTarget::ExternalWithAddend(high_target, high_addend)
                            if high_target == target && high_addend == addend
                    ))
                .then_some(index)
            })
        else {
            continue;
        };
        pairs.push((
            low_relocation_index,
            high_relocation_index,
            low.instruction_index + 1,
            *d,
            immediate,
        ));
    }

    for (low, high, _, _, _) in &pairs {
        function.relocations[*low].target = RelocationTarget::External(base.to_owned());
        function.relocations[*high].target = RelocationTarget::External(base.to_owned());
    }
    pairs.sort_unstable_by_key(|(_, _, position, _, _)| std::cmp::Reverse(*position));
    pairs.dedup_by_key(|(_, _, position, _, _)| *position);
    for (_, _, position, register, immediate) in pairs {
        insert_instruction(
            function,
            position,
            Instruction::AddImmediate {
                d: register,
                a: register,
                immediate,
            },
        );
    }
}

fn insert_instruction(function: &mut MachineFunction, position: usize, instruction: Instruction) {
    function.instructions.insert(position, instruction);
    for relocation in &mut function.relocations {
        if relocation.instruction_index >= position {
            relocation.instruction_index += 1;
        }
    }
    for displacement in &mut function.data_section_displacements {
        if displacement.instruction_index >= position {
            displacement.instruction_index += 1;
        }
    }
    for (_, entry) in &mut function.entry_points {
        if *entry >= position {
            *entry += 1;
        }
    }
    let byte_position = position as u32 * 4;
    for table in &mut function.jump_tables {
        for entry in &mut table.entries {
            if *entry >= byte_position {
                *entry += 4;
            }
        }
    }
    for instruction in &mut function.instructions {
        match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target >= position =>
            {
                *target += 1;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{materialize_function_offsets, PackedStrings};
    use mwcc_machine_code::{
        Instruction, JumpTable, MachineFunction, Relocation, RelocationKind,
        RelocationTarget,
    };

    #[test]
    fn interns_literals_without_per_literal_padding() {
        let mut pool = PackedStrings::default();
        assert_eq!(pool.intern(b"%d"), 0);
        assert_eq!(pool.intern(b"%c"), 3);
        assert_eq!(pool.intern(b"%d"), 0);
        assert_eq!(pool.into_bytes(), b"%d\0%c\0");
    }

    #[test]
    fn materializes_an_interior_address_and_shifts_code_metadata() {
        let mut function = MachineFunction::new("probe");
        function.instructions = vec![
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
            Instruction::Branch { target: 3 },
            Instruction::BranchToLinkRegister,
        ];
        function.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::ExternalWithAddend(
                    "@stringBase0".to_owned(),
                    37,
                ),
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::ExternalWithAddend(
                    "@stringBase0".to_owned(),
                    37,
                ),
            },
        ];
        function.entry_points.push(("tail".to_owned(), 3));
        function.jump_tables.push(JumpTable {
            entries: vec![12],
            anonymous_offset: 0,
        });

        materialize_function_offsets(&mut function, "@stringBase0");

        assert_eq!(
            function.instructions[2],
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 37,
            }
        );
        assert!(matches!(
            function.instructions[3],
            Instruction::Branch { target: 4 }
        ));
        assert!(function.relocations.iter().all(|relocation| matches!(
            &relocation.target,
            RelocationTarget::External(target) if target == "@stringBase0"
        )));
        assert_eq!(function.entry_points[0].1, 4);
        assert_eq!(function.jump_tables[0].entries, vec![16]);
    }
}
