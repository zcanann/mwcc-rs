//! Structural recognition for global intrusive-queue removal.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct Shape<'a> {
    pub(super) item: &'a str,
    pub(super) current: &'a str,
    pub(super) head: &'a str,
    pub(super) tail: &'a str,
    pub(super) flags: i16,
    pub(super) state: i16,
    pub(super) next: i16,
    pub(super) previous: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn no_op(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Cast {
        target_type: Type::Void,
        operand,
    }) if constant_value(operand) == Some(0))
}

fn member(expression: &Expression) -> Option<(&Expression, u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    Some((base, *offset, *member_type))
}

fn direct_member(expression: &Expression, item: &str) -> Option<(u32, Type)> {
    let (base, offset, member_type) = member(expression)?;
    (variable(base) == Some(item)).then_some((offset, member_type))
}

fn nested_member(expression: &Expression, item: &str) -> Option<(u32, Type, u32, Type)> {
    let (base, outer_offset, outer_type) = member(expression)?;
    let (inner_offset, inner_type) = direct_member(base, item)?;
    Some((inner_offset, inner_type, outer_offset, outer_type))
}

fn global_store(statement: &Statement) -> Option<(&str, &Expression)> {
    let Statement::Store {
        target: Expression::Variable(name),
        value,
    } = statement
    else {
        return None;
    };
    Some((name, value))
}

fn direct_member_store<'a>(
    statement: &'a Statement,
    item: &str,
) -> Option<(u32, Type, &'a Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (offset, member_type) = direct_member(target, item)?;
    Some((offset, member_type, value))
}

fn nested_member_store<'a>(
    statement: &'a Statement,
    item: &str,
) -> Option<(u32, Type, u32, Type, &'a Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (inner_offset, inner_type, outer_offset, outer_type) = nested_member(target, item)?;
    Some((inner_offset, inner_type, outer_offset, outer_type, value))
}

fn equality_global_item<'a>(expression: &'a Expression, item: &str) -> Option<&'a str> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    match (variable(left), variable(right)) {
        (Some(global), Some(candidate)) if candidate == item => Some(global),
        (Some(candidate), Some(global)) if candidate == item => Some(global),
        _ => None,
    }
}

fn nonnull_member(expression: &Expression, item: &str) -> Option<(u32, Type)> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if constant_value(right) == Some(0) {
        direct_member(left, item)
    } else if constant_value(left) == Some(0) {
        direct_member(right, item)
    } else {
        None
    }
}

fn pointer_word(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}

fn scalar_word(value_type: Type) -> bool {
    matches!(value_type, Type::Int | Type::UnsignedInt)
}

