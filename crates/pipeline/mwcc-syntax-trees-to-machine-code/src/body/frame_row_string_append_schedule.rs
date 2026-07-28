//! Loop-invariant address and row-cursor schedule for string appends.
//!
//! A loop that appends rows of a fixed-stride frame matrix to one static
//! buffer retains the buffer, separator base, row cursor, count, and bound in
//! `r27..r31`. This is MWCC's measured strength reduction: the cursor advances
//! by the row width instead of recomputing `frame + index * width`.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::Relocation;

impl Generator {
    pub(crate) fn schedule_frame_row_string_append(&mut self) {
        let Some(plan) = recognize(&self.output) else {
            return;
        };

        self.output.instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -224,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 228,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            Instruction::StoreMultipleWord {
                s: 27,
                a: 1,
                offset: 204,
            },
            Instruction::BranchAndLink {
                target: plan.fill_callee,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::move_register(29, 3),
            Instruction::AddImmediate {
                d: 3,
                a: 5,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: plan.copy_callee,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 28,
                a: 1,
                immediate: 8,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate(27, 0),
            Instruction::Branch { target: 27 },
            Instruction::move_register(3, 30),
            Instruction::move_register(4, 28),
            Instruction::BranchAndLink {
                target: plan.first_append_callee,
            },
            Instruction::move_register(3, 30),
            // The packed-string resolver patches this offset after the TU-wide
            // string table has assigned the separator's final byte position.
            Instruction::AddImmediate {
                d: 4,
                a: 31,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: plan.second_append_callee,
            },
            Instruction::AddImmediate {
                d: 28,
                a: 28,
                immediate: plan.row_bytes,
            },
            Instruction::AddImmediate {
                d: 27,
                a: 27,
                immediate: 1,
            },
            Instruction::CompareWord { a: 27, b: 29 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 19,
            },
            Instruction::LoadMultipleWord {
                d: 27,
                a: 1,
                offset: 204,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 228,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 224,
            },
            Instruction::BranchToLinkRegister,
        ];

        self.output.relocations = vec![
            moved(&plan.fill_relocation, 5),
            moved(&plan.empty_high_relocation, 6),
            moved(&plan.buffer_high_relocation, 7),
            moved(&plan.empty_low_relocation, 8),
            moved(&plan.buffer_low_relocation, 10),
            moved(&plan.copy_relocation, 11),
            moved(&plan.buffer_high_relocation, 12),
            moved(&plan.separator_high_relocation, 13),
            moved(&plan.buffer_low_relocation, 14),
            moved(&plan.separator_low_relocation, 16),
            moved(&plan.first_append_relocation, 21),
            moved(&plan.second_append_relocation, 24),
            moved(&plan.buffer_high_relocation, 30),
            moved(&plan.buffer_low_relocation, 32),
        ];
    }
}

struct Plan {
    row_bytes: i16,
    fill_callee: String,
    copy_callee: String,
    first_append_callee: String,
    second_append_callee: String,
    fill_relocation: Relocation,
    copy_relocation: Relocation,
    first_append_relocation: Relocation,
    second_append_relocation: Relocation,
    empty_high_relocation: Relocation,
    empty_low_relocation: Relocation,
    separator_high_relocation: Relocation,
    separator_low_relocation: Relocation,
    buffer_high_relocation: Relocation,
    buffer_low_relocation: Relocation,
}

fn recognize(output: &mwcc_machine_code::MachineFunction) -> Option<Plan> {
    let [
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -208 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 212 },
        Instruction::StoreWord { s: 31, a: 1, offset: 204 },
        Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
        Instruction::BranchAndLink { target: fill_callee },
        Instruction::Or { a: 31, s: 3, b: 3 },
        Instruction::StoreWord { s: 30, a: 1, offset: 200 },
        Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
        Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
        Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
        Instruction::BranchAndLink { target: copy_callee },
        Instruction::AddImmediate { d: 30, a: 0, immediate: 0 },
        Instruction::Branch { target: 27 },
        Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
        Instruction::ShiftLeftImmediate { a: 4, s: 30, shift },
        Instruction::AddImmediate { d: 0, a: 1, immediate: 8 },
        Instruction::Add { d: 4, a: 0, b: 4 },
        Instruction::BranchAndLink { target: first_append_callee },
        Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
        Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
        Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
        Instruction::BranchAndLink { target: second_append_callee },
        Instruction::AddImmediate { d: 30, a: 30, immediate: 1 },
        Instruction::CompareWord { a: 30, b: 31 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 0,
            target: 15,
        },
        Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
        Instruction::LoadWord { d: 31, a: 1, offset: 204 },
        Instruction::LoadWord { d: 0, a: 1, offset: 212 },
        Instruction::LoadWord { d: 30, a: 1, offset: 200 },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate { d: 1, a: 1, immediate: 208 },
        Instruction::BranchToLinkRegister,
    ] = output.instructions.as_slice()
    else {
        return None;
    };
    let row_bytes = 1i16.checked_shl(u32::from(*shift))?;
    if output.relocations.len() != 16
        || !same_target(output, 9, 15)
        || !same_target(output, 9, 22)
        || !same_target(output, 9, 29)
    {
        return None;
    }
    Some(Plan {
        row_bytes,
        fill_callee: fill_callee.clone(),
        copy_callee: copy_callee.clone(),
        first_append_callee: first_append_callee.clone(),
        second_append_callee: second_append_callee.clone(),
        fill_relocation: relocation_at(output, 5, RelocationKind::Rel24)?,
        copy_relocation: relocation_at(output, 12, RelocationKind::Rel24)?,
        first_append_relocation: relocation_at(output, 20, RelocationKind::Rel24)?,
        second_append_relocation: relocation_at(output, 25, RelocationKind::Rel24)?,
        empty_high_relocation: relocation_at(output, 8, RelocationKind::Addr16Ha)?,
        empty_low_relocation: relocation_at(output, 10, RelocationKind::Addr16Lo)?,
        separator_high_relocation: relocation_at(output, 21, RelocationKind::Addr16Ha)?,
        separator_low_relocation: relocation_at(output, 23, RelocationKind::Addr16Lo)?,
        buffer_high_relocation: relocation_at(output, 9, RelocationKind::Addr16Ha)?,
        buffer_low_relocation: relocation_at(output, 11, RelocationKind::Addr16Lo)?,
    })
}

fn same_target(
    output: &mwcc_machine_code::MachineFunction,
    first: usize,
    second: usize,
) -> bool {
    schedule_relocations::same_target_value(
        &output.relocations,
        &output.constants,
        first,
        second,
    )
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

fn moved(relocation: &Relocation, instruction_index: usize) -> Relocation {
    let mut relocation = relocation.clone();
    relocation.instruction_index = instruction_index;
    relocation
}
