//! Late packed-string address materialization and scheduling.

use mwcc_machine_code::{
    Instruction, MachineFunction, RelocationKind, RelocationTarget,
};

/// Mainline MWCC materializes an interior packed-string address as
/// `lis/addi @stringBase0`, followed by a separate `addi` for the byte offset.
/// Turn the temporary relocation addend into that explicit instruction after
/// the translation-unit interner has assigned final offsets.
pub(crate) fn materialize_function_offsets(function: &mut MachineFunction, base: &str) {
    schedule_zero_offset_base_lows(function, base);

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
        let mut insertion_position = low.instruction_index + 1;
        while function
            .instructions
            .get(insertion_position)
            .is_some_and(|instruction| ready_integer_argument_setup(instruction, *d))
        {
            insertion_position += 1;
        }
        pairs.push((
            low_relocation_index,
            high_relocation_index,
            insertion_position,
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

fn schedule_zero_offset_base_lows(function: &mut MachineFunction, base: &str) {
    let mut positions = function
        .relocations
        .iter()
        .filter_map(|relocation| {
            (relocation.kind == RelocationKind::Addr16Lo
                && matches!(
                    &relocation.target,
                    RelocationTarget::External(target) if target == base
                )
                && matches!(
                    function.instructions.get(relocation.instruction_index),
                    Some(Instruction::AddImmediate { d, a, .. }) if d == a
                ))
            .then_some(relocation.instruction_index)
        })
        .collect::<Vec<_>>();
    positions.sort_unstable_by(|left, right| right.cmp(left));
    positions.dedup();

    for mut position in positions {
        let packed_base = match function.instructions[position] {
            Instruction::AddImmediate { d, .. } => d,
            _ => continue,
        };
        while position + 1 < function.instructions.len()
            && ready_integer_argument_setup(&function.instructions[position + 1], packed_base)
            && !is_control_entry(function, position)
            && !is_control_entry(function, position + 1)
        {
            swap_adjacent_instructions(function, position);
            position += 1;
        }
    }
}

fn is_control_entry(function: &MachineFunction, position: usize) -> bool {
    function.entry_points.iter().any(|(_, entry)| *entry == position)
        || function.jump_tables.iter().any(|table| {
            table
                .entries
                .iter()
                .any(|entry| *entry == position as u32 * 4)
        })
        || function.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if *target == position
            )
        })
}

fn swap_adjacent_instructions(function: &mut MachineFunction, left: usize) {
    function.instructions.swap(left, left + 1);
    let swap_index = |index: &mut usize| {
        if *index == left {
            *index += 1;
        } else if *index == left + 1 {
            *index -= 1;
        }
    };
    for relocation in &mut function.relocations {
        swap_index(&mut relocation.instruction_index);
    }
    for displacement in &mut function.data_section_displacements {
        swap_index(&mut displacement.instruction_index);
    }
}

fn ready_integer_argument_setup(instruction: &Instruction, packed_base: u8) -> bool {
    match instruction {
        Instruction::AddImmediate { d, a, .. } => *d != packed_base && *a != packed_base,
        Instruction::Or { a, s, b } if s == b => *a != packed_base && *s != packed_base,
        _ => false,
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
    use super::materialize_function_offsets;
    use mwcc_machine_code::{
        Instruction, JumpTable, MachineFunction, Relocation, RelocationKind,
        RelocationTarget,
    };

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

    #[test]
    fn preserves_an_independent_address_in_the_packed_base_slot() {
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
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            Instruction::move_register(5, 3),
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 0,
            },
            Instruction::BranchToLinkRegister,
        ];
        function.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::ExternalWithAddend(
                    "@stringBase0".to_owned(),
                    16,
                ),
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::ExternalWithAddend(
                    "@stringBase0".to_owned(),
                    16,
                ),
            },
        ];

        materialize_function_offsets(&mut function, "@stringBase0");

        assert!(matches!(
            function.instructions[2],
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8
            }
        ));
        assert!(matches!(
            function.instructions[3],
            Instruction::Or { a: 5, s: 3, b: 3 }
        ));
        assert!(matches!(
            function.instructions[4],
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 0
            }
        ));
        assert!(matches!(
            function.instructions[5],
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 16
            }
        ));
    }

    #[test]
    fn schedules_a_zero_offset_base_low_after_ready_arguments() {
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
            Instruction::move_register(5, 3),
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 0,
            },
            Instruction::BranchToLinkRegister,
        ];
        function.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("@stringBase0".to_owned()),
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("@stringBase0".to_owned()),
            },
        ];

        materialize_function_offsets(&mut function, "@stringBase0");

        assert!(matches!(
            function.instructions[1],
            Instruction::Or { a: 5, s: 3, b: 3 }
        ));
        assert!(matches!(
            function.instructions[2],
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 0
            }
        ));
        assert!(matches!(
            function.instructions[3],
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0
            }
        ));
        assert_eq!(function.relocations[1].instruction_index, 3);
    }
}
