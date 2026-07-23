//! Rectangle updates for writer control characters.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{ArmBody, BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::Behavior;

struct RectControlCalls<'a> {
    cursor_x: &'a str,
    cursor_y: &'a str,
    font_height: &'a str,
    linefeed: &'a str,
    tab: &'a str,
    normalize: &'a str,
}

/// Lower the Wii rectangle schedule for newline/tab control characters. The
/// operation is recognized from both switch arms so it applies across template
/// instantiations without pinning project symbol names.
pub(crate) fn lower_member_rect_control(
    function: &Function,
    behavior: &Behavior,
    cpp_exceptions: bool,
    indexed_restore: bool,
) -> Option<MachineFunction> {
    if !indexed_restore
        || function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || !function.inline_asm_blocks.is_empty()
    {
        return None;
    }
    let [this, rect, code, context] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(this.parameter_type, Type::StructPointer { .. })
        || rect.parameter_type != (Type::StructPointer { element_size: 16 })
        || code.parameter_type != Type::UnsignedShort
        || !matches!(context.parameter_type, Type::StructPointer { .. })
    {
        return None;
    }
    let [newline_writer, tab_writer] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(newline_writer.declared_type, Type::StructPointer { .. })
        || !matches!(tab_writer.declared_type, Type::StructPointer { .. })
        || function.locals.iter().any(|local| {
            local.is_static || local.array_length.is_some() || local.initializer.is_some()
        })
    {
        return None;
    }
    let [noop_rect, noop_code, noop_context, dispatch] = function.statements.as_slice() else {
        return None;
    };
    if !is_void_zero(noop_rect) || !is_void_zero(noop_code) || !is_void_zero(noop_context) {
        return None;
    }
    let Statement::Switch {
        scrutinee,
        arms,
        default,
    } = dispatch
    else {
        return None;
    };
    if !is_named_variable(scrutinee, &code.name)
        || !matches!(
            default,
            Some(ArmBody::Return(Expression::IntegerLiteral(0)))
        )
    {
        return None;
    }
    let [newline_arm, tab_arm] = arms.as_slice() else {
        return None;
    };
    if newline_arm.value != 10
        || tab_arm.value != 9
        || newline_arm.falls_through
        || tab_arm.falls_through
    {
        return None;
    }
    let ArmBody::Statements(newline) = &newline_arm.body else {
        return None;
    };
    let ArmBody::Statements(tab) = &tab_arm.body else {
        return None;
    };
    let calls = recognize_arms(
        newline,
        tab,
        &this.name,
        &rect.name,
        &context.name,
        &newline_writer.name,
        &tab_writer.name,
    )?;
    Some(emit_member_rect_control(
        function,
        behavior,
        cpp_exceptions,
        calls,
    ))
}

#[allow(clippy::too_many_arguments)]
fn recognize_arms<'a>(
    newline: &'a [Statement],
    tab: &'a [Statement],
    this: &str,
    rect: &str,
    context: &str,
    newline_writer: &str,
    tab_writer: &str,
) -> Option<RectControlCalls<'a>> {
    let [bind_newline, newline_right, newline_top, linefeed, newline_left, newline_bottom, normalize_newline, return_newline] =
        newline
    else {
        return None;
    };
    if !is_writer_binding(bind_newline, newline_writer, context)
        || !matches!(
            return_newline,
            Statement::Return(Some(Expression::IntegerLiteral(3)))
        )
    {
        return None;
    }
    let cursor_x = rect_store_direct_call(newline_right, rect, 8, newline_writer)?;
    let cursor_y = rect_store_direct_call(newline_top, rect, 4, newline_writer)?;
    let linefeed = two_argument_call(linefeed, this, context)?;
    if rect_store_direct_call(newline_left, rect, 0, newline_writer)? != cursor_x {
        return None;
    }
    let (bottom_cursor_y, font_height) =
        rect_store_sum_calls(newline_bottom, rect, 12, newline_writer, context)?;
    if bottom_cursor_y != cursor_y {
        return None;
    }
    let normalize = one_argument_statement_call(normalize_newline, rect)?;

    let [bind_tab, tab_left, process_tab, tab_right, tab_top, tab_bottom, normalize_tab, return_tab] =
        tab
    else {
        return None;
    };
    if !is_writer_binding(bind_tab, tab_writer, context)
        || !matches!(
            return_tab,
            Statement::Return(Some(Expression::IntegerLiteral(1)))
        )
        || rect_store_direct_call(tab_left, rect, 0, tab_writer)? != cursor_x
        || rect_store_direct_call(tab_right, rect, 8, tab_writer)? != cursor_x
        || rect_store_direct_call(tab_top, rect, 4, tab_writer)? != cursor_y
        || one_argument_statement_call(normalize_tab, rect)? != normalize
    {
        return None;
    }
    let tab = two_argument_call(process_tab, this, context)?;
    if rect_store_member_call_sum(tab_bottom, rect, 12, 4, tab_writer)? != font_height {
        return None;
    }
    Some(RectControlCalls {
        cursor_x,
        cursor_y,
        font_height,
        linefeed,
        tab,
        normalize,
    })
}

