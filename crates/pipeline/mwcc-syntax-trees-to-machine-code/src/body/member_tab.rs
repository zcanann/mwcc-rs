//! Advance a writer cursor to the next tab stop.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, ConditionalOrigin, Expression, Function, Statement, Type};
use mwcc_versions::Behavior;

/// Lower the Wii schedule for a tab stop computed from an aggregate-held writer.
/// The recognizer owns the source-level operation; concrete template and writer
/// names are recovered from the calls in the tree.
pub(crate) fn lower_member_tab(
    function: &Function,
    behavior: &Behavior,
    cpp_exceptions: bool,
    indexed_restore: bool,
) -> Option<MachineFunction> {
    if !indexed_restore
        || function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || !function.inline_asm_blocks.is_empty()
    {
        return None;
    }
    let [_, context] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(context.parameter_type, Type::StructPointer { .. }) {
        return None;
    }
    let [writer, tab_width, char_width, dx, tab_pixels, tab_count, cursor_x] =
        function.locals.as_slice()
    else {
        return None;
    };
    if !matches!(writer.declared_type, Type::StructPointer { .. })
        || tab_width.declared_type != Type::Int
        || char_width.declared_type != Type::Float
        || dx.declared_type != Type::Float
        || tab_pixels.declared_type != Type::Float
        || tab_count.declared_type != Type::Int
        || cursor_x.declared_type != Type::Float
        || function.locals.iter().any(|local| {
            local.is_static || local.array_length.is_some() || local.initializer.is_some()
        })
    {
        return None;
    }

    let [noop, bind_writer, read_tab_width, guarded_body] = function.statements.as_slice() else {
        return None;
    };
    if !is_void_zero(noop) || !is_writer_binding(bind_writer, &writer.name, &context.name) {
        return None;
    }
    let Statement::Assign {
        name: assigned_tab_width,
        value: tab_width_call,
    } = read_tab_width
    else {
        return None;
    };
    if assigned_tab_width != &tab_width.name {
        return None;
    }
    let get_tab_width = direct_single_argument_call(tab_width_call, &writer.name)?;

    let Statement::If {
        condition,
        then_body,
        else_body,
    } = guarded_body
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(condition,
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            } if is_named_variable(left, &tab_width.name)
                && matches!(right.as_ref(), Expression::IntegerLiteral(0)))
    {
        return None;
    }
    let [choose_width, compute_dx, compute_tab_pixels, compute_tab_count, compute_cursor, set_cursor] =
        then_body.as_slice()
    else {
        return None;
    };

    let Statement::Assign {
        name: assigned_char_width,
        value:
            Expression::Conditional {
                condition: fixed_condition,
                when_true: fixed_width,
                when_false: font_width,
                origin: ConditionalOrigin::Ternary,
            },
    } = choose_width
    else {
        return None;
    };
    if assigned_char_width != &char_width.name {
        return None;
    }
    let is_width_fixed = direct_single_argument_call(fixed_condition, &writer.name)?;
    let get_fixed_width = direct_single_argument_call(fixed_width, &writer.name)?;
    let get_font_width = direct_single_argument_call(font_width, &writer.name)?;

    let Statement::Assign {
        name: assigned_dx,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: cursor_read,
                right: origin_read,
            },
    } = compute_dx
    else {
        return None;
    };
    if assigned_dx != &dx.name || !is_member(origin_read, &context.name, 8, Type::Float) {
        return None;
    }
    let get_cursor_x = direct_single_argument_call(cursor_read, &writer.name)?;

    if !matches!(compute_tab_pixels,
        Statement::Assign {
            name,
            value: Expression::Binary {
                operator: BinaryOperator::Multiply,
                left,
                right,
            },
        } if name == &tab_pixels.name
            && is_cast_variable(left, Type::Float, &tab_width.name)
            && is_named_variable(right, &char_width.name))
        || !matches!(compute_tab_count,
            Statement::Assign {
                name,
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                },
            } if name == &tab_count.name
                && is_int_quotient_cast(left, &dx.name, &tab_pixels.name)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1)))
        || !matches!(compute_cursor,
            Statement::Assign {
                name,
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                },
            } if name == &cursor_x.name
                && is_tab_product(left, &tab_pixels.name, &tab_count.name)
                && is_member(right, &context.name, 8, Type::Float))
    {
        return None;
    }
    let Statement::Expression(set_cursor_expression) = set_cursor else {
        return None;
    };
    let Expression::Call {
        name: set_cursor_x,
        arguments,
    } = set_cursor_expression
    else {
        return None;
    };
    if !matches!(arguments.as_slice(), [writer_value, cursor_value]
        if is_named_variable(writer_value, &writer.name)
            && is_named_variable(cursor_value, &cursor_x.name))
    {
        return None;
    }

    Some(emit_member_tab(
        function,
        behavior,
        cpp_exceptions,
        [
            get_tab_width,
            is_width_fixed,
            get_fixed_width,
            get_font_width,
            get_cursor_x,
            set_cursor_x,
        ],
    ))
}

