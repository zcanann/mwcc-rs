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
    materialize_retained_base_offsets(function, base);

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
    super::schedule::schedule_materialized_offsets(function);
}

/// A loop can retain the zero-offset packed-string base in a saved register and
/// apply one literal's final TU-wide offset only when marshaling the call
/// argument. Keep the HA/LO pair on the shared base and patch that dependent
/// `addi` instead of inserting an offset immediately after the low half.
fn materialize_retained_base_offsets(function: &mut MachineFunction, base: &str) {
    let candidates = function
        .relocations
        .iter()
        .enumerate()
        .filter_map(|(low_relocation_index, low)| {
            let RelocationTarget::ExternalWithAddend(target, addend) = &low.target else {
                return None;
            };
            if target != base || *addend == 0 || low.kind != RelocationKind::Addr16Lo {
                return None;
            }
            let Instruction::AddImmediate { d: retained, .. } =
                function.instructions.get(low.instruction_index)?
            else {
                return None;
            };
            if *retained < 14 {
                return None;
            }
            let high_relocation_index =
                function.relocations.iter().enumerate().rev().find_map(|(index, high)| {
                    (high.instruction_index < low.instruction_index
                        && high.kind == RelocationKind::Addr16Ha
                        && matches!(
                            &high.target,
                            RelocationTarget::ExternalWithAddend(high_target, high_addend)
                                if high_target == target && high_addend == addend
                        ))
                    .then_some(index)
                })?;
            let use_position = function
                .instructions
                .iter()
                .enumerate()
                .skip(low.instruction_index + 1)
                .find_map(|(position, instruction)| {
                    matches!(
                        instruction,
                        Instruction::AddImmediate {
                            d,
                            a,
                            immediate: 0,
                        } if a == retained && d != retained
                    )
                    .then_some(position)
                })?;
            Some((
                low_relocation_index,
                high_relocation_index,
                use_position,
                *addend,
            ))
        })
        .collect::<Vec<_>>();

    for (low, high, use_position, addend) in candidates {
        let Ok(immediate) = i16::try_from(addend) else {
            continue;
        };
        let Instruction::AddImmediate {
            immediate: use_immediate,
            ..
        } = &mut function.instructions[use_position]
        else {
            continue;
        };
        *use_immediate = immediate;
        function.relocations[low].target = RelocationTarget::External(base.to_owned());
        function.relocations[high].target = RelocationTarget::External(base.to_owned());
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
        if is_reloadable_nested_tail_packet(function, position, packed_base) {
            continue;
        }
        while position + 1 < function.instructions.len()
            && ready_zero_offset_argument_setup(&function.instructions[position + 1], packed_base)
            && !is_control_entry(function, position)
            && !is_control_entry(function, position + 1)
        {
            swap_adjacent_instructions(function, position);
            position += 1;
        }
    }
}