fn emit_member_rect_control(
    function: &Function,
    behavior: &Behavior,
    cpp_exceptions: bool,
    calls: RectControlCalls<'_>,
) -> MachineFunction {
    let mut output = MachineFunction::new(function.name.clone());
    output.pre_scheduled = true;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    output.anonymous_label_bump = 5;
    output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -64,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 68,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 48,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 31,
            a: 1,
            offset: 56,
            w: 0,
            i: 0,
        },
        Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 48,
        },
    ]);
    emit_call(&mut output, "_savegpr_26");
    output.instructions.extend([
        Instruction::move_register(31, 1),
        Instruction::move_register(26, 3),
        Instruction::move_register(30, 4),
        Instruction::StoreHalfword {
            s: 5,
            a: 31,
            offset: 8,
        },
        Instruction::move_register(29, 6),
        Instruction::LoadHalfwordZero {
            d: 0,
            a: 31,
            offset: 8,
        },
        Instruction::CompareWordImmediate {
            a: 0,
            immediate: 10,
        },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 19,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 65,
        },
        Instruction::CompareWordImmediate { a: 0, immediate: 9 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 43,
        },
        Instruction::Branch { target: 65 },
        Instruction::LoadWord {
            d: 28,
            a: 29,
            offset: 0,
        },
        Instruction::move_register(3, 28),
    ]);
    emit_call(&mut output, calls.cursor_x);
    output.instructions.extend([
        Instruction::StoreFloatSingle {
            s: 1,
            a: 30,
            offset: 8,
        },
        Instruction::move_register(3, 28),
    ]);
    emit_call(&mut output, calls.cursor_y);
    output.instructions.extend([
        Instruction::StoreFloatSingle {
            s: 1,
            a: 30,
            offset: 4,
        },
        Instruction::move_register(3, 26),
        Instruction::move_register(4, 29),
    ]);
    emit_call(&mut output, calls.linefeed);
    output.instructions.push(Instruction::move_register(3, 28));
    emit_call(&mut output, calls.cursor_x);
    output.instructions.extend([
        Instruction::StoreFloatSingle {
            s: 1,
            a: 30,
            offset: 0,
        },
        Instruction::LoadWord {
            d: 3,
            a: 29,
            offset: 0,
        },
    ]);
    emit_call(&mut output, calls.font_height);
    output.instructions.extend([
        Instruction::FloatMove { d: 31, b: 1 },
        Instruction::move_register(3, 28),
    ]);
    emit_call(&mut output, calls.cursor_y);
    output.instructions.extend([
        Instruction::FloatAddSingle { d: 0, a: 1, b: 31 },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 30,
            offset: 12,
        },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, calls.normalize);
    output.instructions.extend([
        Instruction::load_immediate(3, 3),
        Instruction::Branch { target: 66 },
        Instruction::LoadWord {
            d: 27,
            a: 29,
            offset: 0,
        },
        Instruction::move_register(3, 27),
    ]);
    emit_call(&mut output, calls.cursor_x);
    output.instructions.extend([
        Instruction::StoreFloatSingle {
            s: 1,
            a: 30,
            offset: 0,
        },
        Instruction::move_register(3, 26),
        Instruction::move_register(4, 29),
    ]);
    emit_call(&mut output, calls.tab);
    output.instructions.push(Instruction::move_register(3, 27));
    emit_call(&mut output, calls.cursor_x);
    output.instructions.extend([
        Instruction::StoreFloatSingle {
            s: 1,
            a: 30,
            offset: 8,
        },
        Instruction::move_register(3, 27),
    ]);
    emit_call(&mut output, calls.cursor_y);
    output.instructions.extend([
        Instruction::StoreFloatSingle {
            s: 1,
            a: 30,
            offset: 4,
        },
        Instruction::move_register(3, 27),
    ]);
    emit_call(&mut output, calls.font_height);
    output.instructions.extend([
        Instruction::LoadFloatSingle {
            d: 0,
            a: 30,
            offset: 4,
        },
        Instruction::FloatAddSingle { d: 0, a: 0, b: 1 },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 30,
            offset: 12,
        },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, calls.normalize);
    output.instructions.extend([
        Instruction::load_immediate(3, 1),
        Instruction::Branch { target: 66 },
        Instruction::load_immediate(3, 0),
        Instruction::move_register(10, 31),
        Instruction::load_immediate(0, 56),
        Instruction::PairedSingleQuantizedLoadIndexed {
            d: 31,
            a: 10,
            b: 0,
            w: 0,
            i: 0,
        },
        Instruction::LoadFloatDouble {
            d: 31,
            a: 10,
            offset: 48,
        },
        Instruction::AddImmediate {
            d: 11,
            a: 10,
            immediate: 48,
        },
    ]);
    emit_call(&mut output, "_restgpr_26");
    output.instructions.extend([
        Instruction::LoadWord {
            d: 10,
            a: 1,
            offset: 0,
        },
        Instruction::LoadWord {
            d: 0,
            a: 10,
            offset: 4,
        },
        Instruction::move_register(1, 10),
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]);
    debug_assert_eq!(output.instructions.len(), 77);

    output.symbol_order = [
        calls.cursor_x,
        calls.cursor_y,
        calls.linefeed,
        calls.font_height,
        calls.normalize,
        calls.tab,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    output.referenced_function_symbols = ["_savegpr_26", "_restgpr_26"]
        .into_iter()
        .chain(output.symbol_order.iter().map(String::as_str))
        .map(str::to_owned)
        .collect();
    if cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 6,
            saved_fpr_count: 1,
            uses_fpu: behavior.mark_single_precision_extab,
        });
    }
    output
}

