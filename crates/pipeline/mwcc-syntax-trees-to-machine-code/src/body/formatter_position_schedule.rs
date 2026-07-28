//! Late physical scheduling for adjacent float-to-integer formatter arguments.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    /// Overlap the destination, format, and shared member-base addresses, then
    /// pipeline three adjacent float-to-integer variadic arguments.
    pub(crate) fn schedule_position_formatter_arguments(&mut self) {
        schedule_position_formatter_arguments(&mut self.output);
    }
}

fn external_relocation(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    output.relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index
            || relocation.kind != kind
        {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(name) => Some(name.as_str()),
            _ => None,
        }
    })
}

fn schedule_position_formatter_arguments(
    output: &mut mwcc_machine_code::MachineFunction,
) -> bool {
    let Some(start) = output
        .instructions
        .windows(22)
        .position(is_unscheduled_position_formatter)
    else {
        return false;
    };
    let Some(global_high) =
        external_relocation(output, start, RelocationKind::Addr16Ha)
    else {
        return false;
    };
    let Some(global_low) =
        external_relocation(output, start + 2, RelocationKind::Addr16Lo)
    else {
        return false;
    };
    let Some(buffer_high) =
        external_relocation(output, start + 4, RelocationKind::Addr16Ha)
    else {
        return false;
    };
    let Some(string_high) =
        external_relocation(output, start + 5, RelocationKind::Addr16Ha)
    else {
        return false;
    };
    let Some(buffer_low) =
        external_relocation(output, start + 6, RelocationKind::Addr16Lo)
    else {
        return false;
    };
    let Some(string_low) =
        external_relocation(output, start + 7, RelocationKind::Addr16Lo)
    else {
        return false;
    };
    if global_high != global_low
        || buffer_high != buffer_low
        || string_high != string_low
        || global_high == buffer_high
        || global_high == string_high
        || buffer_high == string_high
        || output
            .relocations
            .iter()
            .filter(|relocation| {
                (start..start + 20).contains(&relocation.instruction_index)
            })
            .count()
            != 6
        || output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if (start + 1..start + 20).contains(target)
            )
        })
    {
        return false;
    }

    let mut global_address_high = output.instructions[start].clone();
    let link_store = output.instructions[start + 1].clone();
    let mut global_address_low = output.instructions[start + 2].clone();
    let mut member_load = output.instructions[start + 3].clone();
    let buffer_address_high = output.instructions[start + 4].clone();
    let string_address_high = output.instructions[start + 5].clone();
    let buffer_address_low = output.instructions[start + 6].clone();
    let string_address_low = output.instructions[start + 7].clone();
    let mut float_loads = [
        output.instructions[start + 8].clone(),
        output.instructions[start + 12].clone(),
        output.instructions[start + 16].clone(),
    ];
    let mut conversions = [
        output.instructions[start + 9].clone(),
        output.instructions[start + 13].clone(),
        output.instructions[start + 17].clone(),
    ];
    let mut float_stores = [
        output.instructions[start + 10].clone(),
        output.instructions[start + 14].clone(),
        output.instructions[start + 18].clone(),
    ];
    let word_loads = [
        output.instructions[start + 11].clone(),
        output.instructions[start + 15].clone(),
        output.instructions[start + 19].clone(),
    ];

    let Instruction::AddImmediateShifted { d, .. } =
        &mut global_address_high
    else {
        unreachable!()
    };
    *d = 4;
    let Instruction::AddImmediate { d, a, .. } = &mut global_address_low else {
        unreachable!()
    };
    *d = 5;
    *a = 4;
    let Instruction::LoadWord { d, a, .. } = &mut member_load else {
        unreachable!()
    };
    *d = 5;
    *a = 5;
    for (register, instruction) in [2, 1, 0].into_iter().zip(&mut float_loads) {
        let Instruction::LoadFloatSingle { d, a, .. } = instruction else {
            unreachable!()
        };
        *d = register;
        *a = 5;
    }
    for (register, instruction) in
        [2, 1, 0].into_iter().zip(&mut conversions)
    {
        let Instruction::ConvertToIntegerWordZero { d, b } = instruction else {
            unreachable!()
        };
        *d = register;
        *b = register;
    }
    for (register, instruction) in [2, 1, 0].into_iter().zip(&mut float_stores) {
        let Instruction::StoreFloatDouble { s, .. } = instruction else {
            unreachable!()
        };
        *s = register;
    }

    output.instructions[start..start + 20].clone_from_slice(&[
        global_address_high,
        buffer_address_high,
        link_store,
        global_address_low,
        string_address_high,
        buffer_address_low,
        member_load,
        string_address_low,
        float_loads[0].clone(),
        float_loads[1].clone(),
        float_loads[2].clone(),
        conversions[0].clone(),
        conversions[1].clone(),
        conversions[2].clone(),
        float_stores[0].clone(),
        float_stores[1].clone(),
        word_loads[0].clone(),
        float_stores[2].clone(),
        word_loads[1].clone(),
        word_loads[2].clone(),
    ]);
    for relocation in &mut output.relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == start + 2 => start + 3,
            index if index == start + 4 => start + 1,
            index if index == start + 5 => start + 4,
            index if index == start + 6 => start + 5,
            index => index,
        };
    }
    true
}

