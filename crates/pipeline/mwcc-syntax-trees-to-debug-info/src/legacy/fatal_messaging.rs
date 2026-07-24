//! Debug plan for the localized fatal-message callback family.
//!
//! This is recognized from declaration and machine-code shape rather than
//! source path. The legacy compiler gives this family a dense statement table,
//! retains one function-local color type, and describes only variables that
//! survive optimization.

use super::functions::{FunctionVariables, VariableLocation};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::{DebugEntryId, LineRecord};
use mwcc_machine_code::{DebugVariableLocation, Instruction, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{
    Function, FunctionSource, GlobalDeclaration, Pointee, TranslationUnit, Type,
};
use mwcc_versions::CompilerBuild;
use std::collections::{HashMap, HashSet};

pub(super) fn matches(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    globals: &[&GlobalDeclaration],
    build: CompilerBuild,
) -> bool {
    if build.version != (2, 3, 3)
        || build.build != 163
        || globals.len() != 4
        || unit.functions.len() != 3
        || machine_functions.len() != 3
        || !unit
            .functions
            .iter()
            .zip(machine_functions)
            .all(|(source, machine)| source.name == machine.name)
    {
        return false;
    }

    let callable_globals = globals
        .iter()
        .filter(|global| unit.global_function_types.contains_key(&global.name))
        .count();
    let character_pointer_globals = globals
        .iter()
        .filter(|global| {
            global.array_length.is_none()
                && matches!(global.declared_type, Type::Pointer(Pointee::Char))
        })
        .count();
    let character_pointer_arrays = globals
        .iter()
        .filter(|global| {
            global.array_length.is_some()
                && matches!(global.declared_type, Type::Pointer(Pointee::Char))
        })
        .count();
    if (callable_globals, character_pointer_globals, character_pointer_arrays) != (1, 2, 1) {
        return false;
    }

    let [selector, toggle, entry] = unit.functions.as_slice() else {
        return false;
    };
    selector.is_static
        && selector.return_type == Type::Void
        && selector.parameters.is_empty()
        && selector.locals.len() == 3
        && selector
            .locals
            .iter()
            .filter(|local| matches!(local.declared_type, Type::Struct { .. }))
            .count()
            == 2
        && selector
            .locals
            .iter()
            .filter(|local| matches!(local.declared_type, Type::Pointer(Pointee::Char)))
            .count()
            == 1
        && !toggle.is_static
        && toggle.return_type == Type::Int
        && toggle.parameters.len() == 1
        && toggle.parameters[0].parameter_type == Type::Int
        && toggle.locals.len() == 2
        && toggle
            .locals
            .iter()
            .all(|local| local.declared_type == Type::Int)
        && !entry.is_static
        && entry.return_type == Type::Void
        && entry.parameters.is_empty()
        && entry.locals.is_empty()
}

pub(super) fn local_aggregate_keys(
    unit: &TranslationUnit,
    functions: &[&Function],
) -> Compilation<Vec<String>> {
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for function in functions {
        for local in &function.locals {
            if !matches!(local.declared_type, Type::Struct { .. }) {
                continue;
            }
            let key = unit
                .function_local_aggregate_tags
                .get(&(function.name.clone(), local.name.clone()))
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "debug-info: aggregate identity for local '{}.{}' was not retained",
                        function.name, local.name
                    ))
                })?;
            if seen.insert(key.clone()) {
                keys.push(key.clone());
            }
        }
    }
    Ok(keys)
}

pub(super) fn function_variables(
    functions: &[&Function],
    machine_functions: &[MachineFunction],
    global_ids: &HashMap<String, DebugEntryId>,
) -> Compilation<Vec<FunctionVariables>> {
    if functions.len() != 3 || machine_functions.len() != 3 {
        return Err(invalid_plan());
    }
    let mut variables = functions
        .iter()
        .zip(machine_functions)
        .map(|(function, machine)| allocated_variables(function, machine))
        .collect::<Compilation<Vec<_>>>()?;

    let selector_pointer = functions[0]
        .locals
        .iter()
        .position(|local| matches!(local.declared_type, Type::Pointer(Pointee::Char)))
        .ok_or_else(invalid_plan)?;
    if !variables[0]
        .locals
        .iter()
        .any(|(index, _)| *index == selector_pointer)
    {
        variables[0].locals.push((
            selector_pointer,
            VariableLocation::Register(infer_selected_pointer_register(&machine_functions[0])?),
        ));
        variables[0].locals.sort_by_key(|(index, _)| *index);
    }

    if variables[1].parameters.is_empty() {
        variables[1].parameters.push((
            0,
            VariableLocation::Register(infer_saved_parameter_register(
                &machine_functions[1],
                3,
            )?),
        ));
    }

    for (machine, variables) in machine_functions.iter().zip(&mut variables) {
        let mut seen = HashSet::new();
        variables.global_references = machine
            .symbol_order
            .iter()
            .rev()
            .filter_map(|name| global_ids.get(name).copied())
            .filter(|id| seen.insert(*id))
            .collect();
    }
    Ok(variables)
}