fn emit_call(output: &mut MachineFunction, target: &str) {
    output.relocations.push(Relocation {
        instruction_index: output.instructions.len(),
        kind: RelocationKind::Rel24,
        target: RelocationTarget::External(target.to_owned()),
    });
    output.instructions.push(Instruction::BranchAndLink {
        target: target.to_owned(),
    });
}

fn is_void_zero(statement: &Statement) -> bool {
    matches!(statement,
        Statement::Expression(Expression::Cast { target_type: Type::Void, operand })
            if matches!(operand.as_ref(), Expression::IntegerLiteral(0)))
}

fn is_writer_binding(statement: &Statement, writer: &str, context: &str) -> bool {
    matches!(statement,
        Statement::Assign {
            name,
            value: Expression::Dereference { pointer },
        } if name == writer && matches!(pointer.as_ref(),
            Expression::Member {
                base,
                offset: 0,
                member_type: Type::StructPointer { .. },
                index_stride: None,
            } if is_named_variable(base, context)))
}

fn rect_store_direct_call<'a>(
    statement: &'a Statement,
    rect: &str,
    offset: u32,
    argument: &str,
) -> Option<&'a str> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    is_member(target, rect, offset, Type::Float)
        .then(|| direct_single_argument_call(value, argument))?
}

fn rect_store_sum_calls<'a>(
    statement: &'a Statement,
    rect: &str,
    offset: u32,
    writer: &str,
    context: &str,
) -> Option<(&'a str, &'a str)> {
    let Statement::Store {
        target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            },
    } = statement
    else {
        return None;
    };
    if !is_member(target, rect, offset, Type::Float) {
        return None;
    }
    let left = direct_single_argument_call(left, writer)?;
    let Expression::Call { name, arguments } = right.as_ref() else {
        return None;
    };
    matches!(arguments.as_slice(), [value] if is_struct_pointer_member(value, context, 0))
        .then_some((left, name.as_str()))
}

fn rect_store_member_call_sum<'a>(
    statement: &'a Statement,
    rect: &str,
    target_offset: u32,
    source_offset: u32,
    writer: &str,
) -> Option<&'a str> {
    let Statement::Store {
        target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            },
    } = statement
    else {
        return None;
    };
    (is_member(target, rect, target_offset, Type::Float)
        && is_member(left, rect, source_offset, Type::Float))
    .then(|| direct_single_argument_call(right, writer))?
}

fn two_argument_call<'a>(statement: &'a Statement, first: &str, second: &str) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    matches!(arguments.as_slice(), [left, right]
        if is_named_variable(left, first) && is_named_variable(right, second))
    .then_some(name.as_str())
}

fn one_argument_statement_call<'a>(statement: &'a Statement, argument: &str) -> Option<&'a str> {
    let Statement::Expression(expression) = statement else {
        return None;
    };
    direct_single_argument_call(expression, argument)
}

fn direct_single_argument_call<'a>(expression: &'a Expression, argument: &str) -> Option<&'a str> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    matches!(arguments.as_slice(), [value] if is_named_variable(value, argument))
        .then_some(name.as_str())
}

fn is_member(expression: &Expression, base_name: &str, offset: u32, member_type: Type) -> bool {
    matches!(expression,
        Expression::Member {
            base,
            offset: actual_offset,
            member_type: actual_type,
            index_stride: None,
        } if *actual_offset == offset
            && *actual_type == member_type
            && is_named_variable(base, base_name))
}

fn is_struct_pointer_member(expression: &Expression, base_name: &str, offset: u32) -> bool {
    matches!(expression,
        Expression::Member {
            base,
            offset: actual_offset,
            member_type: Type::StructPointer { .. },
            index_stride: None,
        } if *actual_offset == offset && is_named_variable(base, base_name))
}

fn is_named_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(actual) if actual == name)
}
