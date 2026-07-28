//! Late issue order for packets that include materialized packed-string offsets.

use mwcc_machine_code::{Instruction, MachineFunction};

pub(super) fn schedule_materialized_offsets(function: &mut MachineFunction) {
    schedule_date_separator_format_call(function);
    schedule_zero_terminated_format_call(function);
    schedule_final_time_format_call(function);
}

fn schedule_date_separator_format_call(function: &mut MachineFunction) {
    let Some(start) = function.instructions.windows(14).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 47,
                },
                Instruction::AddImmediate { d: 0, a: 3, .. },
                Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                Instruction::Add {
                    d: 3,
                    a: 0,
                    b: 14,
                },
                Instruction::StoreByte { s: 5, a: 1, .. },
                Instruction::LoadByteZero {
                    d: 6,
                    a: 3,
                    offset: first_load_offset,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: string_offset,
                },
                Instruction::LoadByteZero {
                    d: 7,
                    a: 3,
                    offset: second_load_offset,
                },
                Instruction::AddImmediate { d: 3, a: 1, .. },
                Instruction::AddImmediate { d: 5, a: 1, .. },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { target },
            ] if is_sprintf(target)
                && *string_offset > 0
                && *second_load_offset == *first_load_offset + 1
        )
    }) else {
        return;
    };
    if (start + 6..=start + 10).any(|position| is_control_entry(function, position)) {
        return;
    }

    swap_adjacent(function, start + 6);
    swap_adjacent(function, start + 8);
    swap_adjacent(function, start + 9);
}

fn schedule_zero_terminated_format_call(function: &mut MachineFunction) {
    let Some(start) = function.instructions.windows(13).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediate {
                    d: 5,
                    a: 0,
                    immediate: 0,
                },
                Instruction::AddImmediate { d: 0, a: 3, .. },
                Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                Instruction::Add {
                    d: 3,
                    a: 0,
                    b: 14,
                },
                Instruction::StoreByte { s: 5, a: 1, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: string_offset,
                },
                Instruction::LoadByteZero {
                    d: 5,
                    a: 3,
                    offset: first_load_offset,
                },
                Instruction::LoadByteZero {
                    d: 6,
                    a: 3,
                    offset: second_load_offset,
                },
                Instruction::AddImmediate { d: 3, a: 1, .. },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { target },
            ] if is_sprintf(target)
                && *string_offset > 0
                && *second_load_offset == *first_load_offset + 1
        )
    }) else {
        return;
    };
    if (start + 7..=start + 10).any(|position| is_control_entry(function, position)) {
        return;
    }

    swap_adjacent(function, start + 7);
    swap_adjacent(function, start + 8);
    swap_adjacent(function, start + 9);
}

fn schedule_final_time_format_call(function: &mut MachineFunction) {
    let Some(start) = function.instructions.windows(13).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                Instruction::AddImmediateShifted { d: 7, a: 0, .. },
                Instruction::AddImmediate { d: 3, a: 1, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: 0,
                },
                Instruction::Or {
                    a: 5,
                    s: hour,
                    b: hour_copy,
                },
                Instruction::AddImmediate { d: 0, a: 7, .. },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: string_offset,
                },
                Instruction::Add {
                    d: 7,
                    a: 0,
                    b: 14,
                },
                Instruction::LoadByteZero {
                    d: 6,
                    a: 7,
                    offset: first_load_offset,
                },
                Instruction::LoadByteZero {
                    d: 7,
                    a: 7,
                    offset: second_load_offset,
                },
                Instruction::AddImmediate { d: 8, a: 1, .. },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { target },
            ] if is_sprintf(target)
                && hour == hour_copy
                && (14..=31).contains(hour)
                && *string_offset > 0
                && *second_load_offset == *first_load_offset + 1
        )
    }) else {
        return;
    };
    if (start + 1..=start + 11).any(|position| is_control_entry(function, position)) {
        return;
    }

    let old = function.instructions[start..start + 12].to_vec();
    let mut scheduled = vec![
        rewrite_lis(&old[1], 3),
        rewrite_lis(&old[0], 6),
        rewrite_addi(&old[5], 0, 3),
        old[4].clone(),
        rewrite_add(&old[7], 4, 0, 14),
        rewrite_addi(&old[3], 8, 6),
        rewrite_load_byte(&old[8], 6, 4),
        old[2].clone(),
        rewrite_load_byte(&old[9], 7, 4),
        rewrite_addi(&old[6], 4, 8),
        old[10].clone(),
        old[11].clone(),
    ];
    function.instructions[start..start + 12].swap_with_slice(&mut scheduled);

    let order = [1, 0, 5, 4, 7, 3, 8, 2, 9, 6, 10, 11];
    let mut permutation: Vec<usize> = (0..function.instructions.len()).collect();
    for (new_relative, old_relative) in order.into_iter().enumerate() {
        permutation[start + old_relative] = start + new_relative;
    }
    remap_instruction_owners(function, &permutation);
}

