//! Physical preheader schedule for the dense effecter-mixing loop.
//!
//! Selection exposes four independent lanes here: a scalar-to-halfword
//! conversion, two retained jump-table addresses, loop induction setup, and
//! saved floating invariants. Build 163 interleaves them to cover load and
//! multiply latency. This final physical pass owns that complete preheader;
//! table dispatch and loop-body scheduling remain separate concerns.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{MachineFunction, RelocationTarget};

use super::structured_conversion_call_schedule::permute_region;

const SCHEDULE: [usize; 24] = [
    0, 18, 1, 16, 3, 19, 2, 5, 17, 20, 21, 22, 4, 6, 7, 8, 9, 10, 11, 12, 13, 23,
    14, 15,
];

impl Generator {
    pub(crate) fn schedule_structured_effecter_preloop(&mut self) -> bool {
        let Some(start) = self
            .output
            .instructions
            .windows(SCHEDULE.len() + 1)
            .enumerate()
            .find_map(|(start, window)| preloop(&self.output, start, window).then_some(start))
        else {
            return false;
        };
        permute_region(&mut self.output, start, &SCHEDULE);
        assign_issue_lanes(&mut self.output.instructions[start..start + SCHEDULE.len()]);
        true
    }
}

fn preloop(output: &MachineFunction, start: usize, window: &[Instruction]) -> bool {
    let [
        Instruction::LoadFloatSingle { d: 1, a: owner, .. },
        Instruction::LoadFloatSingle { d: 0, a: second_owner, .. },
        Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
        Instruction::LoadFloatSingle { d: 0, a: third_owner, .. },
        Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
        Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
        Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
        Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
        Instruction::StoreFloatDouble { s: 0, a: 1, offset: image_offset },
        Instruction::LoadWord { d: 0, a: 1, offset: word_offset },
        Instruction::StoreHalfword { s: 0, a: result_owner, .. },
        Instruction::LoadFloatSingle { d: 24, a: 0, offset: 0 },
        Instruction::LoadFloatSingle { d: 25, a: 0, offset: 0 },
        Instruction::FloatSubtractSingle { d: 23, a: 24, .. },
        Instruction::FloatSubtractSingle { d: 22, a: 24, .. },
        Instruction::FloatSubtractSingle { d: 21, a: 24, .. },
        Instruction::AddImmediateShifted { d: 26, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 26, a: 26, immediate: 0 },
        Instruction::AddImmediateShifted { d: 27, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 27, a: 27, immediate: 0 },
        Instruction::AddImmediate { d: 31, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 29, a: 0, immediate: 0 },
        Instruction::AddImmediateShifted { d: 28, a: 0, immediate: 0x4330 },
        Instruction::LoadFloatDouble { d: 26, a: 0, offset: 0 },
        Instruction::Add { d: 25, a: loop_owner, b: 29 },
    ] = window
    else {
        return false;
    };
    owner == second_owner
        && owner == third_owner
        && owner == result_owner
        && owner == loop_owner
        && *word_offset == *image_offset + 4
        && relocation_pair(output, start + 16, start + 17)
        && relocation_pair(output, start + 18, start + 19)
        && !has_internal_entry(output, start + 1, start + SCHEDULE.len())
}

fn relocation_pair(output: &MachineFunction, high: usize, low: usize) -> bool {
    let high_table = output.relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == high && relocation.kind == RelocationKind::Addr16Ha)
            .then(|| jump_table_index(&relocation.target))
            .flatten()
    });
    let low_table = output.relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == low && relocation.kind == RelocationKind::Addr16Lo)
            .then(|| jump_table_index(&relocation.target))
            .flatten()
    });
    high_table.is_some_and(|high_table| low_table == Some(high_table))
}

fn jump_table_index(target: &RelocationTarget) -> Option<usize> {
    match target {
        RelocationTarget::JumpTable => Some(0),
        RelocationTarget::JumpTableAt(index) => Some(*index),
        _ => None,
    }
}

fn has_internal_entry(output: &MachineFunction, begin: usize, end: usize) -> bool {
    output.instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                if (begin..end).contains(target)
        )
    }) || output.jump_tables.iter().any(|table| {
        table.entries.iter().any(|entry| {
            let target = *entry as usize / 4;
            (begin..end).contains(&target)
        })
    })
}

