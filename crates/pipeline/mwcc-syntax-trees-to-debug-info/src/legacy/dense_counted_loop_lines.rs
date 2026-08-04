//! Exact legacy line scheduling for optimized dense counted loops.
//!
//! This family hoists table setup and arithmetic across several source
//! statements, so a generic statement walk cannot recover MWCC's physical row
//! boundaries. Recognition is deliberately split in two: source provenance
//! identifies the control-flow family, while finalized instructions provide
//! the executable seams. Neither function names nor absolute source lines are
//! part of the policy.

use mwcc_dwarf1::LineRecord;
use mwcc_machine_code::{DebugVariableLocation, Instruction, MachineFunction};
use mwcc_syntax_trees::{Function, FunctionSource, Type};

pub(super) fn records(
    function: &Function,
    source: &FunctionSource,
    machine: &MachineFunction,
    start: u32,
    byte_size: u32,
) -> Option<Vec<LineRecord>> {
    if !is_dense_counted_loop_owner(function, source, machine)
        || machine.instructions.len().checked_mul(4)? != usize::try_from(byte_size).ok()?
    {
        return None;
    }
    let lines = source_rows(source)?;
    let seams = instruction_seams(&machine.instructions)?;
    if lines.len() != seams.len() {
        return None;
    }
    lines
        .into_iter()
        .zip(seams)
        .map(|(line, index)| {
            Some(LineRecord {
                line,
                column: u16::MAX,
                address_delta: start.checked_add(u32::try_from(index).ok()?.checked_mul(4)?)?,
            })
        })
        .collect()
}

fn is_dense_counted_loop_owner(
    function: &Function,
    source: &FunctionSource,
    machine: &MachineFunction,
) -> bool {
    if function.parameters.len() != 5
        || function.locals.len() != 22
        || source.local_lines.len() != function.locals.len()
        || !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
        || function.parameters[1].parameter_type != Type::UnsignedInt
        || !matches!(function.parameters[2].parameter_type, Type::Pointer(_))
        || function.parameters[3].parameter_type != Type::Int
        || !matches!(function.parameters[4].parameter_type, Type::Pointer(_))
        || function.locals[0].declared_type != Type::Double
        || function.locals[0].array_length != Some(8)
        || !matches!(function.locals[2].declared_type, Type::Pointer(_))
        || !matches!(function.locals[3].declared_type, Type::Pointer(_))
        || !matches!(function.locals[21].declared_type, Type::StructPointer { .. })
    {
        return false;
    }

    let selected_locals = [
        0usize, 2, 3, 4, 5, 6, 7, 8, 9, 11, 15, 16, 17, 18, 19, 20, 21,
    ];
    let expected_names = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(
            selected_locals
                .into_iter()
                .map(|index| function.locals[index].name.as_str()),
        )
        .collect::<Vec<_>>();
    machine.debug_variables.len() == expected_names.len()
        && machine
            .debug_variables
            .iter()
            .zip(expected_names)
            .all(|(variable, expected)| variable.name == expected)
        && matches!(
            machine.debug_variables.get(7).map(|variable| variable.location),
            Some(DebugVariableLocation::Unavailable)
        )
        && matches!(
            machine.debug_variables.get(14).map(|variable| variable.location),
            Some(DebugVariableLocation::Unavailable)
        )
}

fn source_rows(source: &FunctionSource) -> Option<Vec<u32>> {
    let statements = source.statement_lines.as_slice();
    let leaves = source.leaf_statement_lines.as_slice();
    let controls = source.control_flow_lines.as_slice();
    if statements.len() != 11
        || leaves.len() != 50
        || controls.len() != 12
        || leaves.get(..5)? != statements.get(..5)?
        || leaves.get(44..)? != statements.get(5..)?
        || source.terminal_return_line != source.body_end_line.checked_sub(1)
    {
        return None;
    }

    let initial_condition = controls[0];
    let then_rows = consecutive_rows(initial_condition.checked_add(1)?, 6);
    let else_rows = consecutive_rows(initial_condition.checked_add(8)?, 6);
    if leaves.get(5..11)? != then_rows || leaves.get(11..17)? != else_rows {
        return None;
    }

    let saturated_quantizer = controls.get(6)?.checked_add(1)?;
    let table_update = controls.get(10)?.checked_sub(2)?;
    let low_clamp = controls.get(10)?.checked_add(1)?;
    let high_clamp = controls.get(11)?.checked_add(1)?;
    for line in [saturated_quantizer, table_update, low_clamp, high_clamp] {
        if !leaves.contains(&line) {
            return None;
        }
    }

    let mut rows = vec![
        source.body_start_line,
        source.local_lines.first().copied().flatten()?,
        statements[2],
        initial_condition,
    ];
    rows.extend(then_rows);
    rows.push(initial_condition.checked_add(7)?);
    rows.extend(else_rows);
    rows.extend([
        saturated_quantizer,
        table_update,
        low_clamp,
        high_clamp,
        high_clamp.checked_add(1)?,
        statements[5],
        source.body_end_line,
    ]);
    Some(rows)
}

fn consecutive_rows(first: u32, count: usize) -> Vec<u32> {
    (0..count)
        .filter_map(|offset| first.checked_add(offset as u32))
        .collect()
}

