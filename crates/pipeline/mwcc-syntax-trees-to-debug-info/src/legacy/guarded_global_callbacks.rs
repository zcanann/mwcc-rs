//! Legacy line/DIE scheduling for a null-guarded file-scope callback.
//!
//! The executable transaction keeps the callback in r12 across its null test.
//! DWARF describes the nested call at the `mtlr` which begins that source
//! action, while the closing/terminal-return line begins at the shared
//! epilogue.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_dwarf1::LineRecord;
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_object::FunctionLayout;
use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, FunctionSource, GlobalDeclaration,
    SourceFundamentalType, Statement, TranslationUnit, Type,
};

pub(super) fn matches(
    unit: &TranslationUnit,
    machine_functions: &[MachineFunction],
    globals: &[&GlobalDeclaration],
) -> bool {
    machine_functions.len() == 1 && source_shape(unit, globals).is_some()
}

pub(super) fn line_records(
    functions: &[(&Function, FunctionSource)],
    machine_functions: &[MachineFunction],
    layout: &FunctionLayout,
) -> Compilation<Vec<LineRecord>> {
    let [(_, source)] = functions else {
        return Err(invalid_plan());
    };
    let [machine] = machine_functions else {
        return Err(invalid_plan());
    };
    let mut calls = machine
        .instructions
        .iter()
        .enumerate()
        .filter(|(_, instruction)| {
            matches!(instruction, Instruction::BranchToLinkRegisterAndLink)
        });
    let call_index = calls.next().ok_or_else(invalid_plan)?.0;
    if calls.next().is_some() || call_index == 0 {
        return Err(invalid_plan());
    }
    let [action_line] = source.leaf_statement_lines.as_slice() else {
        return Err(Diagnostic::error(
            "debug-info: guarded callback needs one nested action source line",
        ));
    };
    let end_line = source.terminal_return_line.unwrap_or(source.body_end_line);
    let start = layout.offsets[0];
    Ok(vec![
        record(source.body_start_line, start),
        record(*action_line, start + (call_index as u32 - 1) * 4),
        record(end_line, start + (call_index as u32 + 1) * 4),
    ])
}

fn source_shape<'a>(
    unit: &'a TranslationUnit,
    globals: &[&'a GlobalDeclaration],
) -> Option<(&'a GlobalDeclaration, &'a Function)> {
    let [global] = globals else {
        return None;
    };
    let [function] = unit.functions.as_slice() else {
        return None;
    };
    let signature = unit.global_function_types.get(&global.name)?;
    if !global.is_static
        || global.is_extern
        || global.array_length.is_some()
        || signature.variadic
        || !signature.parameters.is_empty()
        || signature.return_type.declared_type != Type::Void
        || signature.return_type.source_fundamental != Some(SourceFundamentalType::Void)
        || signature.return_type.pointer_depth != 0
        || signature.return_type.is_reference
        || signature.return_type.function_type.is_some()
        || function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Variable(condition_global) = left.as_ref() else {
        return None;
    };
    let [Statement::Expression(call)] = then_body.as_slice() else {
        return None;
    };
    let call_matches = match call {
        Expression::Call { name, arguments } => {
            name == condition_global && arguments.is_empty()
        }
        Expression::CallThrough { target, arguments } => {
            arguments.is_empty()
                && matches!(
                    target.as_ref(),
                    Expression::Variable(name) if name == condition_global
                )
        }
        _ => false,
    };
    (condition_global == &global.name
        && matches!(right.as_ref(), Expression::IntegerLiteral(0))
        && else_body.is_empty()
        && call_matches)
        .then_some((*global, function))
}

fn record(line: u32, address_delta: u32) -> LineRecord {
    LineRecord {
        line,
        column: u16::MAX,
        address_delta,
    }
}

fn invalid_plan() -> Diagnostic {
    Diagnostic::error("debug-info: invalid guarded global-callback plan")
}