fn rewrite_lis(instruction: &Instruction, d: u8) -> Instruction {
    let Instruction::AddImmediateShifted { a, immediate, .. } = instruction else {
        unreachable!("the packed-format high half was matched above");
    };
    Instruction::AddImmediateShifted {
        d,
        a: *a,
        immediate: *immediate,
    }
}

fn rewrite_addi(instruction: &Instruction, d: u8, a: u8) -> Instruction {
    let Instruction::AddImmediate { immediate, .. } = instruction else {
        unreachable!("the packed-format address adjustment was matched above");
    };
    Instruction::AddImmediate {
        d,
        a,
        immediate: *immediate,
    }
}

fn rewrite_add(instruction: &Instruction, d: u8, a: u8, b: u8) -> Instruction {
    let Instruction::Add { .. } = instruction else {
        unreachable!("the packed-format indexed address was matched above");
    };
    Instruction::Add { d, a, b }
}

fn rewrite_load_byte(instruction: &Instruction, d: u8, a: u8) -> Instruction {
    let Instruction::LoadByteZero { offset, .. } = instruction else {
        unreachable!("the packed-format table load was matched above");
    };
    Instruction::LoadByteZero {
        d,
        a,
        offset: *offset,
    }
}

fn is_sprintf(target: &str) -> bool {
    target == "sprintf" || target.starts_with("sprintf__")
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

fn swap_adjacent(function: &mut MachineFunction, left: usize) {
    function.instructions.swap(left, left + 1);
    let mut permutation: Vec<usize> = (0..function.instructions.len()).collect();
    permutation.swap(left, left + 1);
    remap_instruction_owners(function, &permutation);
}

fn remap_instruction_owners(function: &mut MachineFunction, permutation: &[usize]) {
    for relocation in &mut function.relocations {
        relocation.instruction_index = permutation[relocation.instruction_index];
    }
    for displacement in &mut function.data_section_displacements {
        displacement.instruction_index = permutation[displacement.instruction_index];
    }
}

#[cfg(test)]
mod tests {
    use super::schedule_materialized_offsets;
    use mwcc_machine_code::{
        Instruction, MachineFunction, Relocation, RelocationKind,
        RelocationTarget,
    };

    #[test]
    fn schedules_and_colors_the_final_time_format_packet() {
        let mut function = MachineFunction::new("probe");
        function.instructions = vec![
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 7,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 40,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::move_register(5, 16),
            Instruction::AddImmediate {
                d: 0,
                a: 7,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 38,
            },
            Instruction::Add { d: 7, a: 0, b: 14 },
            Instruction::LoadByteZero {
                d: 6,
                a: 7,
                offset: 78,
            },
            Instruction::LoadByteZero {
                d: 7,
                a: 7,
                offset: 79,
            },
            Instruction::AddImmediate {
                d: 8,
                a: 1,
                immediate: 8,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "sprintf".to_owned(),
            },
        ];
        function.relocations = [0, 1, 3, 5]
            .into_iter()
            .map(|instruction_index| Relocation {
                instruction_index,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("target".to_owned()),
            })
            .collect();

        schedule_materialized_offsets(&mut function);

        assert!(matches!(
            function.instructions.as_slice(),
            [
                Instruction::AddImmediateShifted { d: 3, .. },
                Instruction::AddImmediateShifted { d: 6, .. },
                Instruction::AddImmediate { d: 0, a: 3, .. },
                Instruction::Or { a: 5, s: 16, b: 16 },
                Instruction::Add { d: 4, a: 0, b: 14 },
                Instruction::AddImmediate { d: 8, a: 6, .. },
                Instruction::LoadByteZero { d: 6, a: 4, .. },
                Instruction::AddImmediate { d: 3, a: 1, .. },
                Instruction::LoadByteZero { d: 7, a: 4, .. },
                Instruction::AddImmediate { d: 4, a: 8, .. },
                Instruction::AddImmediate { d: 8, a: 1, .. },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { .. },
            ]
        ));
        assert_eq!(
            function
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [1, 0, 5, 2]
        );
    }
}
