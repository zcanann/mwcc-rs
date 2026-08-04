//! Conservative legacy DWARF plan for ordinary mixed translation units.
//!
//! Exact line scheduling and DIE ordering vary between compiler generations,
//! but those refinements should not prevent a fully lowered translation unit
//! from producing a valid debug object. This plan uses final code placement and
//! the backend's physical variable homes as the stable semantic baseline.

use super::functions::{FunctionVariables, VariableLocation};
use mwcc_dwarf1::LineRecord;
use mwcc_machine_code::{DebugVariableLocation, Instruction, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{AsmItem, Expression, Function, FunctionSource, Statement, TranslationUnit, Type};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirectCallWrapper {
    VoidStatement,
    ReturnedCall,
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Vec<LineRecord> {
    let mut records = Vec::with_capacity(functions.len() * 2);
    for (index, ((function, source), machine)) in
        functions.iter().zip(machine_functions).enumerate()
    {
        let start = layout.offsets[index];
        if let Some(asm_records) = function
            .asm_body
            .as_deref()
            .and_then(|items| exact_asm_line_records(items, start, layout.sizes[index]))
        {
            records.extend(asm_records);
            continue;
        }
        if let Some(wrapper_records) = direct_call_wrapper(function).and_then(|wrapper| {
            exact_direct_call_wrapper_line_records(wrapper, source, machine, start)
        }) {
            records.extend(wrapper_records);
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

/// A pure forwarding wrapper has one stable source-to-machine seam: the direct
/// call itself. Measured optimized C++ wrappers place the statement/return row
/// on that `bl`; void wrappers additionally place their closing-brace row on
/// the first epilogue instruction.
fn exact_direct_call_wrapper_line_records(
    wrapper: DirectCallWrapper,
    source: &FunctionSource,
    machine: &MachineFunction,
    start: u32,
) -> Option<Vec<LineRecord>> {
    let call = direct_call_index(machine)?;
    let action_line = match wrapper {
        DirectCallWrapper::VoidStatement => match source.statement_lines.as_slice() {
            [line] => *line,
            _ => return None,
        },
        DirectCallWrapper::ReturnedCall => source.terminal_return_line?,
    };
    let mut records = vec![
        record(source.body_start_line, start),
        record(action_line, start + call * 4),
    ];
    if wrapper == DirectCallWrapper::VoidStatement {
        let epilogue = call.checked_add(1)?;
        if usize::try_from(epilogue).ok()? >= machine.instructions.len() {
            return None;
        }
        records.push(record(source.body_end_line, start + epilogue * 4));
    }
    Some(records)
}

fn direct_call_index(machine: &MachineFunction) -> Option<u32> {
    let mut calls = machine
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, instruction)| matches!(instruction, Instruction::BranchAndLink { .. }));
    let call = u32::try_from(calls.next()?.0).ok()?;
    calls.next().is_none().then_some(call)
}

fn direct_call_wrapper(function: &Function) -> Option<DirectCallWrapper> {
    if !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !function.inline_asm_blocks.is_empty()
    {
        return None;
    }
    match (function.statements.as_slice(), &function.return_expression) {
        ([Statement::Expression(Expression::Call { .. })], None)
            if function.return_type == Type::Void =>
        {
            Some(DirectCallWrapper::VoidStatement)
        }
        ([], Some(Expression::Call { .. })) if function.return_type != Type::Void => {
            Some(DirectCallWrapper::ReturnedCall)
        }
        _ => None,
    }
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
            let forwarding_wrapper = direct_call_wrapper(function).is_some()
                && direct_call_index(machine).is_some();
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
                let location = forwarding_wrapper
                    .then_some(VariableLocation::Register(0))
                    .or_else(|| {
                        machine
                            .debug_variables
                            .iter()
                            .find(|variable| variable.name == parameter.name)
                            .and_then(|variable| convert_location(variable.location))
                    });
                if let Some(location) = location {
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

    fn source(statement_lines: Vec<u32>, terminal_return_line: Option<u32>) -> FunctionSource {
        FunctionSource {
            body_start_line: 8,
            local_lines: Vec::new(),
            statement_lines,
            leaf_statement_lines: Vec::new(),
            control_flow_lines: Vec::new(),
            terminal_return_line,
            body_end_line: 10,
        }
    }

    fn direct_call_machine() -> MachineFunction {
        let mut machine = MachineFunction::new("wrapper");
        machine.instructions = vec![
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: -16,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "callee".into(),
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
        ];
        machine
    }

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

    #[test]
    fn maps_a_void_forwarding_wrapper_call_and_epilogue() {
        assert_eq!(
            exact_direct_call_wrapper_line_records(
                DirectCallWrapper::VoidStatement,
                &source(vec![9], None),
                &direct_call_machine(),
                0x20,
            ),
            Some(vec![record(8, 0x20), record(9, 0x2c), record(10, 0x30)])
        );
    }

    #[test]
    fn maps_a_returned_forwarding_call_without_a_closing_brace_row() {
        assert_eq!(
            exact_direct_call_wrapper_line_records(
                DirectCallWrapper::ReturnedCall,
                &source(Vec::new(), Some(25)),
                &direct_call_machine(),
                0x40,
            ),
            Some(vec![record(8, 0x40), record(25, 0x4c)])
        );
    }
}