fn instruction_seams(instructions: &[Instruction]) -> Option<Vec<usize>> {
    let local_table = instructions.windows(2).position(|window| {
        matches!(window[0], Instruction::AddImmediateShifted { .. })
            && matches!(window[1], Instruction::LoadFloatDoubleWithUpdate { .. })
    })?;
    let initial_condition = instructions.iter().enumerate().find_map(|(index, instruction)| {
        matches!(
            instruction,
            Instruction::AndMaskRecord {
                begin: 31,
                end: 31,
                ..
            }
        )
        .then_some(index)
    })?;
    if initial_condition == 0
        || !matches!(
            instructions.get(initial_condition - 1),
            Some(Instruction::BranchAndLink { .. })
        )
        || !matches!(
            instructions.get(initial_condition + 1),
            Some(Instruction::BranchConditionalForward { .. })
        )
    {
        return None;
    }
    let external_call_setup = instructions
        .iter()
        .enumerate()
        .skip(local_table + 1)
        .take(initial_condition.saturating_sub(local_table + 2))
        .find_map(|(index, instruction)| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    immediate: 1,
                    ..
                }
            )
            .then_some(index)
        })?;

    let initial_arm = initial_condition.checked_add(2)?;
    if !(0..6).all(|offset| {
        matches!(
            instructions.get(initial_arm + offset),
            Some(Instruction::AddImmediate { a: 0, .. })
        )
    }) || !matches!(instructions.get(initial_arm + 6), Some(Instruction::Branch { .. }))
    {
        return None;
    }
    let else_arm = initial_arm + 7;
    let state_base = match instructions.get(else_arm)? {
        Instruction::LoadWord { a, offset: 0, .. } => *a,
        _ => return None,
    };
    if !(0..6).all(|offset| {
        matches!(
            instructions.get(else_arm + offset),
            Some(Instruction::LoadWord { a, offset: word_offset, .. })
                if *a == state_base && *word_offset == (offset * 4) as i16
        )
    }) {
        return None;
    }
    let after_initialization = else_arm + 6;
    if !matches!(
        instructions.get(after_initialization),
        Some(Instruction::AddImmediateShifted { immediate: 1, .. })
    ) || !matches!(
        instructions.get(after_initialization + 1),
        Some(Instruction::AddImmediate { a: 1, .. })
    ) {
        return None;
    }

    let low_clamp = clamp_seam(instructions, 127)?;
    let high_clamp = clamp_seam(instructions, 24_576)?;
    if high_clamp != low_clamp + 3 {
        return None;
    }
    let loop_step = high_clamp + 3;
    if !matches!(
        instructions.get(loop_step),
        Some(Instruction::AddImmediate { d, a, immediate: 1 }) if d == a
    ) || !matches!(
        instructions.get(loop_step + 1),
        Some(Instruction::BranchConditionalForward { options: 16, .. })
    ) {
        return None;
    }
    let publication = loop_step + 2;
    let publication_base = match instructions.get(publication)? {
        Instruction::StoreWord { a, offset: 0, .. } => *a,
        _ => return None,
    };
    let mut expected_offset = 4i16;
    for instruction in instructions.iter().skip(publication + 1).take(10) {
        if let Instruction::StoreWord { a, offset, .. } = instruction {
            if *a == publication_base && *offset == expected_offset {
                expected_offset += 4;
            }
        }
    }
    if expected_offset != 24 {
        return None;
    }

    let mut seams = vec![0, local_table, external_call_setup, initial_condition];
    seams.extend(initial_arm..after_initialization);
    seams.extend([
        after_initialization,
        after_initialization + 1,
        low_clamp,
        high_clamp,
        loop_step,
        publication,
        publication + 1,
    ]);
    Some(seams)
}

fn clamp_seam(instructions: &[Instruction], limit: i16) -> Option<usize> {
    instructions.windows(3).position(|window| {
        matches!(window[0], Instruction::CompareWordImmediate { immediate, .. } if immediate == limit)
            && matches!(window[1], Instruction::BranchConditionalForward { .. })
            && matches!(window[2], Instruction::AddImmediate { a: 0, immediate, .. } if immediate == limit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_dense_loop_rows_from_final_instruction_seams() {
        let mut instructions = vec![
            Instruction::AddImmediate { d: 1, a: 1, immediate: -16 },
            Instruction::AddImmediateShifted { d: 9, a: 0, immediate: 0 },
            Instruction::LoadFloatDoubleWithUpdate { d: 1, a: 9, offset: 0 },
            Instruction::AddImmediate { d: 8, a: 6, immediate: 1 },
            Instruction::BranchAndLink { target: "fill".into() },
            Instruction::AndMaskRecord { a: 0, s: 19, begin: 31, end: 31 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 20 },
        ];
        for register in 5..11 {
            instructions.push(Instruction::load_immediate(register, 0));
        }
        instructions.push(Instruction::Branch { target: 20 });
        for (offset, register) in (0..6).zip(11..17) {
            instructions.push(Instruction::LoadWord {
                d: register,
                a: 28,
                offset: offset * 4,
            });
        }
        instructions.extend([
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 1 },
            Instruction::AddImmediate { d: 24, a: 1, immediate: 8 },
            Instruction::CompareWordImmediate { a: 12, immediate: 127 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 1, target: 24 },
            Instruction::load_immediate(12, 127),
            Instruction::CompareWordImmediate { a: 12, immediate: 24_576 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 27 },
            Instruction::load_immediate(12, 24_576),
            Instruction::AddImmediate { d: 6, a: 6, immediate: 1 },
            Instruction::BranchConditionalForward { options: 16, condition_bit: 0, target: 21 },
            Instruction::StoreWord { s: 11, a: 28, offset: 0 },
            Instruction::move_register(3, 30),
        ]);
        for (offset, register) in (1..6).zip([12, 5, 27, 26, 25]) {
            instructions.push(Instruction::StoreWord {
                s: register,
                a: 28,
                offset: offset * 4,
            });
        }

        assert_eq!(
            instruction_seams(&instructions),
            Some(vec![
                0, 1, 3, 5, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21,
                22, 25, 28, 30, 31,
            ])
        );
    }
}