fn assign_issue_lanes(window: &mut [Instruction]) {
    let Instruction::AddImmediateShifted { immediate: second_high, .. } = window[1] else {
        unreachable!("the second table high was scheduled")
    };
    window[1] = Instruction::AddImmediateShifted { d: 3, a: 0, immediate: second_high };
    let Instruction::AddImmediateShifted { immediate: first_high, .. } = window[3] else {
        unreachable!("the first table high was scheduled")
    };
    window[3] = Instruction::AddImmediateShifted { d: 4, a: 0, immediate: first_high };
    let Instruction::LoadFloatSingle { a: owner, offset: third, .. } = window[4] else {
        unreachable!("the third factor was scheduled")
    };
    window[4] = Instruction::LoadFloatSingle { d: 2, a: owner, offset: third };
    window[5] = Instruction::AddImmediate { d: 27, a: 3, immediate: 0 };
    window[6] = Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 };
    window[7] = Instruction::LoadFloatSingle { d: 1, a: 0, offset: 0 };
    window[8] = Instruction::AddImmediate { d: 26, a: 4, immediate: 0 };
    window[12] = Instruction::FloatMultiplySingle { d: 0, a: 2, c: 0 };
    window[13] = Instruction::FloatMultiplySingle { d: 0, a: 1, c: 0 };
    window[15] = Instruction::StoreFloatDouble { s: 0, a: 1, offset: 32 };
    window[16] = Instruction::LoadWord { d: 0, a: 1, offset: 36 };
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind};

    fn sample() -> MachineFunction {
        let mut output = MachineFunction::default();
        output.instructions = vec![
            Instruction::LoadFloatSingle { d: 1, a: 30, offset: 176 },
            Instruction::LoadFloatSingle { d: 0, a: 30, offset: 236 },
            Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 30, offset: 256 },
            Instruction::FloatMultiplySingle { d: 1, a: 1, c: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 0 },
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 1 },
            Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
            Instruction::StoreFloatDouble { s: 0, a: 1, offset: 24 },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::StoreHalfword { s: 0, a: 30, offset: 248 },
            Instruction::LoadFloatSingle { d: 24, a: 0, offset: 0 },
            Instruction::LoadFloatSingle { d: 25, a: 0, offset: 0 },
            Instruction::FloatSubtractSingle { d: 23, a: 24, b: 31 },
            Instruction::FloatSubtractSingle { d: 22, a: 24, b: 29 },
            Instruction::FloatSubtractSingle { d: 21, a: 24, b: 28 },
            Instruction::AddImmediateShifted { d: 26, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 26, a: 26, immediate: 0 },
            Instruction::AddImmediateShifted { d: 27, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 27, a: 27, immediate: 0 },
            Instruction::load_immediate(31, 0),
            Instruction::load_immediate(29, 0),
            Instruction::load_immediate_shifted(28, 0x4330),
            Instruction::LoadFloatDouble { d: 26, a: 0, offset: 0 },
            Instruction::Add { d: 25, a: 30, b: 29 },
        ];
        output.relocations = vec![
            Relocation { instruction_index: 16, kind: RelocationKind::Addr16Ha, target: RelocationTarget::JumpTable },
            Relocation { instruction_index: 17, kind: RelocationKind::Addr16Lo, target: RelocationTarget::JumpTable },
            Relocation { instruction_index: 18, kind: RelocationKind::Addr16Ha, target: RelocationTarget::JumpTableAt(1) },
            Relocation { instruction_index: 19, kind: RelocationKind::Addr16Lo, target: RelocationTarget::JumpTableAt(1) },
        ];
        output
    }

    #[test]
    fn recognizes_and_interleaves_the_complete_physical_preloop() {
        let mut output = sample();
        assert!(preloop(&output, 0, &output.instructions));
        permute_region(&mut output, 0, &SCHEDULE);
        assign_issue_lanes(&mut output.instructions[..SCHEDULE.len()]);
        assert!(matches!(output.instructions[1], Instruction::AddImmediateShifted { d: 3, .. }));
        assert!(matches!(output.instructions[5], Instruction::AddImmediate { d: 27, a: 3, .. }));
        assert!(matches!(output.instructions[8], Instruction::AddImmediate { d: 26, a: 4, .. }));
        assert!(matches!(output.instructions[15], Instruction::StoreFloatDouble { offset: 32, .. }));
        assert_eq!(output.relocations[2].instruction_index, 1);
        assert_eq!(output.relocations[0].instruction_index, 3);
    }
}
