//! Debug plans for ordinary void functions with one directly scheduled action.
//!
//! Legacy DWARF describes optimized parameters, not every source parameter: an
//! unused argument disappears, while a pointer kept across a call is located in
//! its allocated callee-saved register. This module recognizes the small
//! measured family and turns syntax plus final instruction order into that plan.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::LineRecord;
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{Expression, Function, FunctionSource, Statement, TranslationUnit, Type};

pub(super) fn matches(unit: &TranslationUnit, machine_functions: &[MachineFunction]) -> bool {
    unit.functions.len() == machine_functions.len()
        && !unit.functions.is_empty()
        && unit.functions.iter().all(function_shape)
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    let mut records = Vec::new();
    for (index, ((function, source), machine)) in
        functions.iter().zip(machine_functions).enumerate()
    {
        let start = layout.offsets[index];
        match function.statements.as_slice() {
            [] => records.push(record(source.body_end_line, start)),
            [Statement::Expression(Expression::Call { .. })] => {
                let call = call_index(machine)?;
                let argument_start = call.checked_sub(1).ok_or_else(invalid_plan)?;
                records.push(record(source.body_start_line, start));
                records.push(record(
                    only_statement_line(source)?,
                    start + argument_start * 4,
                ));
                records.push(record(source.body_end_line, start + (call + 1) * 4));
            }
            [Statement::Store { .. }] => {
                let call = call_index(machine)?;
                records.push(record(source.body_start_line, start));
                records.push(record(only_statement_line(source)?, start + call * 4));
                records.push(record(source.body_end_line, start + (call + 2) * 4));
            }
            _ => return Err(invalid_plan()),
        }
    }
    Ok(records)
}

pub(super) fn parameter_registers(
    functions: &[(&Function, FunctionSource)],
) -> Compilation<Vec<Vec<(usize, u8)>>> {
    functions
        .iter()
        .map(|(function, _)| match function.statements.as_slice() {
            [] => Ok(Vec::new()),
            [Statement::Expression(Expression::Call { .. })] => Ok(vec![(0, 3)]),
            [Statement::Store { .. }] => Ok(vec![(0, 31)]),
            _ => Err(invalid_plan()),
        })
        .collect()
}

fn function_shape(function: &Function) -> bool {
    if function.return_type != Type::Void
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || function.parameters.len() > 1
    {
        return false;
    }
    match function.statements.as_slice() {
        [] => true,
        [Statement::Expression(Expression::Call { arguments, .. })] => {
            one_dereferenced_parameter(function, arguments)
        }
        [Statement::Store { target, value }] => {
            function.parameters.len() == 1
                && dereferences_parameter(target, &function.parameters[0].name)
                && matches!(value, Expression::Call { arguments, .. } if arguments.is_empty())
        }
        _ => false,
    }
}

fn one_dereferenced_parameter(function: &Function, arguments: &[Expression]) -> bool {
    function.parameters.len() == 1
        && arguments.len() == 1
        && dereferences_parameter(&arguments[0], &function.parameters[0].name)
}

fn dereferences_parameter(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Dereference { pointer }
            if matches!(pointer.as_ref(), Expression::Variable(variable) if variable == name)
    )
}

fn call_index(machine: &MachineFunction) -> Compilation<u32> {
    let mut calls = machine
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, instruction)| matches!(instruction, Instruction::BranchAndLink { .. }));
    let index = calls.next().ok_or_else(invalid_plan)?.0;
    if calls.next().is_some() {
        return Err(invalid_plan());
    }
    u32::try_from(index).map_err(|_| invalid_plan())
}

fn only_statement_line(source: &FunctionSource) -> Compilation<u32> {
    match source.statement_lines.as_slice() {
        [line] => Ok(*line),
        _ => Err(Diagnostic::error(
            "debug-info: a simple void function needs one retained statement source line",
        )),
    }
}

fn record(line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line,
        column: u16::MAX,
        address_delta,
    }
}

fn invalid_plan() -> Diagnostic {
    Diagnostic::error("debug-info: invalid simple void-function plan")
}
