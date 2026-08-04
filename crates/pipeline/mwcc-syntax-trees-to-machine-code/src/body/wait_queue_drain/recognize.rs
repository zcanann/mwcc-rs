use super::*;

pub(super) fn classify(function: &Function) -> Option<WaitQueueDrain<'_>> {
    let [selector] = function.parameters.as_slice() else {
        return None;
    };
    let [object, allocated, padding] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Void
        || selector.parameter_type != Type::UnsignedChar
        || !matches!(object.declared_type, Type::StructPointer { .. })
        || object.initializer.is_some()
        || !matches!(allocated.declared_type, Type::StructPointer { .. })
        || allocated.initializer.is_some()
        || padding.declared_type != Type::Int
        || padding.array_length != Some(2)
        || padding.initializer.is_some()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(loop_condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let count = nonzero_variable(loop_condition)?;
    let [Statement::Assign { name: object_name, value: lookup }, Statement::If {
        condition: object_condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if object_name != &object.name || variable(object_condition) != Some(object.name.as_str()) {
        return None;
    }
    let Expression::Index { base: table_expression, index: index_expression } = lookup else {
        return None;
    };
    let table = variable(table_expression)?;
    let index = variable(index_expression)?;

    let [Statement::Assign { name: allocated_name, value: allocation }, allocation_break,
        result_store, play_statement, cut_statement, true_advance @ .., stop_after_one] =
        then_body.as_slice()
    else {
        return None;
    };
    if allocated_name != &allocated.name {
        return None;
    }
    let Expression::Call { name: allocate, arguments: allocation_arguments } = allocation else {
        return None;
    };
    let [first_argument, second_argument] = allocation_arguments.as_slice() else {
        return None;
    };
    if constant_value(first_argument) != Some(0)
        || variable(second_argument) != Some(object.name.as_str())
        || !breaks_when_equal_to_zero(allocation_break, &allocated.name)
    {
        return None;
    }

    let (result_target, result_value) = store(result_store)?;
    let (result_base, object_result_offset) = member(result_target)?;
    if variable(result_base) != Some(object.name.as_str())
        || variable(result_value) != Some(allocated.name.as_str())
    {
        return None;
    }
    let (play, play_arguments) = direct_call_statement(play_statement)?;
    if !single_variable_argument(play_arguments, &object.name) {
        return None;
    }

    let Statement::If {
        condition: cut_condition,
        then_body: append_body,
        else_body: cut_else,
    } = cut_statement
    else {
        return None;
    };
    if !cut_else.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left: cut_call,
        right: cut_failure,
    } = cut_condition
    else {
        return None;
    };
    let Expression::Call { name: cut, arguments: cut_arguments } = cut_call.as_ref() else {
        return None;
    };
    if !single_variable_argument(cut_arguments, &object.name)
        || constant_value(cut_failure) != Some(-1)
    {
        return None;
    }
    let [append_statement] = append_body.as_slice() else {
        return None;
    };
    let (append, append_arguments) = direct_call_statement(append_statement)?;
    let [Expression::AddressOf { operand: append_list }, appended_object] = append_arguments else {
        return None;
    };
    if variable(appended_object) != Some(object.name.as_str()) {
        return None;
    }
    let (manager, manager_list_offset) = member(append_list)?;
    let (manager_base, object_manager_offset) = member(manager)?;
    if variable(manager_base) != Some(object.name.as_str()) {
        return None;
    }

    if true_advance.len() != 3 {
        return None;
    }
    let true_queue = queue_advance(true_advance)?;
    let false_queue = queue_advance(else_body)?;
    if true_queue.index != index
        || true_queue.count != count
        || true_queue.index != false_queue.index
        || true_queue.count != false_queue.count
        || true_queue.bound != false_queue.bound
        || !breaks_when_equal_to_one(stop_after_one, &selector.name)
    {
        return None;
    }

    Some(WaitQueueDrain {
        table,
        index,
        count,
        bound: true_queue.bound,
        object_result_offset: i16::try_from(object_result_offset).ok()?,
        object_manager_offset: i16::try_from(object_manager_offset).ok()?,
        manager_list_offset: i16::try_from(manager_list_offset).ok()?,
        allocate,
        play,
        cut,
        append,
    })
}

struct QueueAdvance<'a> {
    index: &'a str,
    count: &'a str,
    bound: u16,
}

fn queue_advance(statements: &[Statement]) -> Option<QueueAdvance<'_>> {
    let [increment, wrap, decrement] = statements else {
        return None;
    };
    let (increment_target, increment_value) = store(increment)?;
    let index = variable(increment_target)?;
    if !binary_variable_constant(increment_value, BinaryOperator::Add, index, 1) {
        return None;
    }
    let Statement::If {
        condition: wrap_condition,
        then_body: wrap_body,
        else_body,
    } = wrap
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: wrapped_index,
        right: bound,
    } = wrap_condition
    else {
        return None;
    };
    let bound = u16::try_from(constant_value(bound)?).ok()?;
    let [wrap_store] = wrap_body.as_slice() else {
        return None;
    };
    let (wrap_target, wrap_value) = store(wrap_store)?;
    if variable(wrapped_index) != Some(index)
        || variable(wrap_target) != Some(index)
        || constant_value(wrap_value) != Some(0)
    {
        return None;
    }
    let (decrement_target, decrement_value) = store(decrement)?;
    let count = variable(decrement_target)?;
    if !binary_variable_constant(decrement_value, BinaryOperator::Subtract, count, 1) {
        return None;
    }
    Some(QueueAdvance { index, count, bound })
}

fn nonzero_variable(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (constant_value(right) == Some(0)).then(|| variable(left)).flatten()
}

fn breaks_when_equal_to_zero(statement: &Statement, name: &str) -> bool {
    breaks_when_equal_to(statement, name, 0)
}

fn breaks_when_equal_to_one(statement: &Statement, name: &str) -> bool {
    breaks_when_equal_to(statement, name, 1)
}

fn breaks_when_equal_to(statement: &Statement, name: &str, constant: i64) -> bool {
    let Statement::If { condition, then_body, else_body } = statement else {
        return false;
    };
    else_body.is_empty()
        && matches!(then_body.as_slice(), [Statement::Break])
        && binary_variable_constant(condition, BinaryOperator::Equal, name, constant)
}

fn binary_variable_constant(
    expression: &Expression,
    operator: BinaryOperator,
    name: &str,
    constant: i64,
) -> bool {
    matches!(expression, Expression::Binary { operator: candidate, left, right }
        if *candidate == operator
            && variable(left) == Some(name)
            && constant_value(right) == Some(constant))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member { base, offset, index_stride: None, .. } = expression else {
        return None;
    };
    Some((base, *offset))
}

fn store(statement: &Statement) -> Option<(&Expression, &Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    Some((target, value))
}

fn direct_call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn single_variable_argument(arguments: &[Expression], expected: &str) -> bool {
    matches!(arguments, [argument] if variable(argument) == Some(expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_incomplete_queue_body() {
        let function = Function {
            return_type: Type::Void,
            name: "not_a_queue_drain".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![],
            statements: vec![],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(classify(&function).is_none());
    }
}
