//! Structural recognition for a traced global queue append.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct Shape<'a> {
    pub(super) item: &'a str,
    pub(super) current: &'a str,
    pub(super) head: &'a str,
    pub(super) tail: &'a str,
    pub(super) callee: &'a str,
    pub(super) string: &'a [u8],
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

fn null_global(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if constant_value(right) == Some(0) {
        variable(left)
    } else if constant_value(left) == Some(0) {
        variable(right)
    } else {
        None
    }
}

fn pointer_word(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
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
    let [append, clear_state, trace] = statements else {
        return None;
    };

    let Statement::If {
        condition,
        then_body,
        else_body,
    } = append
    else {
        return None;
    };
    let tail = null_global(condition)?;
    let [publish_current, publish_tail, publish_head, clear_links] = then_body.as_slice() else {
        return None;
    };
    let (current, current_value) = global_store(publish_current)?;
    let (then_tail, then_tail_value) = global_store(publish_tail)?;
    let (head, head_value) = global_store(publish_head)?;
    let (next, next_type, clear_previous) = direct_member_store(clear_links, item)?;
    let Expression::Assign {
        target: previous_target,
        value: links_zero,
    } = clear_previous
    else {
        return None;
    };
    let (previous, previous_type) = direct_member(previous_target, item)?;

    let [repair_tail_next, clear_next, save_previous, republish_tail] = else_body.as_slice() else {
        return None;
    };
    let Statement::Store {
        target: tail_next_target,
        value: tail_next_value,
    } = repair_tail_next
    else {
        return None;
    };
    let (tail_base, repaired_next, repaired_next_type) = member(tail_next_target)?;
    let (cleared_next, cleared_next_type, cleared_next_value) =
        direct_member_store(clear_next, item)?;
    let (saved_previous, saved_previous_type, saved_previous_value) =
        direct_member_store(save_previous, item)?;
    let (published_tail, published_tail_value) = global_store(republish_tail)?;
    if then_tail != tail
        || variable(current_value) != Some(item)
        || variable(then_tail_value) != Some(item)
        || variable(head_value) != Some(item)
        || next == previous
        || constant_value(links_zero) != Some(0)
        || variable(tail_base) != Some(tail)
        || repaired_next != next
        || variable(tail_next_value) != Some(item)
        || cleared_next != next
        || constant_value(cleared_next_value) != Some(0)
        || saved_previous != previous
        || variable(saved_previous_value) != Some(tail)
        || published_tail != tail
        || variable(published_tail_value) != Some(item)
        || ![
            next_type,
            previous_type,
            repaired_next_type,
            cleared_next_type,
            saved_previous_type,
        ]
        .into_iter()
        .all(pointer_word)
    {
        return None;
    }

    let (state, state_type, state_value) = direct_member_store(clear_state, item)?;
    let Statement::Expression(Expression::Call { name, arguments }) = trace else {
        return None;
    };
    let [Expression::StringLiteral(string), Expression::Cast {
        target_type: Type::UnsignedInt,
        operand: traced_item,
    }] = arguments.as_slice()
    else {
        return None;
    };
    if !matches!(state_type, Type::Int | Type::UnsignedInt)
        || constant_value(state_value) != Some(0)
        || variable(traced_item) != Some(item)
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
        callee: name,
        string,
        state: i16::try_from(state).ok()?,
        next: i16::try_from(next).ok()?,
        previous: i16::try_from(previous).ok()?,
    })
}
