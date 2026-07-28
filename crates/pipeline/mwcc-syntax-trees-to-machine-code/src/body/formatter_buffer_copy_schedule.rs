//! Scheduling for formatting through a cleared temporary buffer.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Preserve MWCC's saved-result latency slot and the two address schedules
    /// used by `sprintf(tmp, integer_format, saved)` followed by
    /// `sprintf(static_buffer, string_format, tmp)`.
    pub(crate) fn schedule_temporary_buffer_format_copy(&mut self) {
        if !recognize_complete_format_copy(&self.output) {
            return;
        }
        stage_saved_result(self);
        schedule_temporary_format(&mut self.output);
        schedule_buffer_copy_format(&mut self.output);
    }
}

fn recognize_complete_format_copy(
    output: &mwcc_machine_code::MachineFunction,
) -> bool {
    let Some(handoff) = output
        .instructions
        .windows(6)
        .position(is_unscheduled_saved_result)
    else {
        return false;
    };
    let Some(first) = output.instructions[handoff + 6..]
        .windows(6)
        .position(is_unscheduled_temporary_format)
        .map(|offset| handoff + 6 + offset)
    else {
        return false;
    };
    let Some(second) = output.instructions[first + 6..]
        .windows(7)
        .position(is_unscheduled_buffer_copy_format)
        .map(|offset| first + 6 + offset)
    else {
        return false;
    };
    let temporary_offset = match output.instructions[handoff + 2] {
        Instruction::AddImmediate {
            a: 1,
            immediate,
            ..
        } => immediate,
        _ => unreachable!(),
    };
    matches!(
        output.instructions[first + 1],
        Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate,
        } if immediate == temporary_offset
    ) && matches!(
        output.instructions[second + 4],
        Instruction::AddImmediate {
            d: 5,
            a: 1,
            immediate,
        } if immediate == temporary_offset
    ) && output.instructions[handoff + 6..first]
        .iter()
        .any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchAndLink { target } if target == "memset"
            )
        })
        && paired_external_address(output, first, first + 2)
        && paired_external_address(output, second, second + 2)
        && paired_external_address(output, second + 1, second + 3)
        && no_branch_entry(output, handoff + 1, second + 5)
}

fn paired_external_address(
    output: &mwcc_machine_code::MachineFunction,
    high: usize,
    low: usize,
) -> bool {
    output.relocations.iter().any(|relocation| {
        relocation.instruction_index == high
            && relocation.kind == RelocationKind::Addr16Ha
    }) && output.relocations.iter().any(|relocation| {
        relocation.instruction_index == low
            && relocation.kind == RelocationKind::Addr16Lo
    }) && schedule_relocations::same_target_value(
        &output.relocations,
        &output.constants,
        high,
        low,
    )
}

fn no_branch_entry(
    output: &mwcc_machine_code::MachineFunction,
    first: usize,
    last: usize,
) -> bool {
    !output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                if (first..=last).contains(target)
        )
    })
}

fn is_unscheduled_saved_result(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::BranchAndLink { .. },
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::AddImmediate { d: 3, a: 1, .. },
            Instruction::AddImmediate {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::BranchAndLink { target },
        ] if target == "memset"
    )
}

fn is_unscheduled_temporary_format(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 1, .. },
            Instruction::AddImmediate { d: 4, a: 4, .. },
            Instruction::Or { a: 5, s: 31, b: 31 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] if target == "sprintf"
    )
}

fn is_unscheduled_buffer_copy_format(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 4, .. },
            Instruction::AddImmediate { d: 5, a: 1, .. },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] if target == "sprintf"
    )
}

fn stage_saved_result(generator: &mut Generator) {
    let start = generator
        .output
        .instructions
        .windows(6)
        .position(is_unscheduled_saved_result)
        .expect("complete format-copy shape retained its handoff");
    let insertion = stage_saved_result_output(&mut generator.output, start);
    generator.labels.inserted(insertion, 1);
}

fn stage_saved_result_output(
    output: &mut mwcc_machine_code::MachineFunction,
    start: usize,
) -> usize {
    output.instructions[start + 1] = Instruction::move_register(0, 3);
    let insertion = start + 3;
    output
        .instructions
        .insert(insertion, Instruction::move_register(31, 0));
    for relocation in &mut output.relocations {
        if relocation.instruction_index >= insertion {
            relocation.instruction_index += 1;
        }
    }
    for displacement in &mut output.data_section_displacements {
        if displacement.instruction_index >= insertion {
            displacement.instruction_index += 1;
        }
    }
    for instruction in &mut output.instructions {
        match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target >= insertion =>
            {
                *target += 1;
            }
            _ => {}
        }
    }
    insertion
}