pub(super) fn recognize(function: &Function) -> Option<Shape<'_>> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Void
        || !matches!(parameter.parameter_type, Type::StructPointer { .. })
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let item = parameter.name.as_str();
    let statements = match function.statements.first() {
        Some(statement) if no_op(statement) => &function.statements[1..],
        _ => function.statements.as_slice(),
    };
    let [clear_flags, set_state, remove_head, remove_tail, publish_current, repair_previous_next, repair_next_previous] =
        statements
    else {
        return None;
    };

    let (flags, flags_type, flags_value) = direct_member_store(clear_flags, item)?;
    let (state, state_type, state_value) = direct_member_store(set_state, item)?;
    if flags == state
        || !scalar_word(flags_type)
        || !scalar_word(state_type)
        || constant_value(flags_value) != Some(0)
        || constant_value(state_value) != Some(3)
    {
        return None;
    }

    let Statement::If {
        condition: head_condition,
        then_body: head_body,
        else_body: head_else,
    } = remove_head
    else {
        return None;
    };
    let head = equality_global_item(head_condition, item)?;
    let [nonnull_next, clear_all, Statement::Return(None)] = head_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: next_condition,
        then_body: next_body,
        else_body: next_else,
    } = nonnull_next
    else {
        return None;
    };
    let (next, next_type) = nonnull_member(next_condition, item)?;
    let [publish_head, clear_new_head_previous, Statement::Return(None)] = next_body.as_slice()
    else {
        return None;
    };
    let (published_head, published_head_value) = global_store(publish_head)?;
    let (published_next, published_next_type) = direct_member(published_head_value, item)?;
    let (new_head_base, new_head_base_type, previous, previous_type, cleared_previous) =
        nested_member_store(clear_new_head_previous, item)?;
    let (cleared_head, clear_all_value) = global_store(clear_all)?;
    let Expression::Assign {
        target: tail_target,
        value: tail_value,
    } = clear_all_value
    else {
        return None;
    };
    let tail = variable(tail_target)?;
    let Expression::Assign {
        target: current_target,
        value: cleared_all_value,
    } = tail_value.as_ref()
    else {
        return None;
    };
    let current = variable(current_target)?;
    if !head_else.is_empty()
        || !next_else.is_empty()
        || published_head != head
        || published_next != next
        || new_head_base != next
        || constant_value(cleared_previous) != Some(0)
        || cleared_head != head
        || constant_value(cleared_all_value) != Some(0)
        || ![
            next_type,
            published_next_type,
            new_head_base_type,
            previous_type,
        ]
        .into_iter()
        .all(pointer_word)
    {
        return None;
    }

    let Statement::If {
        condition: tail_condition,
        then_body: tail_body,
        else_body: tail_else,
    } = remove_tail
    else {
        return None;
    };
    let tested_tail = equality_global_item(tail_condition, item)?;
    let [publish_previous, clear_new_tail_next, publish_head_current, Statement::Return(None)] =
        tail_body.as_slice()
    else {
        return None;
    };
    let (published_tail, published_tail_value) = global_store(publish_previous)?;
    let (published_previous, published_previous_type) = direct_member(published_tail_value, item)?;
    let (new_tail_base, new_tail_base_type, cleared_next, cleared_next_type, cleared_next_value) =
        nested_member_store(clear_new_tail_next, item)?;
    let (published_current, published_current_value) = global_store(publish_head_current)?;
    if !tail_else.is_empty()
        || tested_tail != tail
        || published_tail != tail
        || published_previous != previous
        || new_tail_base != previous
        || cleared_next != next
        || constant_value(cleared_next_value) != Some(0)
        || published_current != current
        || variable(published_current_value) != Some(head)
        || ![
            published_previous_type,
            new_tail_base_type,
            cleared_next_type,
        ]
        .into_iter()
        .all(pointer_word)
    {
        return None;
    }

    let (middle_current, middle_current_value) = global_store(publish_current)?;
    let (middle_next, middle_next_type) = direct_member(middle_current_value, item)?;
    let (previous_base, previous_base_type, previous_next, previous_next_type, next_value) =
        nested_member_store(repair_previous_next, item)?;
    let (next_value_offset, next_value_type) = direct_member(next_value, item)?;
    let (next_base, next_base_type, next_previous, next_previous_type, previous_value) =
        nested_member_store(repair_next_previous, item)?;
    let (previous_value_offset, previous_value_type) = direct_member(previous_value, item)?;
    if middle_current != current
        || middle_next != next
        || previous_base != previous
        || previous_next != next
        || next_value_offset != next
        || next_base != next
        || next_previous != previous
        || previous_value_offset != previous
        || ![
            middle_next_type,
            previous_base_type,
            previous_next_type,
            next_value_type,
            next_base_type,
            next_previous_type,
            previous_value_type,
        ]
        .into_iter()
        .all(pointer_word)
        || [current, head, tail]
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != 3
    {
        return None;
    }

    Some(Shape {
        item,
        current,
        head,
        tail,
        flags: i16::try_from(flags).ok()?,
        state: i16::try_from(state).ok()?,
        next: i16::try_from(next).ok()?,
        previous: i16::try_from(previous).ok()?,
    })
}
