//! Four-field floating member normalization through an out-of-line selector.

use mwcc_machine_code::{
    FrameInfo, Instruction, MachineFunction, Relocation, RelocationKind, RelocationTarget,
};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Statement, Type};
use mwcc_versions::Behavior;

/// Lower the common rectangle/bounds normalization shape which snapshots four
/// float members, then calls one selector for min/max of each opposing pair.
/// The snapshots live in f31..f28 across four calls and `this` lives in r31.
pub(crate) fn lower_member_float_normalize(
    function: &Function,
    behavior: &Behavior,
    cpp_exceptions: bool,
) -> Option<MachineFunction> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || !function.inline_asm_blocks.is_empty()
    {
        return None;
    }
    let [object] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        object.parameter_type,
        Type::StructPointer { element_size: 16 }
    ) {
        return None;
    }
    let [left, top, right, bottom] = function.locals.as_slice() else {
        return None;
    };
    let locals = [left, top, right, bottom];
    for (local, offset) in locals.iter().zip([0, 4, 8, 12]) {
        if local.declared_type != Type::Float
            || local.is_static
            || local.array_length.is_some()
            || !member_read(local.initializer.as_ref()?, &object.name, offset)
        {
            return None;
        }
    }

    let [left_store, right_store, top_store, bottom_store] = function.statements.as_slice() else {
        return None;
    };
    let callee = selector_store(
        left_store,
        &object.name,
        0,
        &right.name,
        &left.name,
        &left.name,
        &right.name,
    )?;
    if selector_store(
        right_store,
        &object.name,
        8,
        &right.name,
        &left.name,
        &right.name,
        &left.name,
    )? != callee
        || selector_store(
            top_store,
            &object.name,
            4,
            &bottom.name,
            &top.name,
            &top.name,
            &bottom.name,
        )? != callee
        || selector_store(
            bottom_store,
            &object.name,
            12,
            &bottom.name,
            &top.name,
            &bottom.name,
            &top.name,
        )? != callee
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
            offset: -80,
        },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 84,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 64,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 31,
            a: 1,
            offset: 72,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 30,
            a: 1,
            offset: 48,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 30,
            a: 1,
            offset: 56,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 29,
            a: 1,
            offset: 32,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 29,
            a: 1,
            offset: 40,
            w: 0,
            i: 0,
        },
        Instruction::StoreFloatDouble {
            s: 28,
            a: 1,
            offset: 16,
        },
        Instruction::PairedSingleQuantizedStore {
            s: 28,
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
        Instruction::move_register(31, 3),
        Instruction::LoadFloatSingle {
            d: 31,
            a: 31,
            offset: 0,
        },
        Instruction::LoadFloatSingle {
            d: 30,
            a: 31,
            offset: 4,
        },
        Instruction::LoadFloatSingle {
            d: 29,
            a: 31,
            offset: 8,
        },
        Instruction::LoadFloatSingle {
            d: 28,
            a: 31,
            offset: 12,
        },
    ]);
    emit_select(&mut output, callee, 29, 31, 31, 29, 0);
    emit_select(&mut output, callee, 29, 31, 29, 31, 8);
    emit_select(&mut output, callee, 28, 30, 30, 28, 4);
    emit_select(&mut output, callee, 28, 30, 28, 30, 12);
    for (register, psq_offset, double_offset) in
        [(31, 72, 64), (30, 56, 48), (29, 40, 32), (28, 24, 16)]
    {
        output
            .instructions
            .push(Instruction::load_immediate(0, psq_offset));
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
            d: 0,
            a: 1,
            offset: 84,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 80,
        },
        Instruction::BranchToLinkRegister,
    ]);
    output.symbol_order.push(callee.to_owned());
    output.referenced_function_symbols.push(callee.to_owned());
    if cpp_exceptions {
        output.frame = Some(FrameInfo {
            saved_gpr_count: 1,
            saved_fpr_count: 4,
            uses_fpu: behavior.mark_single_precision_extab,
        });
    }
    Some(output)
}

fn member_read(expression: &Expression, object: &str, offset: u32) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            offset: actual,
            member_type: Type::Float,
            index_stride: None,
        } if *actual == offset && matches!(base.as_ref(), Expression::Variable(name) if name == object)
    )
}

fn selector_store<'a>(
    statement: &'a Statement,
    object: &str,
    offset: u32,
    difference_left: &str,
    difference_right: &str,
    positive: &str,
    negative: &str,
) -> Option<&'a str> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: actual_offset,
                member_type: Type::Float,
                index_stride: None,
            },
        value: Expression::Call { name, arguments },
    } = statement
    else {
        return None;
    };
    let [Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    }, Expression::Variable(actual_positive), Expression::Variable(actual_negative)] =
        arguments.as_slice()
    else {
        return None;
    };
    if *actual_offset != offset
        || !matches!(base.as_ref(), Expression::Variable(actual) if actual == object)
        || !matches!(left.as_ref(), Expression::Variable(actual) if actual == difference_left)
        || !matches!(right.as_ref(), Expression::Variable(actual) if actual == difference_right)
        || actual_positive != positive
        || actual_negative != negative
    {
        return None;
    }
    Some(name)
}

fn emit_select(
    output: &mut MachineFunction,
    callee: &str,
    difference_left: u8,
    difference_right: u8,
    positive: u8,
    negative: u8,
    member_offset: i16,
) {
    output.instructions.extend([
        Instruction::FloatSubtractSingle {
            d: 1,
            a: difference_left,
            b: difference_right,
        },
        Instruction::FloatMove { d: 2, b: positive },
        Instruction::FloatMove { d: 3, b: negative },
    ]);
    output.relocations.push(Relocation {
        instruction_index: output.instructions.len(),
        kind: RelocationKind::Rel24,
        target: RelocationTarget::External(callee.to_owned()),
    });
    output.instructions.push(Instruction::BranchAndLink {
        target: callee.to_owned(),
    });
    output.instructions.push(Instruction::StoreFloatSingle {
        s: 1,
        a: 31,
        offset: member_offset,
    });
}