fn emit_member_tab(
    function: &Function,
    behavior: &Behavior,
    cpp_exceptions: bool,
    calls: [&str; 6],
) -> MachineFunction {
    let [get_tab_width, is_width_fixed, get_fixed_width, get_font_width, get_cursor_x, set_cursor_x] =
        calls;
    let mut output = MachineFunction::new(function.name.clone());
    output.pre_scheduled = true;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
    output.has_conversion = true;
    output.instructions.extend([
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -144,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 148,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 128,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 31,
            a: 1,
            offset: 136,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 30,
            a: 1,
            offset: 112,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 30,
            a: 1,
            offset: 120,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 29,
            a: 1,
            offset: 96,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 29,
            a: 1,
            offset: 104,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 28,
            a: 1,
            offset: 80,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 28,
            a: 1,
            offset: 88,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 27,
            a: 1,
            offset: 64,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 27,
            a: 1,
            offset: 72,
            w: 0,
            i: 0,
        },
        Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 64,
        },
    ]);
    emit_call(&mut output, "_savegpr_27");
    output.instructions.extend([
        Instruction::move_register(31, 1),
        Instruction::move_register(29, 4),
        Instruction::LoadWord {
            d: 30,
            a: 29,
            offset: 0,
        },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, get_tab_width);
    output.instructions.extend([
        Instruction::move_register(28, 3),
        Instruction::CompareWordImmediate {
            a: 28,
            immediate: 0,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: 65,
        },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, is_width_fixed);
    output.instructions.extend([
        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 31,
        },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, get_fixed_width);
    output.instructions.extend([
        Instruction::FloatMove { d: 27, b: 1 },
        Instruction::Branch { target: 34 },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, get_font_width);
    output.instructions.extend([
        Instruction::FloatMove { d: 27, b: 1 },
        Instruction::FloatMove { d: 30, b: 27 },
        Instruction::move_register(3, 30),
    ]);
    emit_call(&mut output, get_cursor_x);
    output.instructions.extend([
        Instruction::LoadFloatSingle {
            d: 0,
            a: 29,
            offset: 8,
        },
        Instruction::FloatSubtractSingle { d: 29, a: 1, b: 0 },
    ]);
    let bias = output.intern_constant(0x4330_0000_8000_0000, 8);
    emit_bias_load(&mut output, bias, 1);
    output.instructions.extend([
        Instruction::XorImmediateShifted {
            a: 0,
            s: 28,
            immediate: 0x8000,
        },
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 12,
        },
        Instruction::load_immediate_shifted(0, 0x4330),
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 8,
        },
        Instruction::LoadFloatDouble {
            d: 0,
            a: 31,
            offset: 8,
        },
        Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 },
        Instruction::FloatMultiplySingle { d: 31, a: 0, c: 30 },
        Instruction::FloatDivideSingle { d: 0, a: 29, b: 31 },
        Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
        Instruction::StoreFloatDouble {
            s: 0,
            a: 31,
            offset: 16,
        },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: 20,
        },
        Instruction::AddImmediate {
            d: 27,
            a: 3,
            immediate: 1,
        },
    ]);
    emit_bias_load(&mut output, bias, 1);
    output.instructions.extend([
        Instruction::XorImmediateShifted {
            a: 0,
            s: 27,
            immediate: 0x8000,
        },
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 28,
        },
        Instruction::load_immediate_shifted(0, 0x4330),
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 24,
        },
        Instruction::LoadFloatDouble {
            d: 0,
            a: 31,
            offset: 24,
        },
        Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 },
        Instruction::FloatMultiplySingle { d: 1, a: 31, c: 0 },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 29,
            offset: 8,
        },
        Instruction::FloatAddSingle { d: 28, a: 0, b: 1 },
        Instruction::move_register(3, 30),
        Instruction::FloatMove { d: 1, b: 28 },
    ]);
    emit_call(&mut output, set_cursor_x);
    output.instructions.push(Instruction::move_register(10, 31));
    for (register, paired_offset, double_offset) in [
        (31, 136, 128),
        (30, 120, 112),
        (29, 104, 96),
        (28, 88, 80),
        (27, 72, 64),
    ] {
        output
            .instructions
            .push(Instruction::load_immediate(0, paired_offset));
        output
            .instructions
            .push(Instruction::PairedSingleQuantizedLoadIndexed {
                d: register,
                a: 10,
                b: 0,
                w: 0,
                i: 0,
            });
        output.instructions.push(Instruction::LoadFloatDouble {
            d: register,
            a: 10,
            offset: double_offset,
        });
    }
    output.instructions.push(Instruction::AddImmediate {
        d: 11,
        a: 10,
        immediate: 64,
    });
    emit_call(&mut output, "_restgpr_27");
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
    debug_assert_eq!(output.instructions.len(), 88);

    output.symbol_order = calls.into_iter().map(str::to_owned).collect();
    output.referenced_function_symbols = ["_savegpr_27", "_restgpr_27"]
        .into_iter()
        .chain(calls)
        .map(str::to_owned)
        .collect();
    if cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 5,
            saved_fpr_count: 5,
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

fn emit_bias_load(output: &mut MachineFunction, constant: usize, destination: u8) {
    output.relocations.push(Relocation {
        instruction_index: output.instructions.len(),
        kind: RelocationKind::EmbSda21,
        target: RelocationTarget::Constant(constant),
    });
    output.instructions.push(Instruction::LoadFloatDouble {
        d: destination,
        a: 0,
        offset: 0,
    });
}

fn is_void_zero(statement: &Statement) -> bool {
    matches!(statement,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) if matches!(operand.as_ref(), Expression::IntegerLiteral(0)))
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

fn is_named_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(actual) if actual == name)
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

fn is_cast_variable(expression: &Expression, target_type: Type, variable: &str) -> bool {
    matches!(expression,
        Expression::Cast { target_type: actual_type, operand }
            if *actual_type == target_type && is_named_variable(operand, variable))
}

fn is_int_quotient_cast(expression: &Expression, numerator: &str, denominator: &str) -> bool {
    matches!(expression,
        Expression::Cast {
            target_type: Type::Int,
            operand,
        } if matches!(operand.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Divide,
                left,
                right,
            } if is_named_variable(left, numerator) && is_named_variable(right, denominator)))
}

fn is_tab_product(expression: &Expression, pixels: &str, count: &str) -> bool {
    matches!(expression,
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } if is_named_variable(left, pixels) && is_cast_variable(right, Type::Float, count))
}
