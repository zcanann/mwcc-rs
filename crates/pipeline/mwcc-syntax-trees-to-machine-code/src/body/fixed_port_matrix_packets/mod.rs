//! Three fixed-port matrix packets fed by scaled float pairs.

mod emit;
mod intrinsic;

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldOperation {
    ShiftOr,
    RotateInsert,
}

pub(super) struct MatrixPacket<'a> {
    pub(super) matrix_id: &'a str,
    pub(super) source: &'a str,
    pub(super) scale: &'a str,
    pub(super) values: &'a str,
    pub(super) word: &'a str,
    pub(super) packet_id: &'a str,
    pub(super) global: &'a str,
    pub(super) flag_offset: i16,
    pub(super) operation: FieldOperation,
    pub(super) port: u32,
    pub(super) command: i16,
}

fn word_value(mut expression: &Expression) -> &Expression {
    while let Expression::Cast {
        target_type: Type::Int | Type::UnsignedInt,
        operand,
    } = expression
    {
        expression = operand;
    }
    expression
}

fn no_op(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Cast {
        target_type: Type::Void,
        operand,
    }) if constant_value(operand) == Some(0))
}

fn switch_assigns_ranges(statement: &Statement, matrix_id: &str, packet_id: &str) -> bool {
    let Statement::Switch {
        scrutinee: Expression::Variable(scrutinee),
        arms,
        default: Some(ArmBody::Statements(default)),
    } = statement
    else {
        return false;
    };
    if scrutinee != matrix_id
        || arms.iter().map(|arm| arm.value).collect::<Vec<_>>() != [1, 2, 3, 5, 6, 7, 9, 10, 11]
        || arms
            .iter()
            .enumerate()
            .any(|(index, arm)| arm.falls_through != !matches!(index, 2 | 5 | 8))
    {
        return false;
    }
    for (index, subtract) in [(2usize, 1), (5, 5), (8, 9)] {
        let ArmBody::Statements(body) = &arms[index].body else {
            return false;
        };
        if !matches!(body.as_slice(), [Statement::Assign { name, value: Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        }}] if name == packet_id
            && matches!(left.as_ref(), Expression::Variable(name) if name == matrix_id)
            && constant_value(right) == Some(subtract))
        {
            return false;
        }
    }
    arms.iter().enumerate().all(|(index, arm)| {
        matches!(&arm.body, ArmBody::Statements(body) if matches!(index, 2 | 5 | 8) || body.is_empty())
    }) && matches!(default.as_slice(), [Statement::Assign { name, value }]
        if name == packet_id && constant_value(value) == Some(0))
}

fn matrix_store(
    statement: &Statement,
    values: &str,
    index: i64,
    source: &str,
    source_offset: u32,
) -> bool {
    let Statement::Store {
        target: Expression::Index {
            base,
            index: target_index,
        },
        value:
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            },
    } = statement
    else {
        return false;
    };
    if !matches!(base.as_ref(), Expression::Variable(name) if name == values)
        || constant_value(target_index) != Some(index)
        || constant_value(right) != Some(2047)
    {
        return false;
    }
    let Expression::Cast {
        target_type: Type::Int,
        operand: converted,
    } = left.as_ref()
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: factor,
        right: sample,
    } = converted.as_ref()
    else {
        return false;
    };
    matches!(factor.as_ref(), Expression::FloatLiteral(value) if *value == 1024.0)
        && matches!(sample.as_ref(), Expression::Member {
            base, offset, member_type: Type::Float, index_stride: None
        } if *offset == source_offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == source))
}

fn scale_update(statement: &Statement, scale: &str) -> bool {
    matches!(statement, Statement::Assign { name, value: Expression::Binary {
        operator: BinaryOperator::Add, left, right
    }} if name == scale
        && matches!(left.as_ref(), Expression::Variable(name) if name == scale)
        && constant_value(right) == Some(17))
}

fn zero_word(statement: &Statement, word: &str) -> bool {
    matches!(statement, Statement::Assign { name, value }
        if name == word && constant_value(value) == Some(0))
}

fn field_insert<'a>(
    statement: &'a Statement,
    word: &str,
    preserve: u32,
    shift: i64,
) -> Option<(&'a Expression, FieldOperation)> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    if name != word {
        return None;
    }
    if let Expression::Call { name, arguments } = word_value(value) {
        if crate::intrinsics::classify(name, arguments.len())
            != Some(crate::intrinsics::Intrinsic::RotateLeftWordInsert)
        {
            return None;
        }
        let insert = crate::intrinsics::rotate_insert(arguments)?;
        if insert.shift != shift as u8
            || insert.begin > insert.end
            || run_mask(insert.begin, insert.end) != !preserve
            || !matches!(word_value(insert.initial), Expression::Variable(name) if name == word)
        {
            return None;
        }
        return Some((word_value(insert.source), FieldOperation::RotateInsert));
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: old,
        right: mask,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: inserted,
        right: found_shift,
    } = right.as_ref()
    else {
        return None;
    };
    (name == word
        && matches!(word_value(old), Expression::Variable(name) if name == word)
        && constant_value(mask).map(|value| value as u32) == Some(preserve)
        && constant_value(found_shift) == Some(shift))
    .then_some((word_value(inserted), FieldOperation::ShiftOr))
}

fn indexed(expression: &Expression, values: &str, index: i64) -> bool {
    matches!(expression, Expression::Index { base, index: found }
        if matches!(base.as_ref(), Expression::Variable(name) if name == values)
            && constant_value(found) == Some(index))
}

