//! Final register and invariant schedule for a global-structure binary search.
//!
//! The structured emitter exposes the complete binary-search topology, but its
//! ordinary local allocator materializes the global array base in every
//! iteration. MWCC retains that invariant in the sixth saved GPR, reuses the
//! comparison's CR0 value for both sign branches, and consequently saves the
//! dense `r26..r31` range with `stmw`/`lmw`.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::Relocation;

impl Generator {
    pub(crate) fn schedule_global_struct_binary_search(&mut self) {
        let Some(plan) = recognize(&self.output) else {
            return;
        };

        let mut call_relocation = plan.call_relocation;
        call_relocation.instruction_index = 14;
        let mut global_high_relocation = plan.global_high_relocation;
        global_high_relocation.instruction_index = 24;
        let mut global_low_relocation = plan.global_low_relocation;
        global_low_relocation.instruction_index = 25;

        self.output.instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 8,
            },
            Instruction::move_register(26, 3),
            Instruction::load_immediate(30, 0),
            Instruction::load_immediate(29, plan.end),
            Instruction::Branch { target: 24 },
            Instruction::Add {
                d: 0,
                a: 30,
                b: 29,
            },
            Instruction::move_register(3, 26),
            Instruction::ShiftRightLogicalImmediate {
                a: 28,
                s: 0,
                shift: 1,
            },
            Instruction::MultiplyImmediate {
                d: 0,
                a: 28,
                immediate: plan.stride,
            },
            Instruction::Add {
                d: 27,
                a: 31,
                b: 0,
            },
            Instruction::move_register(4, 27),
            Instruction::BranchAndLink {
                target: plan.callee,
            },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 0,
                target: 19,
            },
            Instruction::move_register(29, 28),
            Instruction::Branch { target: 26 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 22,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 28,
                immediate: 1,
            },
            Instruction::Branch { target: 26 },
            Instruction::move_register(3, 27),
            Instruction::Branch { target: 29 },
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
            Instruction::CompareLogicalWord { a: 30, b: 29 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 8,
            },
            Instruction::load_immediate(3, 0),
            Instruction::LoadMultipleWord {
                d: 26,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ];
        self.output.relocations = vec![
            call_relocation,
            global_high_relocation,
            global_low_relocation,
        ];
    }
}

struct Plan {
    end: i16,
    stride: i16,
    callee: String,
    global_high_relocation: Relocation,
    global_low_relocation: Relocation,
    call_relocation: Relocation,
}

fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Plan> {
    let [
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 36 },
        Instruction::StoreWord { s: 31, a: 1, offset: 28 },
        Instruction::AddImmediate { d: 31, a: 0, immediate: 0 },
        Instruction::StoreWord { s: 30, a: 1, offset: 24 },
        Instruction::AddImmediate { d: 30, a: 0, immediate: end },
        Instruction::StoreWord { s: 29, a: 1, offset: 20 },
        Instruction::Or { a: 29, s: 3, b: 3 },
        Instruction::StoreWord { s: 28, a: 1, offset: 16 },
        Instruction::StoreWord { s: 27, a: 1, offset: 12 },
        Instruction::Branch { target: 31 },
        Instruction::Add { d: 0, a: 31, b: 30 },
        Instruction::ShiftRightLogicalImmediate { a: 28, s: 0, shift: 1 },
        Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
        Instruction::MultiplyImmediate { d: 4, a: 28, immediate: stride },
        Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
        Instruction::Add { d: 27, a: 0, b: 4 },
        Instruction::Or { a: 3, s: 29, b: 29 },
        Instruction::Or { a: 4, s: 27, b: 27 },
        Instruction::BranchAndLink { target: callee },
        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 25,
        },
        Instruction::Or { a: 30, s: 28, b: 28 },
        Instruction::Branch { target: 31 },
        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 29,
        },
        Instruction::AddImmediate { d: 31, a: 28, immediate: 1 },
        Instruction::Branch { target: 31 },
        Instruction::Or { a: 3, s: 27, b: 27 },
        Instruction::Branch { target: 34 },
        Instruction::CompareLogicalWord { a: 31, b: 30 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: 12,
        },
        Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
        Instruction::LoadWord { d: 31, a: 1, offset: 28 },
        Instruction::LoadWord { d: 30, a: 1, offset: 24 },
        Instruction::LoadWord { d: 29, a: 1, offset: 20 },
        Instruction::LoadWord { d: 28, a: 1, offset: 16 },
        Instruction::LoadWord { d: 0, a: 1, offset: 36 },
        Instruction::LoadWord { d: 27, a: 1, offset: 12 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
        Instruction::BranchToLinkRegister,
    ] = output.instructions.as_slice()
    else {
        return None;
    };
    if output.relocations.len() != 3
        || !schedule_relocations::same_target_value(
            &output.relocations,
            &output.constants,
            14,
            16,
        )
    {
        return None;
    }
    let global_high_relocation = relocation_at(output, 14, RelocationKind::Addr16Ha)?;
    let global_low_relocation = relocation_at(output, 16, RelocationKind::Addr16Lo)?;
    let call_relocation = relocation_at(output, 20, RelocationKind::Rel24)?;
    Some(Plan {
        end: *end,
        stride: *stride,
        callee: callee.clone(),
        global_high_relocation,
        global_low_relocation,
        call_relocation,
    })
}

fn relocation_at(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<Relocation> {
    output
        .relocations
        .iter()
        .find(|relocation| {
            relocation.instruction_index == instruction_index && relocation.kind == kind
        })
        .cloned()
}

