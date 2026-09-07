//! Structural recognition for full-width local packet accumulators.

#[allow(unused_imports)]
use super::super::*;

#[derive(Clone, Copy)]
pub(super) struct FieldSource<'a> {
    pub(super) parameter: &'a str,
    pub(super) addend: i64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldOperation {
    ShiftOr,
    RotateInsert,
}

#[derive(Clone, Copy)]
pub(super) struct FieldInsert<'a> {
    pub(super) source: FieldSource<'a>,
    pub(super) preserve_mask: u32,
    pub(super) shift: u8,
    pub(super) operation: FieldOperation,
}

pub(super) struct Packet<'a> {
    pub(super) fields: Vec<FieldInsert<'a>>,
    pub(super) command: i16,
    pub(super) port: u32,
    pub(super) global: &'a str,
    pub(super) flag_offset: i16,
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

fn field_source(expression: &Expression) -> Option<FieldSource<'_>> {
    let expression = word_value(expression);
    match expression {
        Expression::Variable(parameter) => Some(FieldSource {
            parameter,
            addend: 0,
        }),
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } => {
            let (parameter, addend) = match (word_value(left), word_value(right)) {
                (Expression::Variable(parameter), constant) => {
                    (parameter.as_str(), constant_value(constant)?)
                }
                (constant, Expression::Variable(parameter)) => {
                    (parameter.as_str(), constant_value(constant)?)
                }
                _ => return None,
            };
            Some(FieldSource { parameter, addend })
        }
        _ => None,
    }
}

fn field_insert<'a>(statement: &'a Statement, accumulator: &str) -> Option<FieldInsert<'a>> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    if name != accumulator {
        return None;
    }
    if let Expression::Call { name, arguments } = word_value(value) {
        if crate::intrinsics::classify(name, arguments.len())
            != Some(crate::intrinsics::Intrinsic::RotateLeftWordInsert)
        {
            return None;
        }
        let insert = crate::intrinsics::rotate_insert(arguments)?;
        if insert.begin > insert.end
            || !matches!(word_value(insert.initial), Expression::Variable(name) if name == accumulator)
        {
            return None;
        }
        return Some(FieldInsert {
            source: field_source(insert.source)?,
            preserve_mask: !run_mask(insert.begin, insert.end),
            shift: insert.shift,
            operation: FieldOperation::RotateInsert,
        });
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
        left: old_value,
        right: preserve,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!(word_value(old_value), Expression::Variable(name) if name == accumulator) {
        return None;
    }
    let preserve_mask = constant_value(preserve)? as u32;
    contiguous_mask((!preserve_mask) as i64)?;

    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: inserted,
        right: shift,
    } = right.as_ref()
    else {
        return None;
    };
    let shift = u8::try_from(constant_value(shift)?).ok()?;
    (shift <= 31).then_some(FieldInsert {
        source: field_source(inserted)?,
        preserve_mask,
        shift,
        operation: FieldOperation::ShiftOr,
    })
}

fn fixed_port_store(statement: &Statement) -> Option<(u32, Type, &Expression)> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: 0,
                member_type,
                index_stride: None,
            },
        value,
    } = statement
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::StructPointer { .. },
        operand,
    } = base.as_ref()
    else {
        return None;
    };
    let address = u32::try_from(constant_value(operand)?).ok()?;
    Some((address, *member_type, value))
}

fn is_noop(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) if constant_value(operand) == Some(0)
    )
}

pub(super) fn recognize(function: &Function) -> Option<Packet<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [accumulator] = function.locals.as_slice() else {
        return None;
    };
    if accumulator.declared_type != Type::UnsignedInt
        || accumulator
            .initializer
            .as_ref()
            .is_some_and(|value| constant_value(value) != Some(0))
        || accumulator.array_length.is_some()
        || accumulator.is_static
        || accumulator.is_volatile
    {
        return None;
    }

    // The shared body normalizer has already flattened single-iteration macros.
    let statements = match function.statements.as_slice() {
        [noop, rest @ ..] if is_noop(noop) => rest,
        statements => statements,
    };
    let statements = if accumulator.initializer.is_some() {
        statements
    } else {
        let [Statement::Assign { name, value }, rest @ ..] = statements else {
            return None;
        };
        if name != &accumulator.name || constant_value(value) != Some(0) {
            return None;
        }
        rest
    };
    let [field_statements @ .., command_statement, data_statement, flag_statement] = statements
    else {
        return None;
    };
    let fields = field_statements
        .iter()
        .map(|statement| field_insert(statement, &accumulator.name))
        .collect::<Option<Vec<_>>>()?;

    let (port, Type::UnsignedChar, command_value) = fixed_port_store(command_statement)? else {
        return None;
    };
    let command = i16::try_from(constant_value(command_value)?).ok()?;
    let (data_port, Type::UnsignedInt, data_value) = fixed_port_store(data_statement)? else {
        return None;
    };
    if data_port != port
        || !matches!(word_value(data_value), Expression::Variable(name) if name == &accumulator.name)
    {
        return None;
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
    } = flag_statement
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    if constant_value(value) != Some(0) {
        return None;
    }

    Some(Packet {
        fields,
        command,
        port,
        global,
        flag_offset: i16::try_from(*offset).ok()?,
    })
}