fn schedule_temporary_format(
    output: &mut mwcc_machine_code::MachineFunction,
) {
    let start = output
        .instructions
        .windows(6)
        .position(is_unscheduled_temporary_format)
        .expect("complete format-copy shape retained its first formatter");
    let mut format_high = output.instructions[start].clone();
    let temporary = output.instructions[start + 1].clone();
    let mut format_low = output.instructions[start + 2].clone();
    let saved = output.instructions[start + 3].clone();
    let Instruction::AddImmediateShifted { d, .. } = &mut format_high else {
        unreachable!()
    };
    *d = 3;
    let Instruction::AddImmediate { d, a, .. } = &mut format_low else {
        unreachable!()
    };
    *d = 4;
    *a = 3;
    output.instructions[start..start + 4].clone_from_slice(&[
        format_high,
        saved,
        format_low,
        temporary,
    ]);
}

fn schedule_buffer_copy_format(
    output: &mut mwcc_machine_code::MachineFunction,
) {
    let start = output
        .instructions
        .windows(7)
        .position(is_unscheduled_buffer_copy_format)
        .expect("complete format-copy shape retained its second formatter");
    let buffer_high = output.instructions[start].clone();
    let format_high = output.instructions[start + 1].clone();
    let buffer_low = output.instructions[start + 2].clone();
    let format_low = output.instructions[start + 3].clone();
    let temporary = output.instructions[start + 4].clone();
    output.instructions[start..start + 5].clone_from_slice(&[
        format_high,
        buffer_high,
        format_low,
        temporary,
        buffer_low,
    ]);
    remap_relocations(
        output,
        &[
            (start, start + 1),
            (start + 1, start),
            (start + 2, start + 4),
            (start + 3, start + 2),
        ],
    );
}

fn remap_relocations(
    output: &mut mwcc_machine_code::MachineFunction,
    remaps: &[(usize, usize)],
) {
    for relocation in &mut output.relocations {
        if let Some((_, destination)) = remaps
            .iter()
            .find(|(source, _)| relocation.instruction_index == *source)
        {
            relocation.instruction_index = *destination;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{
        MachineFunction, Relocation, RelocationKind, RelocationTarget,
    };

    fn relocation(index: usize, kind: RelocationKind, name: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind,
            target: RelocationTarget::External(name.into()),
        }
    }

    #[test]
    fn schedules_saved_temporary_format_and_copy_as_one_shape() {
        let mut output = MachineFunction::new("probe");
        output.instructions = vec![
            Instruction::BranchAndLink { target: "available".into() },
            Instruction::move_register(31, 3),
            Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
            Instruction::load_immediate(4, 0),
            Instruction::load_immediate(5, 32),
            Instruction::BranchAndLink { target: "memset".into() },
            Instruction::BranchAndLink { target: "memset".into() },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::move_register(5, 31),
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "sprintf".into() },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 1, immediate: 8 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "sprintf".into() },
        ];
        output.relocations = vec![
            relocation(7, RelocationKind::Addr16Ha, "format"),
            relocation(9, RelocationKind::Addr16Lo, "format"),
            relocation(13, RelocationKind::Addr16Ha, "buffer"),
            relocation(14, RelocationKind::Addr16Ha, "string_format"),
            relocation(15, RelocationKind::Addr16Lo, "buffer"),
            relocation(16, RelocationKind::Addr16Lo, "string_format"),
        ];

        assert!(recognize_complete_format_copy(&output));
        let start = output
            .instructions
            .windows(6)
            .position(is_unscheduled_saved_result)
            .unwrap();
        stage_saved_result_output(&mut output, start);
        schedule_temporary_format(&mut output);
        schedule_buffer_copy_format(&mut output);

        assert!(matches!(
            output.instructions[1..4],
            [
                Instruction::Or { a: 0, s: 3, b: 3 },
                Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
                Instruction::Or { a: 31, s: 0, b: 0 },
            ]
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [8, 10, 15, 14, 18, 16]
        );
    }
}
