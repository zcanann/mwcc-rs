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
use mwcc_syntax_trees::{AsmItem, Function, FunctionSource, TranslationUnit, Type};

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    layout: &FunctionLayout,
) -> Vec<LineRecord> {
    let mut records = Vec::with_capacity(functions.len() * 2);
    for (index, (function, source)) in functions.iter().enumerate() {
        let start = layout.offsets[index];
        if let Some(asm_records) = function
            .asm_body
            .as_deref()
            .and_then(|items| exact_asm_line_records(items, start, layout.sizes[index]))
        {
            records.extend(asm_records);
            continue;
        }
        let end = start + layout.sizes[index].saturating_sub(4);
        records.push(record(source.body_start_line, start));
        if end != start {
            records.push(record(source.body_end_line, end));
        }
    }
    records
}

/// Naked assembly has an authoritative one-source-line/one-word mapping. Only
/// use it when it covers the finalized function exactly: a synthesized return
/// or a later peephole can otherwise leave an instruction without provenance,
/// in which case the conservative function-boundary schedule remains safer.
fn exact_asm_line_records(
    items: &[AsmItem],
    start: u32,
    byte_size: u32,
) -> Option<Vec<LineRecord>> {
    let mut address = start;
    let mut records = Vec::new();
    for item in items {
        let AsmItem::Instruction(instruction) = item else {
            continue;
        };
        if matches!(instruction.mnemonic.as_str(), "nofralloc" | "frfree") {
            continue;
        }
        records.push(record(instruction.source_line, address));
        address += 4;
    }
    (address == start + byte_size).then_some(records)
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::AsmInstruction;

    fn instruction(mnemonic: &str, source_line: u32) -> AsmItem {
        AsmItem::Instruction(AsmInstruction {
            mnemonic: mnemonic.into(),
            operands: Vec::new(),
            source_line,
        })
    }

    #[test]
    fn maps_each_naked_assembly_word_to_its_physical_source_line() {
        let items = vec![
            instruction("nofralloc", 10),
            instruction("mr", 11),
            AsmItem::Label("done".into()),
            instruction("blr", 13),
        ];

        assert_eq!(
            exact_asm_line_records(&items, 0x24, 8),
            Some(vec![record(11, 0x24), record(13, 0x28)])
        );
    }

    #[test]
    fn rejects_a_partial_map_when_codegen_added_an_unmapped_word() {
        let items = vec![instruction("mr", 11), instruction("blr", 13)];
        assert_eq!(exact_asm_line_records(&items, 0x24, 12), None);
    }
}