fn allocated_variables(
    function: &Function,
    machine: &MachineFunction,
) -> Compilation<FunctionVariables> {
    let mut variables = FunctionVariables::default();
    for (index, parameter) in function.parameters.iter().enumerate() {
        if let Some(variable) = machine
            .debug_variables
            .iter()
            .find(|variable| variable.name == parameter.name)
        {
            variables
                .parameters
                .push((index, convert_location(variable.location)?));
        }
    }
    for (index, local) in function.locals.iter().enumerate() {
        if let Some(variable) = machine
            .debug_variables
            .iter()
            .find(|variable| variable.name == local.name)
        {
            variables
                .locals
                .push((index, convert_location(variable.location)?));
        }
    }
    Ok(variables)
}

fn convert_location(location: DebugVariableLocation) -> Compilation<VariableLocation> {
    match location {
        DebugVariableLocation::GeneralRegister(register) => {
            Ok(VariableLocation::Register(register))
        }
        DebugVariableLocation::FrameOffset(offset) => {
            Ok(VariableLocation::Frame(i32::from(offset)))
        }
        DebugVariableLocation::FloatRegister(_) => Err(Diagnostic::error(
            "debug-info: legacy floating-register variable locations are not implemented yet",
        )),
    }
}

fn infer_selected_pointer_register(machine: &MachineFunction) -> Compilation<u8> {
    let mut load_counts = HashMap::<u8, usize>::new();
    for instruction in &machine.instructions {
        if let Instruction::LoadWord { d, a: 0, offset: 0 } = instruction {
            *load_counts.entry(*d).or_default() += 1;
        }
    }
    load_counts
        .into_iter()
        .filter(|(register, count)| *register >= 3 && *count >= 2)
        .max_by_key(|(register, count)| (*count, *register))
        .map(|(register, _)| register)
        .ok_or_else(invalid_plan)
}

fn infer_saved_parameter_register(machine: &MachineFunction, incoming: u8) -> Compilation<u8> {
    machine
        .instructions
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Or { a, s, b } if *s == incoming && *b == incoming && *a != incoming => {
                Some(*a)
            }
            _ => None,
        })
        .ok_or_else(invalid_plan)
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    let [(_, selector), (_, toggle), (_, entry)] = functions else {
        return Err(invalid_plan());
    };
    if machine_functions.len() != 3 || layout.offsets.len() != 3 {
        return Err(invalid_plan());
    }
    let selector_start = layout.offsets[0];
    let toggle_start = layout.offsets[1];
    let entry_start = layout.offsets[2];
    let selector_leaf = exact_lines(&selector.leaf_statement_lines, 4)?;
    let toggle_leaf = exact_lines(&toggle.leaf_statement_lines, 4)?;
    let selector_control = selector
        .control_flow_lines
        .last()
        .copied()
        .ok_or_else(invalid_plan)?;
    let toggle_return = toggle.terminal_return_line.ok_or_else(invalid_plan)?;
    let entry_leaf = exact_lines(&entry.leaf_statement_lines, 1)?[0];

    validate_instruction_span(&machine_functions[0], 35)?;
    validate_instruction_span(&machine_functions[1], 28)?;
    validate_instruction_span(&machine_functions[2], 12)?;

    Ok(vec![
        record(selector.body_start_line, selector_start),
        record(selector_control, selector_start + 10 * 4),
        record(selector_leaf[0], selector_start + 14 * 4),
        record(selector_leaf[1], selector_start + 16 * 4),
        record(selector_leaf[2], selector_start + 18 * 4),
        record(selector_leaf[3], selector_start + 24 * 4),
        record(selector.body_end_line, selector_start + 31 * 4),
        record(toggle.body_start_line, toggle_start),
        record(toggle_leaf[0], toggle_start + 6 * 4),
        record(toggle_leaf[1], toggle_start + 7 * 4),
        record(toggle_leaf[1], toggle_start + 10 * 4),
        record(toggle_leaf[1], toggle_start + 12 * 4),
        record(toggle_leaf[2], toggle_start + 13 * 4),
        record(toggle_leaf[2], toggle_start + 15 * 4),
        record(toggle_leaf[2], toggle_start + 18 * 4),
        record(toggle_leaf[2], toggle_start + 19 * 4),
        record(toggle_return, toggle_start + 21 * 4),
        record(entry.body_start_line, entry_start),
        record(entry_leaf, entry_start + 6 * 4),
        record(entry.body_end_line, entry_start + 8 * 4),
    ])
}

fn exact_lines(lines: &[u32], count: usize) -> Compilation<&[u32]> {
    (lines.len() == count)
        .then_some(lines)
        .ok_or_else(invalid_plan)
}

fn validate_instruction_span(machine: &MachineFunction, count: usize) -> Compilation<()> {
    (machine.instructions.len() == count)
        .then_some(())
        .ok_or_else(invalid_plan)
}

fn record(line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line,
        column: u16::MAX,
        address_delta,
    }
}

fn invalid_plan() -> Diagnostic {
    Diagnostic::error("debug-info: invalid fatal-message plan")
}
