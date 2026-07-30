//! Conservative legacy DWARF plan for ordinary mixed translation units.
//!
//! Exact line scheduling and DIE ordering vary between compiler generations,
//! but those refinements should not prevent a fully lowered translation unit
//! from producing a valid debug object. This plan uses final code placement and
//! the backend's physical variable homes as the stable semantic baseline.

use super::functions::{FunctionVariables, VariableLocation};
use mwcc_dwarf1::LineRecord;
use mwcc_machine_code::{DebugVariableLocation, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{Function, FunctionSource, TranslationUnit, Type};

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    layout: &FunctionLayout,
) -> Vec<LineRecord> {
    let mut records = Vec::with_capacity(functions.len() * 2);
    for (index, (_, source)) in functions.iter().enumerate() {
        let start = layout.offsets[index];
        let end = start + layout.sizes[index].saturating_sub(4);
        records.push(record(source.body_start_line, start));
        if end != start {
            records.push(record(source.body_end_line, end));
        }
    }
    records
}

pub(super) fn variables(
    unit: &TranslationUnit,
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
) -> Vec<FunctionVariables> {
    functions
        .iter()
        .zip(machine_functions)
        .map(|((function, _), machine)| {
            let mut variables = FunctionVariables::default();
            for (index, parameter) in function.parameters.iter().enumerate() {
                if parameter.name.is_empty()
                    || (matches!(
                        parameter.parameter_type,
                        Type::Struct { .. } | Type::StructPointer { .. }
                    ) && !unit
                        .function_parameter_aggregate_tags
                        .contains_key(&(function.name.clone(), parameter.name.clone())))
                {
                    continue;
                }
                if let Some(location) = machine
                    .debug_variables
                    .iter()
                    .find(|variable| variable.name == parameter.name)
                    .and_then(|variable| convert_location(variable.location))
                {
                    variables.parameters.push((index, location));
                }
            }
            for (index, local) in function.locals.iter().enumerate() {
                if matches!(
                    local.declared_type,
                    Type::Struct { .. } | Type::StructPointer { .. }
                ) && !unit
                    .function_local_aggregate_tags
                    .contains_key(&(function.name.clone(), local.name.clone()))
                {
                    continue;
                }
                if let Some(location) = machine
                    .debug_variables
                    .iter()
                    .find(|variable| variable.name == local.name)
                    .and_then(|variable| convert_location(variable.location))
                {
                    variables.locals.push((index, location));
                }
            }
            variables
        })
        .collect()
}

fn convert_location(location: DebugVariableLocation) -> Option<VariableLocation> {
    match location {
        DebugVariableLocation::GeneralRegister(register) => {
            Some(VariableLocation::Register(register))
        }
        DebugVariableLocation::FrameOffset(offset) => {
            Some(VariableLocation::Frame(i32::from(offset)))
        }
        // Legacy FPR location expressions need generation-specific register
        // numbering. Omit those variables until that policy is measured.
        DebugVariableLocation::FloatRegister(_) => None,
    }
}

fn record(line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line,
        column: u16::MAX,
        address_delta,
    }
}
