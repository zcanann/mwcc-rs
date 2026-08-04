//! Conservative legacy DWARF plan for ordinary mixed translation units.
//!
//! Exact line scheduling and DIE ordering vary between compiler generations,
//! but those refinements should not prevent a fully lowered translation unit
//! from producing a valid debug object. This plan uses final code placement and
//! the backend's physical variable homes as the stable semantic baseline.

use super::functions::{FunctionVariables, VariableLocation};
use mwcc_dwarf1::{DebugEntryId, LineRecord};
use mwcc_machine_code::{DebugVariableLocation, Instruction, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{AsmItem, Expression, Function, FunctionSource, Statement, TranslationUnit, Type};
use std::collections::{HashMap, HashSet};

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
        if let Some(statement_records) = exact_single_statement_leaf_line_records(
            function,
            source,
            machine,
            start,
            layout.sizes[index],
        ) {
            records.extend(statement_records);
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

/// A frame-free, call-free function with one retained statement has stable
/// statement/return seams even when that statement expands to several words.
/// MWCC points the first row at the statement rather than the opening brace;
/// a value return begins on the final non-`blr` instruction, while a void body
/// points its closing-brace row at `blr`.
fn exact_single_statement_leaf_line_records(
    function: &Function,
    source: &FunctionSource,
    machine: &MachineFunction,
    start: u32,
    byte_size: u32,
) -> Option<Vec<LineRecord>> {
    if !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
        || !function.inline_asm_blocks.is_empty()
        || function.statements.len() != 1
        || source.statement_lines.len() != 1
        || !source.control_flow_lines.is_empty()
        || byte_size < 8
        || !matches!(machine.instructions.last(), Some(Instruction::BranchToLinkRegister))
        || machine
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
    {
        return None;
    }
    let mut records = vec![record(source.statement_lines[0], start)];
    let (line, address_delta) = if function.return_expression.is_some() {
        (source.terminal_return_line?, start + byte_size - 8)
    } else if function.return_type == Type::Void {
        (source.body_end_line, start + byte_size - 4)
    } else {
        return None;
    };
    if records
        .last()
        .is_none_or(|row| row.line != line || row.address_delta != address_delta)
    {
        records.push(record(line, address_delta));
    }
    Some(records)
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

/// Legacy optimized debug describes the sole input of a direct file-scope
/// publication as register zero rather than its incoming ABI register. Keep
/// this source-semantic case separate from machine variable homes: the value is
/// consumed directly by the store and has no retained program point afterward.
fn direct_global_store_parameter(
    function: &Function,
    parameter_name: &str,
    global_ids: &HashMap<String, DebugEntryId>,
) -> bool {
    function.return_type == Type::Void
        && function.parameters.len() == 1
        && function.parameters[0].name == parameter_name
        && function.locals.is_empty()
        && function.guards.is_empty()
        && function.return_expression.is_none()
        && function.asm_body.is_none()
        && function.inline_asm_blocks.is_empty()
        && matches!(
            function.statements.as_slice(),
            [Statement::Store {
                target: Expression::Variable(target),
                value: Expression::Variable(value),
            }] if value == parameter_name && global_ids.contains_key(target)
        )
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
    global_ids: &HashMap<String, DebugEntryId>,
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
                let location = (forwarding_wrapper
                    || direct_global_store_parameter(function, &parameter.name, global_ids))
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
            variables.global_references = referenced_global_ids(machine, global_ids);
            variables
        })
        .collect()
}

/// Function DIEs carry vendor references to file-scope objects used by their
/// machine bodies. MWCC walks the finalized symbol transaction backward and
/// emits each referenced data DIE once.
fn referenced_global_ids(
    machine: &MachineFunction,
    global_ids: &HashMap<String, DebugEntryId>,
) -> Vec<DebugEntryId> {
    let mut seen = HashSet::new();
    machine
        .symbol_order
        .iter()
        .rev()
        .filter_map(|name| global_ids.get(name).copied())
        .filter(|id| seen.insert(*id))
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
        DebugVariableLocation::Unavailable => Some(VariableLocation::Unavailable),
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
    use mwcc_syntax_trees::{AsmInstruction, Parameter};

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

    fn single_statement_function(return_expression: Option<Expression>) -> Function {
        Function {
            return_type: if return_expression.is_some() {
                Type::Int
            } else {
                Type::Void
            },
            name: "single".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Expression(Expression::IntegerLiteral(1))],
            guards: Vec::new(),
            return_expression,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    fn leaf_machine(word_count: usize) -> MachineFunction {
        let mut machine = MachineFunction::new("single");
        machine.instructions = (1..word_count)
            .map(|_| Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            })
            .chain([Instruction::BranchToLinkRegister])
            .collect();
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

    #[test]
    fn maps_one_statement_leaf_functions_to_their_executable_seams() {
        let mut statement_source = source(vec![6], Some(7));
        statement_source.body_end_line = 8;
        assert_eq!(
            exact_single_statement_leaf_line_records(
                &single_statement_function(Some(Expression::IntegerLiteral(0))),
                &statement_source,
                &leaf_machine(8),
                8,
                32,
            ),
            Some(vec![record(6, 8), record(7, 32)])
        );

        statement_source.statement_lines = vec![11];
        statement_source.terminal_return_line = None;
        statement_source.body_end_line = 12;
        assert_eq!(
            exact_single_statement_leaf_line_records(
                &single_statement_function(None),
                &statement_source,
                &leaf_machine(2),
                0,
                8,
            ),
            Some(vec![record(11, 0), record(12, 4)])
        );
    }

    #[test]
    fn function_global_references_follow_reverse_symbol_order_once() {
        let mut machine = MachineFunction::new("consumer");
        machine.symbol_order = vec!["first".into(), "external".into(), "second".into()];
        let global_ids = HashMap::from([
            ("first".into(), DebugEntryId(3)),
            ("second".into(), DebugEntryId(7)),
        ]);

        assert_eq!(
            referenced_global_ids(&machine, &global_ids),
            [DebugEntryId(7), DebugEntryId(3)]
        );
    }

    #[test]
    fn recognizes_the_consumed_input_of_a_direct_defined_global_store() {
        let mut function = single_statement_function(None);
        function.parameters = vec![Parameter {
            parameter_type: Type::UnsignedInt,
            name: "seed".into(),
        }];
        function.statements = vec![Statement::Store {
            target: Expression::Variable("next".into()),
            value: Expression::Variable("seed".into()),
        }];
        let globals = HashMap::from([("next".into(), DebugEntryId(4))]);

        assert!(direct_global_store_parameter(&function, "seed", &globals));
        assert!(!direct_global_store_parameter(
            &function,
            "seed",
            &HashMap::new()
        ));
        assert!(!direct_global_store_parameter(&function, "other", &globals));
    }
}