fn is_reloadable_nested_tail_packet(
    function: &MachineFunction,
    low: usize,
    packed_base: u8,
) -> bool {
    let Some(start) = low.checked_sub(2) else {
        return false;
    };
    matches!(
        function.instructions.get(start..low + 3),
        Some([
            Instruction::AddImmediateShifted { d: string, a: 0, .. },
            Instruction::AddImmediateShifted { d: array, a: 0, .. },
            Instruction::AddImmediate { d: string_low, a: string_base, .. },
            Instruction::Or { a: result, s: 3, b: 3 },
            Instruction::AddImmediate { d: 3, a: array_base, .. },
        ]) if *string == packed_base
            && *string_low == packed_base
            && *string_base == packed_base
            && *array >= 6
            && *array_base == *array
            && *result == packed_base + 1
    )
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

fn ready_zero_offset_argument_setup(instruction: &Instruction, packed_base: u8) -> bool {
    match instruction {
        Instruction::AddImmediate { d, a, .. } => {
            *d != packed_base
                && *a != packed_base
                && (*a == 0 || *a >= 6)
                && !(*d == packed_base + 1 && *a >= 6)
        }
        other => ready_integer_argument_setup(other, packed_base),
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
    use crate::packed_strings::schedule::schedule_materialized_offsets;
    use mwcc_machine_code::{
        Instruction, JumpTable, MachineFunction, Relocation, RelocationKind,
        RelocationTarget,
    };

    #[test]
    fn schedules_a_materialized_offset_inside_the_table_format_packet() {
        let mut function = MachineFunction::new("probe");
        function.instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::load_immediate(5, 47),
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::Add { d: 3, a: 0, b: 14 },
            Instruction::StoreByte {
                s: 5,
                a: 1,
                offset: 74,
            },
            Instruction::LoadByteZero {
                d: 6,
                a: 3,
                offset: 72,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 19,
            },
            Instruction::LoadByteZero {
                d: 7,
                a: 3,
                offset: 73,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 104,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 1,
                immediate: 72,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "sprintf".to_owned(),
            },
        ];
        function.relocations.push(Relocation {
            instruction_index: 7,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External("@stringBase0".to_owned()),
        });

        schedule_materialized_offsets(&mut function);

        assert!(matches!(
            function.instructions[6..11],
            [
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0,
                },
                Instruction::LoadByteZero { d: 6, .. },
                Instruction::LoadByteZero { d: 7, .. },
                Instruction::AddImmediate { d: 3, a: 1, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 19,
                },
            ]
        ));
        assert_eq!(function.relocations[0].instruction_index, 6);
    }

    #[test]
    fn delays_a_materialized_offset_past_the_terminated_table_loads() {
        let mut function = MachineFunction::new("probe");
        function.instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::load_immediate(5, 0),
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::Add { d: 3, a: 0, b: 14 },
            Instruction::StoreByte {
                s: 5,
                a: 1,
                offset: 103,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 27,
            },
            Instruction::LoadByteZero {
                d: 5,
                a: 3,
                offset: 75,
            },
            Instruction::LoadByteZero {
                d: 6,
                a: 3,
                offset: 76,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 40,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "sprintf".to_owned(),
            },
        ];
        function.relocations.push(Relocation {
            instruction_index: 7,
            kind: RelocationKind::Addr16Lo,
            target: RelocationTarget::External("@offset".to_owned()),
        });

        schedule_materialized_offsets(&mut function);

        assert!(matches!(
            function.instructions[7..11],
            [
                Instruction::LoadByteZero { d: 5, .. },
                Instruction::LoadByteZero { d: 6, .. },
                Instruction::AddImmediate { d: 3, a: 1, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 27,
                },
            ]
        ));
        assert_eq!(function.relocations[0].instruction_index, 10);
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

    #[test]
    fn applies_an_interior_offset_at_a_retained_base_use() {
        let mut function = MachineFunction::new("probe");
        function.instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::Branch { target: 3 },
            Instruction::AddImmediate {
                d: 4,
                a: 31,
                immediate: 0,
            },
        ];
        function.relocations = vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::ExternalWithAddend(
                    "@stringBase0".to_owned(),
                    14,
                ),
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::ExternalWithAddend(
                    "@stringBase0".to_owned(),
                    14,
                ),
            },
        ];

        materialize_function_offsets(&mut function, "@stringBase0");

        assert_eq!(function.instructions.len(), 4);
        assert!(matches!(
            function.instructions[3],
            Instruction::AddImmediate {
                d: 4,
                a: 31,
                immediate: 14,
            }
        ));
        assert!(function.relocations.iter().all(|relocation| matches!(
            &relocation.target,
            RelocationTarget::External(target) if target == "@stringBase0"
        )));
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

    #[test]
    fn keeps_a_zero_offset_base_low_before_argument_address_arithmetic() {
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
            Instruction::AddImmediate { d: 4, a: 4, .. }
        ));
        assert!(matches!(
            function.instructions[2],
            Instruction::AddImmediate { d: 3, a: 1, .. }
        ));
        assert_eq!(function.relocations[1].instruction_index, 1);
    }

    #[test]
    fn preserves_a_reloadable_nested_tail_argument_packet() {
        let mut function = MachineFunction::new("probe");
        function.instructions = vec![
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 6,
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
                instruction_index: 2,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("@stringBase0".to_owned()),
            },
        ];

        materialize_function_offsets(&mut function, "@stringBase0");

        assert!(matches!(
            function.instructions[2..5],
            [
                Instruction::AddImmediate { d: 4, a: 4, .. },
                Instruction::Or { a: 5, s: 3, b: 3 },
                Instruction::AddImmediate { d: 3, a: 6, .. },
            ]
        ));
        assert_eq!(function.relocations[1].instruction_index, 2);
    }

    #[test]
    fn keeps_a_zero_offset_base_low_before_a_dependent_result_offset() {
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
                a: 6,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 6,
                immediate: 1,
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
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 0
            }
        ));
        assert!(matches!(
            function.instructions[2],
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0
            }
        ));
        assert!(matches!(
            function.instructions[3],
            Instruction::AddImmediate {
                d: 5,
                a: 6,
                immediate: 1
            }
        ));
        assert_eq!(function.relocations[1].instruction_index, 2);
    }
}