fn is_unscheduled_position_formatter(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::AddImmediateShifted { d: 7, a: 0, .. },
            Instruction::StoreWord { s: 0, a: 1, .. },
            Instruction::AddImmediate { d: 7, a: 7, .. },
            Instruction::LoadWord { d: 7, a: 7, .. },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 4, .. },
            Instruction::LoadFloatSingle { d: 0, a: 7, offset: first_offset },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: first_slot },
            Instruction::LoadWord { d: 5, a: 1, offset: first_word },
            Instruction::LoadFloatSingle { d: 0, a: 7, offset: second_offset },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: second_slot },
            Instruction::LoadWord { d: 6, a: 1, offset: second_word },
            Instruction::LoadFloatSingle { d: 0, a: 7, offset: third_offset },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: third_slot },
            Instruction::LoadWord { d: 7, a: 1, offset: third_word },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] if *second_offset == first_offset.saturating_add(4)
            && *third_offset == first_offset.saturating_add(8)
            && *second_slot == first_slot.saturating_add(8)
            && *third_slot == first_slot.saturating_add(16)
            && *first_word == first_slot.saturating_add(4)
            && *second_word == second_slot.saturating_add(4)
            && *third_word == third_slot.saturating_add(4)
            && target == "sprintf"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation, RelocationTarget};

    #[test]
    fn pipelines_three_adjacent_position_arguments() {
        let mut output = MachineFunction::new("probe");
        output.instructions = vec![
            Instruction::AddImmediateShifted { d: 7, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 36 },
            Instruction::AddImmediate { d: 7, a: 7, immediate: 0 },
            Instruction::LoadWord { d: 7, a: 7, offset: 1832 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 7, offset: 48 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 8 },
            Instruction::LoadWord { d: 5, a: 1, offset: 12 },
            Instruction::LoadFloatSingle { d: 0, a: 7, offset: 52 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 16 },
            Instruction::LoadWord { d: 6, a: 1, offset: 20 },
            Instruction::LoadFloatSingle { d: 0, a: 7, offset: 56 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 },
            Instruction::LoadWord { d: 7, a: 1, offset: 28 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "sprintf".into() },
        ];
        for (instruction_index, kind, target) in [
            (0, RelocationKind::Addr16Ha, "global"),
            (2, RelocationKind::Addr16Lo, "global"),
            (4, RelocationKind::Addr16Ha, "buffer"),
            (5, RelocationKind::Addr16Ha, "format"),
            (6, RelocationKind::Addr16Lo, "buffer"),
            (7, RelocationKind::Addr16Lo, "format"),
        ] {
            output.relocations.push(Relocation {
                instruction_index,
                kind,
                target: RelocationTarget::External(target.into()),
            });
        }

        assert!(schedule_position_formatter_arguments(&mut output));
        assert!(matches!(
            output.instructions[8..14],
            [
                Instruction::LoadFloatSingle { d: 2, a: 5, .. },
                Instruction::LoadFloatSingle { d: 1, a: 5, .. },
                Instruction::LoadFloatSingle { d: 0, a: 5, .. },
                Instruction::ConvertToIntegerWordZero { d: 2, b: 2 },
                Instruction::ConvertToIntegerWordZero { d: 1, b: 1 },
                Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            ]
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [0, 3, 1, 4, 5, 7]
        );
    }
}