fn scale_bits(expression: &Expression, scale: &str, right_shift: i64) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return false;
    };
    if constant_value(right) != Some(3) {
        return false;
    }
    if right_shift == 0 {
        matches!(left.as_ref(), Expression::Variable(name) if name == scale)
    } else {
        matches!(left.as_ref(), Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: shifted,
            right: amount,
        } if matches!(shifted.as_ref(), Expression::Variable(name) if name == scale)
            && constant_value(amount) == Some(right_shift))
    }
}

fn packet_number(expression: &Expression, packet_id: &str, addend: i64) -> bool {
    matches!(expression, Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } if constant_value(right) == Some(addend)
        && matches!(left.as_ref(), Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: id,
            right: factor,
        } if matches!(id.as_ref(), Expression::Variable(name) if name == packet_id)
            && constant_value(factor) == Some(3)))
}

fn port_write(command: &Statement, data: &Statement, word: &str) -> Option<(u32, i16)> {
    let port_target = |target: &Expression, member_type| {
        let Expression::Member {
            base,
            offset: 0,
            member_type: found,
            index_stride: None,
        } = target
        else {
            return None;
        };
        if *found != member_type {
            return None;
        }
        let Expression::Cast {
            target_type: Type::Pointer(_) | Type::StructPointer { .. },
            operand,
        } = base.as_ref()
        else {
            return None;
        };
        Some(constant_value(operand)? as u32)
    };
    let Statement::Store {
        target: command_target,
        value: command,
    } = command
    else {
        return None;
    };
    let Statement::Store {
        target: data_target,
        value: data,
    } = data
    else {
        return None;
    };
    let port = port_target(command_target, Type::UnsignedChar)?;
    if port_target(data_target, Type::UnsignedInt)? != port
        || !matches!(word_value(data), Expression::Variable(name) if name == word)
    {
        return None;
    }
    Some((port, i16::try_from(constant_value(command)?).ok()?))
}

pub(super) fn recognize(function: &Function) -> Option<MatrixPacket<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [matrix_id, source, scale] = function.parameters.as_slice() else {
        return None;
    };
    if matrix_id.parameter_type != Type::Int
        || source.parameter_type != Type::Pointer(Pointee::Float)
        || scale.parameter_type != Type::Char
    {
        return None;
    }
    let [values, word, packet_id] = function.locals.as_slice() else {
        return None;
    };
    if values.declared_type != Type::Int
        || values.array_length != Some(6)
        || word.declared_type != Type::UnsignedInt
        || packet_id.declared_type != Type::UnsignedInt
        || function
            .locals
            .iter()
            .any(|local| local.initializer.is_some() || local.is_static || local.is_volatile)
    {
        return None;
    }
    let statements = match function.statements.as_slice() {
        [noop, rest @ ..] if no_op(noop) => rest,
        statements => statements,
    };
    let [select, a0, a1, scale_add, zero0, f00, f01, f02, f03, command0, data0, a2, a3, zero1, f10, f11, f12, f13, command1, data1, a4, a5, zero2, f20, f21, f22, f23, command2, data2, flag] =
        statements
    else {
        return None;
    };
    if !switch_assigns_ranges(select, &matrix_id.name, &packet_id.name)
        || !scale_update(scale_add, &scale.name)
        || !zero_word(zero0, &word.name)
        || !zero_word(zero1, &word.name)
        || !zero_word(zero2, &word.name)
        || !matrix_store(a0, &values.name, 0, &source.name, 0)
        || !matrix_store(a1, &values.name, 1, &source.name, 12)
        || !matrix_store(a2, &values.name, 2, &source.name, 4)
        || !matrix_store(a3, &values.name, 3, &source.name, 16)
        || !matrix_store(a4, &values.name, 4, &source.name, 8)
        || !matrix_store(a5, &values.name, 5, &source.name, 20)
    {
        return None;
    }
    let (port, command) = port_write(command0, data0, &word.name)?;
    if port_write(command1, data1, &word.name)? != (port, command)
        || port_write(command2, data2, &word.name)? != (port, command)
    {
        return None;
    }
    let mut operation = None;
    for (packet, fields) in [
        [f00, f01, f02, f03],
        [f10, f11, f12, f13],
        [f20, f21, f22, f23],
    ]
    .into_iter()
    .enumerate()
    {
        let first = field_insert(fields[0], &word.name, 0xffff_f800, 0)?;
        let second = field_insert(fields[1], &word.name, 0xffc0_07ff, 11)?;
        let third = field_insert(fields[2], &word.name, 0xff3f_ffff, 22)?;
        let fourth = field_insert(fields[3], &word.name, 0x00ff_ffff, 24)?;
        for kind in [first.1, second.1, third.1, fourth.1] {
            if operation.is_some_and(|old| old != kind) {
                return None;
            }
            operation = Some(kind);
        }
        if !indexed(first.0, &values.name, (packet * 2) as i64)
            || !indexed(second.0, &values.name, (packet * 2 + 1) as i64)
            || !scale_bits(third.0, &scale.name, (packet * 2) as i64)
            || !packet_number(fourth.0, &packet_id.name, packet as i64 + 6)
        {
            return None;
        }
    }
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type: Type::UnsignedShort,
                index_stride: None,
            },
        value,
    } = flag
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    (constant_value(value) == Some(0)).then_some(MatrixPacket {
        operation: operation?,
        port,
        command,
        matrix_id: &matrix_id.name,
        source: &source.name,
        scale: &scale.name,
        values: &values.name,
        word: &word.name,
        packet_id: &packet_id.name,
        global,
        flag_offset: i16::try_from(*offset).ok()?,
    })
}
