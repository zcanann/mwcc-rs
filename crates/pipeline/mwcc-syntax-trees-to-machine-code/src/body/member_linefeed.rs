//! Cursor linefeed through an aggregate-held writer reference.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::Behavior;

/// Lower the Wii schedule for snapshotting an origin coordinate, calling two
/// writer accessors, adding their results, and setting the resulting cursor.
/// The shape is semantic: names of the concrete writer/template instance are
/// taken from its calls rather than captured from one project symbol.
pub(crate) fn lower_member_linefeed(
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
    let [writer, x, y] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(writer.declared_type, Type::StructPointer { .. })
        || x.declared_type != Type::Float
        || y.declared_type != Type::Float
        || function.locals.iter().any(|local| {
            local.is_static || local.array_length.is_some() || local.initializer.is_some()
        })
    {
        return None;
    }
    let [noop, bind_writer, load_origin, compute_y, set_cursor] = function.statements.as_slice()
    else {
        return None;
    };
    if !is_void_zero(noop)
        || !matches!(bind_writer,
            Statement::Assign { name, value: Expression::Dereference { pointer } }
                if name == &writer.name && matches!(pointer.as_ref(),
                    Expression::Member {
                        base,
                        offset: 0,
                        member_type: Type::StructPointer { .. },
                        index_stride: None,
                    } if is_variable(base, &context.name)))
        || !matches!(load_origin,
            Statement::Assign { name, value: Expression::Member {
                base,
                offset: 8,
                member_type: Type::Float,
                index_stride: None,
            }} if name == &x.name && is_variable(base, &context.name))
    {
        return None;
    }
    let Statement::Assign {
        name: computed_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            },
    } = compute_y
    else {
        return None;
    };
    if computed_name != &y.name {
        return None;
    }
    let cursor_y = direct_single_argument_call(left, &writer.name)?;
    let line_height = direct_single_argument_call(right, &writer.name)?;
    let Statement::Expression(Expression::Call {
        name: set_cursor_name,
        arguments,
    }) = set_cursor
    else {
        return None;
    };
    if !matches!(arguments.as_slice(), [writer_arg, x_arg, y_arg]
        if is_named_variable(writer_arg, &writer.name)
            && is_named_variable(x_arg, &x.name)
            && is_named_variable(y_arg, &y.name))
    {
        return None;
    }

    let mut output = MachineFunction::new(function.name.clone());
    output.pre_scheduled = true;
    output.is_static = function.is_static;
    output.is_weak = function.is_weak;
    output.section = function.section.clone();
    output.force_active = function.force_active;
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
        Instruction::StoreFloatDouble {
            s: 30,
            a: 1,
            offset: 32,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 30,
            a: 1,
            offset: 40,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 29,
            a: 1,
            offset: 16,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 29,
            a: 1,
            offset: 24,
            w: 0,
            i: 0,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 8,
        },
        Instruction::move_register(30, 4),
        Instruction::LoadWord {
            d: 31,
            a: 30,
            offset: 0,
        },
        Instruction::LoadFloatSingle {
            d: 31,
            a: 30,
            offset: 8,
        },
        Instruction::move_register(3, 31),
    ]);
    emit_call(&mut output, line_height);
    output.instructions.extend([
        Instruction::FloatMove { d: 29, b: 1 },
        Instruction::move_register(3, 31),
    ]);
    emit_call(&mut output, cursor_y);
    output.instructions.extend([
        Instruction::FloatAddSingle { d: 30, a: 1, b: 29 },
        Instruction::move_register(3, 31),
        Instruction::FloatMove { d: 1, b: 31 },
        Instruction::FloatMove { d: 2, b: 30 },
    ]);
    emit_call(&mut output, set_cursor_name);
    for (register, paired_offset, double_offset) in [(31, 56, 48), (30, 40, 32), (29, 24, 16)] {
        output
            .instructions
            .push(Instruction::load_immediate(0, paired_offset));
        output
            .instructions
            .push(Instruction::PairedSingleQuantizedLoadIndexed {
                d: register,
                a: 1,
                b: 0,
                w: 0,
                i: 0,
            });
        output.instructions.push(Instruction::LoadFloatDouble {
            d: register,
            a: 1,
            offset: double_offset,
        });
    }
    output.instructions.extend([
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 8,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 68,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        },
        Instruction::BranchToLinkRegister,
    ]);
    output.symbol_order.extend([
        cursor_y.to_owned(),
        line_height.to_owned(),
        set_cursor_name.to_owned(),
    ]);
    output.referenced_function_symbols = output.symbol_order.clone();
    if cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 2,
            saved_fpr_count: 3,
            uses_fpu: behavior.mark_single_precision_extab,
        });
    }
    Some(output)
}

fn is_void_zero(statement: &Statement) -> bool {
    matches!(statement,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) if matches!(operand.as_ref(), Expression::IntegerLiteral(0)))
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    is_named_variable(expression, name)
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
