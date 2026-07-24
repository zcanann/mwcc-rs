//! Three fixed-port matrix packets fed by scaled float pairs.

mod emit;

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

pub(super) struct MatrixPacket<'a> {
    pub(super) matrix_id: &'a str,
    pub(super) source: &'a str,
    pub(super) scale: &'a str,
    pub(super) values: &'a str,
    pub(super) word: &'a str,
    pub(super) packet_id: &'a str,
    pub(super) global: &'a str,
    pub(super) flag_offset: i16,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
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

fn switch_assigns_ranges(
    statement: &Statement,
    matrix_id: &str,
    packet_id: &str,
) -> bool {
    let Statement::Switch {
        scrutinee: Expression::Variable(scrutinee),
        arms,
        default: Some(ArmBody::Statements(default)),
    } = statement
    else {
        return false;
    };
    if scrutinee != matrix_id
        || arms.iter().map(|arm| arm.value).collect::<Vec<_>>()
            != [1, 2, 3, 5, 6, 7, 9, 10, 11]
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
        target: Expression::Index { base, index: target_index },
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
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: factor,
        right: sample,
    } = stripped(left)
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
) -> Option<&'a Expression> {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } = statement
    else {
        return None;
    };
    if constant_value(condition) != Some(0) {
        return None;
    }
    let [Statement::Assign {
        name,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            },
    }] = body.as_slice()
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
        && matches!(stripped(old), Expression::Variable(name) if name == word)
        && constant_value(mask).map(|value| value as u32) == Some(preserve)
        && constant_value(found_shift) == Some(shift))
    .then_some(stripped(inserted))
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

fn port_write(statement: &Statement, word: &str) -> bool {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } = statement
    else {
        return false;
    };
    if constant_value(condition) != Some(0) {
        return false;
    }
    let [Statement::Store { target: command_target, value: command },
        Statement::Store { target: data_target, value: data }] = body.as_slice()
    else {
        return false;
    };
    let port_target = |target: &Expression, member_type| {
        matches!(target, Expression::Member {
            base, offset: 0, member_type: found, index_stride: None
        } if *found == member_type
            && matches!(stripped(base), Expression::IntegerLiteral(value)
                if *value as u32 == 0xcc00_8000))
    };
    port_target(command_target, Type::UnsignedChar)
        && constant_value(command) == Some(0x61)
        && port_target(data_target, Type::UnsignedInt)
        && matches!(stripped(data), Expression::Variable(name) if name == word)
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
        || function.locals.iter().any(|local| {
            local.initializer.is_some() || local.is_static || local.is_volatile
        })
    {
        return None;
    }
    let [noop, select,
        a0, a1, scale_add, zero0, f00, f01, f02, f03, port0,
        a2, a3, zero1, f10, f11, f12, f13, port1,
        a4, a5, zero2, f20, f21, f22, f23, port2, flag] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !no_op(noop)
        || !switch_assigns_ranges(select, &matrix_id.name, &packet_id.name)
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
        || !port_write(port0, &word.name)
        || !port_write(port1, &word.name)
        || !port_write(port2, &word.name)
    {
        return None;
    }
    for (packet, fields) in [[f00, f01, f02, f03], [f10, f11, f12, f13], [f20, f21, f22, f23]]
        .into_iter()
        .enumerate()
    {
        let first = field_insert(fields[0], &word.name, 0xffff_f800, 0)?;
        let second = field_insert(fields[1], &word.name, 0xffc0_07ff, 11)?;
        let third = field_insert(fields[2], &word.name, 0xff3f_ffff, 22)?;
        let fourth = field_insert(fields[3], &word.name, 0x00ff_ffff, 24)?;
        if !indexed(first, &values.name, (packet * 2) as i64)
            || !indexed(second, &values.name, (packet * 2 + 1) as i64)
            || !scale_bits(third, &scale.name, (packet * 2) as i64)
            || !packet_number(fourth, &packet_id.name, packet as i64 + 6)
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
