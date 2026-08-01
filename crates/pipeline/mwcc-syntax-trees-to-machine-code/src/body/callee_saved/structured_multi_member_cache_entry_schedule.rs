//! Entry issue order for a retained aggregate base with multiple member homes.
//!
//! Semantic lowering defines each saved address before storing it. Optimized
//! MWCC fills the saved-register store latency by issuing each store immediately
//! before the address definition whose old register value is dead. This pass is
//! deliberately late: relocation-bearing moves must see the final physical
//! stream after the general global-memory schedules.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_multi_member_cache_entry(&mut self) {
        let [first, second, ..] = self.structured_global_member_address_caches.as_slice() else {
            return;
        };
        if !first.initialized
            || !second.initialized
            || first.global != second.global
            || external_target(
                &self.output.relocations,
                1,
                RelocationKind::Addr16Ha,
            ) != Some(first.global.as_str())
            || external_target(
                &self.output.relocations,
                4,
                RelocationKind::Addr16Lo,
            ) != Some(first.global.as_str())
            || !is_serial_entry(&self.output.instructions, first.offset, second.offset)
        {
            return;
        }

        self.move_instruction_before(5, 4);
        self.move_instruction_before(7, 6);
        self.move_instruction_before(9, 8);
    }
}

fn is_serial_entry(instructions: &[Instruction], first_offset: i16, second_offset: i16) -> bool {
    match instructions {
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 31,
                immediate: emitted_first_offset,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 31,
                immediate: emitted_second_offset,
            },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 28,
                a: 1,
                offset: 16,
            },
            ..
        ]
        if *emitted_first_offset == first_offset && *emitted_second_offset == second_offset => true,
        _ => false,
    }
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
