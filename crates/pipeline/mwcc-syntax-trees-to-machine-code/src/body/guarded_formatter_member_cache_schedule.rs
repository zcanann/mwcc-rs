//! Retain one global member across a guarded formatter choice.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Cache the member tested by `if (value == A) ... else if (value == B) ...`
    /// and reuse it as the final formatter argument. The complete two-branch
    /// chain and three matching global relocations prove the value is stable.
    pub(crate) fn schedule_guarded_formatter_member_cache(&mut self) {
        let Some(shape) = recognize(&self.output) else {
            return;
        };
        cache_first_load(&mut self.output.instructions, shape.first);
        remove_second_load(self, shape.second);
        let default = find_default_formatter(&self.output, shape.member_offset)
            .expect("guarded formatter cache retained its default arm");
        collapse_default_formatter(self, default);
    }
}

#[derive(Clone, Copy)]
struct Shape {
    first: usize,
    second: usize,
    member_offset: i16,
}

fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Shape> {
    let (first, first_target, member_offset) = output
        .instructions
        .windows(6)
        .enumerate()
        .find_map(|(index, window)| first_test(window).map(|value| (index, value.0, value.1)))?;
    let second = first_target;
    let (second_target, second_offset) =
        second_test(output.instructions.get(second..second + 5)?)?;
    if second_offset != member_offset
        || !address_pair(output, first, first + 2)
        || !address_pair(output, second, second + 1)
        || !schedule_relocations::same_target_value(
            &output.relocations,
            &output.constants,
            first,
            second,
        )
    {
        return None;
    }
    let default = find_default_formatter(output, member_offset)?;
    if default != second_target
        || !address_pair(output, default, default + 2)
        || !address_pair(output, default + 1, default + 3)
        || !address_pair(output, default + 4, default + 6)
        || !schedule_relocations::same_target_value(
            &output.relocations,
            &output.constants,
            first,
            default,
        )
        || output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if (first + 1..first + 5).contains(target)
                        || (second + 1..second + 4).contains(target)
                        || (default + 1..default + 7).contains(target)
            )
        })
    {
        return None;
    }
    Some(Shape {
        first,
        second,
        member_offset,
    })
}

fn first_test(window: &[Instruction]) -> Option<(usize, i16)> {
    match window {
        [
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::StoreWord { s: 0, a: 1, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::LoadWord { d: 0, a: 3, offset },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { target, .. },
        ] => Some((*target, *offset)),
        _ => None,
    }
}

fn second_test(window: &[Instruction]) -> Option<(usize, i16)> {
    match window {
        [
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::LoadWord { d: 0, a: 3, offset },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward { target, .. },
        ] => Some((*target, *offset)),
        _ => None,
    }
}

fn find_default_formatter(
    output: &mwcc_machine_code::MachineFunction,
    member_offset: i16,
) -> Option<usize> {
    output.instructions.windows(9).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted { d: 3, a: 0, .. },
                Instruction::AddImmediateShifted { d: 5, a: 0, .. },
                Instruction::AddImmediate { d: 4, a: 3, .. },
                Instruction::AddImmediate { d: 3, a: 5, .. },
                Instruction::AddImmediateShifted { d: 6, a: 0, .. },
                Instruction::LoadWord { d: 5, a: 4, offset },
                Instruction::AddImmediate { d: 4, a: 6, .. },
                Instruction::ConditionRegisterClear { d: 6 },
                Instruction::BranchAndLink { target },
            ] if *offset == member_offset && target == "sprintf"
        )
    })
}

fn address_pair(
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

fn cache_first_load(instructions: &mut [Instruction], first: usize) {
    let Instruction::LoadWord { d, .. } = &mut instructions[first + 3] else {
        unreachable!()
    };
    *d = 5;
    let Instruction::CompareLogicalWordImmediate { a, .. } =
        &mut instructions[first + 4]
    else {
        unreachable!()
    };
    *a = 5;
}

fn remove_second_load(generator: &mut Generator, second: usize) {
    for index in (second..=second + 2).rev() {
        generator.remove_structured_condition_instruction(index);
    }
    let Instruction::CompareLogicalWordImmediate { a, .. } =
        &mut generator.output.instructions[second]
    else {
        unreachable!()
    };
    *a = 5;
}

fn collapse_default_formatter(generator: &mut Generator, start: usize) {
    let mut buffer_high = generator.output.instructions[start + 1].clone();
    let mut buffer_low = generator.output.instructions[start + 3].clone();
    let mut string_high = generator.output.instructions[start + 4].clone();
    let mut string_low = generator.output.instructions[start + 6].clone();
    let Instruction::AddImmediateShifted { d, .. } = &mut buffer_high else {
        unreachable!()
    };
    *d = 3;
    let Instruction::AddImmediate { d, a, .. } = &mut buffer_low else {
        unreachable!()
    };
    *d = 3;
    *a = 3;
    let Instruction::AddImmediateShifted { d, .. } = &mut string_high else {
        unreachable!()
    };
    *d = 4;
    let Instruction::AddImmediate { d, a, .. } = &mut string_low else {
        unreachable!()
    };
    *d = 4;
    *a = 4;

    generator.output.instructions[start..start + 4].clone_from_slice(&[
        string_high,
        buffer_high,
        string_low,
        buffer_low,
    ]);
    generator.output.relocations.retain(|relocation| {
        relocation.instruction_index != start
            && relocation.instruction_index != start + 2
    });
    for relocation in &mut generator.output.relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == start + 1 => start + 1,
            index if index == start + 3 => start + 3,
            index if index == start + 4 => start,
            index if index == start + 6 => start + 2,
            index => index,
        };
    }
    for index in (start + 4..=start + 6).rev() {
        generator.remove_structured_condition_instruction(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{
        MachineFunction, Relocation, RelocationKind, RelocationTarget,
    };

    fn relocation(index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn recognizes_one_member_across_two_tests_and_the_default_formatter() {
        let mut output = MachineFunction::new("probe");
        output.instructions = vec![
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 0, a: 3, offset: 44 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 8 },
            Instruction::Branch { target: 22 },
            Instruction::Branch { target: 22 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 0, a: 3, offset: 44 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 14 },
            Instruction::Branch { target: 22 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 5, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 5, immediate: 0 },
            Instruction::AddImmediateShifted { d: 6, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 5, a: 4, offset: 44 },
            Instruction::AddImmediate { d: 4, a: 6, immediate: 0 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "sprintf".into() },
        ];
        output.relocations = vec![
            relocation(0, RelocationKind::Addr16Ha, "global"),
            relocation(2, RelocationKind::Addr16Lo, "global"),
            relocation(8, RelocationKind::Addr16Ha, "global"),
            relocation(9, RelocationKind::Addr16Lo, "global"),
            relocation(14, RelocationKind::Addr16Ha, "global"),
            relocation(15, RelocationKind::Addr16Ha, "buffer"),
            relocation(16, RelocationKind::Addr16Lo, "global"),
            relocation(17, RelocationKind::Addr16Lo, "buffer"),
            relocation(18, RelocationKind::Addr16Ha, "format"),
            relocation(20, RelocationKind::Addr16Lo, "format"),
        ];
        let shape = recognize(&output).expect("guarded formatter cache");
        assert_eq!(shape.first, 0);
        assert_eq!(shape.second, 8);
        assert_eq!(shape.member_offset, 44);
    }
}
