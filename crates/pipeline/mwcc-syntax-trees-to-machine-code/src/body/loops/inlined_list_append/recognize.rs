//! Semantic recognition for an outer list-construction caller.

use super::*;

pub(super) struct InlinedListConstruction {
    pub(super) helper: String,
    pub(super) registry: String,
    pub(super) padding_helper: String,
    pub(super) size_offset: i16,
    pub(super) count_offset: i16,
    pub(super) head_offset: i16,
    pub(super) next_offset: i16,
    pub(super) alignment_bias: i16,
    pub(super) alignment_bits: u8,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn casted_var(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => casted_var(operand, expected),
        _ => var(expression, expected),
    }
}

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn indirect_member(expression: &Expression, pointer_name: &str) -> Option<(u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    var(base, pointer_name).then_some((*offset, *member_type))
}

pub(super) fn classify(function: &Function) -> Option<InlinedListConstruction> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [list_out, item_size] = function.parameters.as_slice() else {
        return None;
    };
    if list_out.parameter_type != Type::Pointer(Pointee::Pointer)
        || item_size.parameter_type != Type::Int
    {
        return None;
    }
    let [round_size, construct_if, padding] = function.statements.as_slice() else {
        return None;
    };

    let Statement::Assign {
        name: rounded_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: biased_size,
                right: alignment_mask,
            },
    } = round_size
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: size_value,
        right: alignment_bias,
    } = biased_size.as_ref()
    else {
        return None;
    };
    let alignment_bias = i16::try_from(constant_value(alignment_bias)?).ok()?;
    let mask = constant_value(alignment_mask)? as i32 as u32;
    let alignment_bits = mask.trailing_zeros() as u8;
    if rounded_name != &item_size.name
        || !var(size_value, &item_size.name)
        || alignment_bits == 0
        || alignment_bits > 15
        || mask != u32::MAX << alignment_bits
        || alignment_bias != ((1_i16 << alignment_bits) - 1)
    {
        return None;
    }

    let Statement::If {
        condition: Expression::Call {
            name: helper,
            arguments,
        },
        then_body,
        else_body,
    } = construct_if
    else {
        return None;
    };
    let [Expression::AddressOf { operand: registry }, list_argument] = arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(registry) = registry.as_ref() else {
        return None;
    };
    if !casted_var(list_argument, &list_out.name) || !else_body.is_empty() {
        return None;
    }

    let [clear_count, store_size, clear_next, clear_head, success] = then_body.as_slice() else {
        return None;
    };
    let Statement::Store {
        target: count_target,
        value: count_value,
    } = clear_count
    else {
        return None;
    };
    let Statement::Store {
        target: size_target,
        value: stored_size,
    } = store_size
    else {
        return None;
    };
    let Statement::Store {
        target: next_target,
        value: next_value,
    } = clear_next
    else {
        return None;
    };
    let Statement::Store {
        target: head_target,
        value: head_value,
    } = clear_head
    else {
        return None;
    };
    let (count_offset, count_type) = indirect_member(count_target, &list_out.name)?;
    let (size_offset, size_type) = indirect_member(size_target, &list_out.name)?;
    let (next_offset, next_type) = indirect_member(next_target, &list_out.name)?;
    let (head_offset, head_type) = indirect_member(head_target, &list_out.name)?;
    if count_type != Type::Int
        || size_type != Type::Int
        || !matches!(next_type, Type::Pointer(_))
        || !matches!(head_type, Type::Pointer(_))
        || !is_constant(count_value, 0)
        || !var(stored_size, &item_size.name)
        || !is_constant(next_value, 0)
        || !is_constant(head_value, 0)
        || !matches!(
            success,
            Statement::Return(Some(value)) if is_constant(value, 1)
        )
    {
        return None;
    }

    let Statement::Expression(Expression::Call {
        name: padding_helper,
        arguments: padding_arguments,
    }) = padding
    else {
        return None;
    };
    if !padding_arguments.is_empty() {
        return None;
    }

    Some(InlinedListConstruction {
        helper: helper.clone(),
        registry: registry.clone(),
        padding_helper: padding_helper.clone(),
        size_offset: i16::try_from(size_offset).ok()?,
        count_offset: i16::try_from(count_offset).ok()?,
        head_offset: i16::try_from(head_offset).ok()?,
        next_offset: i16::try_from(next_offset).ok()?,
        alignment_bias,
        alignment_bits,
    })
}
