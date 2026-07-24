//! Scheduling for a reciprocal replicated into a frame-resident float vector.
//!
//! The expression selector deliberately keeps literal and member addressing
//! generic. Once their physical homes are known, MWCC exposes two additional
//! choices: the first pointer load fills the constant-load latency window, and
//! the divide overwrites its dying divisor before three adjacent frame stores.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_reciprocal_frame_fill(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        if let Some(start) = schedule(&mut self.output) {
            self.labels.moved_before(start + 1, start);
        }
    }
}

fn schedule(output: &mut mwcc_machine_code::MachineFunction) -> Option<usize> {
    let Some((start, result, divisor)) = output
        .instructions
        .windows(8)
        .enumerate()
        .find_map(|(start, window)| reciprocal_frame_fill(window).map(|pair| (start, pair.0, pair.1)))
    else {
        return None;
    };
    if !output.relocations.iter().any(|relocation| {
        relocation.instruction_index == start && relocation.kind == RelocationKind::EmbSda21
    }) || output
        .relocations
        .iter()
        .any(|relocation| relocation.instruction_index == start + 1)
    {
        return None;
    }

    output.instructions.swap(start, start + 1);
    for relocation in &mut output.relocations {
        if relocation.instruction_index == start {
            relocation.instruction_index = start + 1;
        }
    }
    output.instructions[start + 4] = Instruction::FloatDivideSingle {
        d: divisor,
        a: result,
        b: divisor,
    };
    for instruction in &mut output.instructions[start + 5..start + 8] {
        let Instruction::StoreFloatSingle { s, .. } = instruction else {
            unreachable!("the reciprocal fill stores were matched above")
        };
        *s = divisor;
    }
    Some(start)
}

fn reciprocal_frame_fill(window: &[Instruction]) -> Option<(u8, u8)> {
    let [
        Instruction::LoadFloatSingle {
            d: result,
            a: 0,
            ..
        },
        Instruction::LoadWord {
            d: first_base,
            a: entry_base,
            ..
        },
        Instruction::LoadWord {
            d: second_base,
            a: first_input,
            ..
        },
        Instruction::LoadFloatSingle {
            d: divisor,
            a: final_base,
            ..
        },
        Instruction::FloatDivideSingle {
            d: divide_result,
            a: numerator,
            b: denominator,
        },
        Instruction::StoreFloatSingle {
            s: first_value,
            a: 1,
            offset: first_offset,
        },
        Instruction::StoreFloatSingle {
            s: second_value,
            a: 1,
            offset: second_offset,
        },
        Instruction::StoreFloatSingle {
            s: third_value,
            a: 1,
            offset: third_offset,
        },
    ] = window
    else {
        return None;
    };
    (*entry_base != 0
        && first_base == second_base
        && first_base == first_input
        && second_base == final_base
        && result == divide_result
        && result == numerator
        && divisor == denominator
        && result != divisor
        && result == first_value
        && result == second_value
        && result == third_value
        && *first_offset == second_offset.saturating_add(4)
        && *second_offset == third_offset.saturating_add(4))
    .then_some((*result, *divisor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> mwcc_machine_code::MachineFunction {
        let mut output = mwcc_machine_code::MachineFunction::new("reciprocal_fill");
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 },
            Instruction::LoadWord { d: 3, a: 3, offset: 268 },
            Instruction::LoadWord { d: 3, a: 3, offset: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 3, offset: 140 },
            Instruction::FloatDivideSingle { d: 1, a: 1, b: 0 },
            Instruction::StoreFloatSingle { s: 1, a: 1, offset: 24 },
            Instruction::StoreFloatSingle { s: 1, a: 1, offset: 20 },
            Instruction::StoreFloatSingle { s: 1, a: 1, offset: 16 },
        ];
        output.relocations.push(mwcc_machine_code::Relocation {
            instruction_index: 0,
            kind: RelocationKind::EmbSda21,
            target: mwcc_machine_code::RelocationTarget::Constant(0),
        });
        output
    }

    #[test]
    fn schedules_the_pointer_load_and_reuses_the_divisor() {
        let mut output = candidate();

        assert_eq!(schedule(&mut output), Some(0));
        assert!(matches!(output.instructions[0], Instruction::LoadWord { .. }));
        assert!(matches!(output.instructions[1], Instruction::LoadFloatSingle { d: 1, a: 0, .. }));
        assert!(matches!(output.instructions[4], Instruction::FloatDivideSingle { d: 0, a: 1, b: 0 }));
        assert!(output.instructions[5..].iter().all(|instruction| {
            matches!(instruction, Instruction::StoreFloatSingle { s: 0, .. })
        }));
        assert_eq!(output.relocations[0].instruction_index, 1);
    }

    #[test]
    fn rejects_a_nonconstant_first_load() {
        let mut output = candidate();
        output.relocations.clear();

        assert_eq!(schedule(&mut output), None);
    }
}
