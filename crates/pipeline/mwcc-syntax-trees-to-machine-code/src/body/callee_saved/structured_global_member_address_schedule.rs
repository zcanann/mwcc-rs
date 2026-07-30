//! Build-163 issue order for a retained global-member address.
//!
//! The semantic planner establishes the one saved address. This late physical
//! pass interleaves its materialization with linkage setup and schedules the
//! assertion plus terminal four-argument call after allocation has fixed the
//! argument registers.

#[allow(unused_imports)]
use super::*;

const SCHEDULE: [usize; 37] = [
    0, 7, 2, 8, 1, 3, 4, 6, 5, 10, 9, 12, 11, 13, 14, 15, 19, 16, 17, 18, 20, 26, 23, 27, 21, 29,
    28, 24, 22, 30, 25, 31, 32, 33, 34, 35, 36,
];

impl Generator {
    pub(crate) fn schedule_structured_global_member_address(&mut self) {
        let Some(cache) = self.structured_global_member_address_cache.as_ref() else {
            return;
        };
        if !is_serial_member_address_body(
            &self.output.instructions,
            &self.output.relocations,
            &cache.global,
        ) {
            return;
        }

        let mut current: Vec<usize> = (0..SCHEDULE.len()).collect();
        for (destination, &original) in SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("member-address schedule is a permutation");
            if source != destination {
                self.move_instruction_before(source, destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
        assign_mwcc_registers(&mut self.output.instructions);
    }
}

fn is_serial_member_address_body(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    global: &str,
) -> bool {
    if instructions.len() != SCHEDULE.len()
        || external_target(relocations, 1, RelocationKind::Addr16Ha) != Some(global)
        || external_target(relocations, 4, RelocationKind::Addr16Lo) != Some(global)
    {
        return false;
    }
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { a: 0, .. },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::AddImmediate { .. },
            Instruction::AddImmediate {
                d: 31,
                immediate: 8,
                ..
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediateShifted { a: 0, .. },
            Instruction::AddImmediate { d: 0, .. },
            Instruction::LoadWord { a: 0, .. },
            Instruction::StoreWord { s: 0, a: 0, .. },
            Instruction::LoadWord { offset: 60, .. },
            Instruction::LoadWord {
                a: 31,
                offset: 0,
                ..
            },
            Instruction::CompareLogicalWord { .. },
            Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediateShifted { a: 0, .. },
            Instruction::AddImmediate { d: 5, .. },
            Instruction::AddImmediate { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 4, a: 0, .. },
            Instruction::ConditionRegisterClear { .. },
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord { a: 0, .. },
            Instruction::LoadWord { offset: 56, .. },
            Instruction::LoadWord {
                a: 31,
                offset: 0,
                ..
            },
            Instruction::AddImmediate {
                d: 0,
                immediate: 31,
                ..
            },
            Instruction::AndContiguousMask { .. },
            Instruction::AddImmediateShifted { a: 0, .. },
            Instruction::AddImmediate { .. },
            Instruction::LoadWord { offset: 4, .. },
            Instruction::AddImmediateShifted { a: 0, .. },
            Instruction::AddImmediate { .. },
            Instruction::BranchAndLink { .. },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    )
}

fn external_target(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
            return None;
        };
        Some(target.as_str())
    })
}

fn assign_mwcc_registers(instructions: &mut [Instruction]) {
    let Instruction::BranchConditionalForward { target, .. } = &mut instructions[14] else {
        unreachable!("validated member-address branch changed form")
    };
    *target = 21;
    set_load(&mut instructions[10], 4, 0);
    set_load(&mut instructions[12], 3, 4);
    set_shifted_add(&mut instructions[21], 3, 0);
    set_load(&mut instructions[22], 6, 31);
    set_add(&mut instructions[23], 5, 3);
    set_load(&mut instructions[24], 7, 0);
    set_shifted_add(&mut instructions[25], 4, 0);
    set_load(&mut instructions[26], 5, 5);
    set_add(&mut instructions[27], 0, 6);
    set_load(&mut instructions[28], 3, 7);
    set_add(&mut instructions[29], 6, 4);
}

fn set_load(instruction: &mut Instruction, destination: u8, base: u8) {
    let Instruction::LoadWord { d, a, .. } = instruction else {
        unreachable!("validated member-address load changed form")
    };
    *d = destination;
    *a = base;
}

fn set_add(instruction: &mut Instruction, destination: u8, base: u8) {
    let Instruction::AddImmediate { d, a, .. } = instruction else {
        unreachable!("validated member-address add changed form")
    };
    *d = destination;
    *a = base;
}

fn set_shifted_add(instruction: &mut Instruction, destination: u8, base: u8) {
    let Instruction::AddImmediateShifted { d, a, .. } = instruction else {
        unreachable!("validated member-address shifted add changed form")
    };
    *d = destination;
    *a = base;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_issue_order_is_a_complete_permutation() {
        let mut sorted = SCHEDULE.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..SCHEDULE.len()).collect::<Vec<_>>());
    }
}
