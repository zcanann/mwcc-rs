use super::*;

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn literal(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn binary<'a>(
    expression: &'a Expression,
    expected: BinaryOperator,
) -> Option<(&'a Expression, &'a Expression)> {
    match expression {
        Expression::Binary {
            operator,
            left,
            right,
        } if *operator == expected => Some((left, right)),
        _ => None,
    }
}

fn is_byte_pointer(value: Type) -> bool {
    matches!(value, Type::Pointer(Pointee::Char | Pointee::UnsignedChar))
}

fn is_plain_local(local: &LocalDeclaration, declared_type: Type) -> bool {
    local.declared_type == declared_type
        && local.initializer.is_none()
        && !local.is_const
        && !local.is_static
        && !local.is_volatile
        && local.array_length.is_none()
        && local.data_bytes.is_none()
        && local.data_relocations.is_empty()
        && local.row_bytes.is_none()
}

fn is_plain_byte_array(local: &LocalDeclaration, length: u16) -> bool {
    matches!(local.declared_type, Type::Char | Type::UnsignedChar)
        && local.initializer.is_none()
        && !local.is_const
        && !local.is_static
        && !local.is_volatile
        && local.array_length == Some(length)
        && local.data_bytes.is_none()
        && local.data_relocations.is_empty()
        && local.row_bytes.is_none()
}

fn dereferences(expression: &Expression, pointer: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer: value }
        if variable(value) == Some(pointer))
}

fn casted_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Cast { operand, .. }
        if variable(operand) == Some(name))
}

fn casted_dereference(expression: &Expression, pointer: &str) -> bool {
    matches!(expression, Expression::Cast { operand, .. }
        if dereferences(operand, pointer))
}

fn add_one(expression: &Expression, name: &str) -> bool {
    binary(expression, BinaryOperator::Add)
        .is_some_and(|(left, right)| variable(left) == Some(name) && literal(right, 1))
}

fn assignment_is(
    expression: &Expression,
    target: &str,
    value: impl Fn(&Expression) -> bool,
) -> bool {
    matches!(expression, Expression::Assign { target: assigned, value: assigned_value }
        if variable(assigned) == Some(target) && value(assigned_value))
}

fn map_index<'a>(expression: &'a Expression, map: &str) -> Option<&'a Expression> {
    match expression {
        Expression::Index { base, index } if variable(base) == Some(map) => Some(index),
        _ => None,
    }
}

fn byte_bucket(expression: &Expression, byte_pointer: &str, masked: bool) -> bool {
    let shifted = if masked {
        let Some((shifted, mask)) = binary(expression, BinaryOperator::BitAnd) else {
            return false;
        };
        if !literal(mask, 31) {
            return false;
        }
        shifted
    } else {
        expression
    };
    binary(shifted, BinaryOperator::ShiftRight)
        .is_some_and(|(byte, amount)| dereferences(byte, byte_pointer) && literal(amount, 3))
}

fn bit_mask(expression: &Expression, byte_pointer: &str) -> bool {
    let Some((one, amount)) = binary(expression, BinaryOperator::ShiftLeft) else {
        return false;
    };
    let Some((byte, mask)) = binary(amount, BinaryOperator::BitAnd) else {
        return false;
    };
    literal(one, 1) && dereferences(byte, byte_pointer) && literal(mask, 7)
}

fn map_access(expression: &Expression, map: &str, byte_pointer: &str, masked: bool) -> bool {
    map_index(expression, map).is_some_and(|index| byte_bucket(index, byte_pointer, masked))
}

fn map_test(expression: &Expression, map: &str, byte_pointer: &str) -> bool {
    binary(expression, BinaryOperator::BitAnd).is_some_and(|(byte, mask)| {
        map_access(byte, map, byte_pointer, true) && bit_mask(mask, byte_pointer)
    })
}

fn map_update(statement: &Statement, map: &str, byte_pointer: &str) -> bool {
    let Statement::Store { target, value } = statement else {
        return false;
    };
    if !map_access(target, map, byte_pointer, false) {
        return false;
    }
    let Expression::IndexedUpdateValue { value } = value else {
        return false;
    };
    binary(value, BinaryOperator::BitOr).is_some_and(|(old, mask)| {
        map_access(old, map, byte_pointer, false) && bit_mask(mask, byte_pointer)
    })
}

fn nonzero_dereference(expression: &Expression, pointer: &str) -> bool {
    binary(expression, BinaryOperator::NotEqual)
        .is_some_and(|(value, zero)| dereferences(value, pointer) && literal(zero, 0))
}

fn increment_statement(statement: &Statement, name: &str) -> bool {
    matches!(statement, Statement::Assign { name: assigned, value }
        if assigned == name && add_one(value, name))
}

fn zero_map_loop(statement: &Statement, map: &str, index: &str) -> bool {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return false;
    };
    let [Statement::Store { target, value }] = body.as_slice() else {
        return false;
    };
    assignment_is(initializer, index, |value| literal(value, 0))
        && binary(condition, BinaryOperator::Less)
            .is_some_and(|(left, right)| variable(left) == Some(index) && literal(right, 32))
        && assignment_is(step, index, |value| add_one(value, index))
        && map_index(target, map).is_some_and(|value| variable(value) == Some(index))
        && literal(value, 0)
}

fn control_map_loop(statement: &Statement, map: &str, control: &str) -> bool {
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    } = statement
    else {
        return false;
    };
    let [update] = body.as_slice() else {
        return false;
    };
    let Some((value, zero)) = binary(condition, BinaryOperator::NotEqual) else {
        return false;
    };
    let Expression::Dereference { pointer } = value else {
        return false;
    };
    matches!(pointer.as_ref(), Expression::PostStep {
        target,
        operator: BinaryOperator::Add,
    } if variable(target) == Some(control))
        && literal(zero, 0)
        && map_update(update, map, control)
}

fn select_start(expression: &Expression, string: &str, next_token: &str) -> bool {
    matches!(expression, Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } if variable(condition) == Some(string)
        && casted_variable(when_true, string)
        && casted_dereference(when_false, next_token))
}

fn skip_loop(statement: &Statement, map: &str, cursor: &str) -> bool {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    } = statement
    else {
        return false;
    };
    let Some((class_test, terminator_test)) = binary(condition, BinaryOperator::LogicalAnd) else {
        return false;
    };
    matches!(body.as_slice(), [increment] if increment_statement(increment, cursor))
        && map_test(class_test, map, cursor)
        && nonzero_dereference(terminator_test, cursor)
}

fn delimiter_if(statement: &Statement, map: &str, cursor: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let [Statement::Store { target, value }, increment, Statement::Break] = then_body.as_slice()
    else {
        return false;
    };
    else_body.is_empty()
        && map_test(condition, map, cursor)
        && dereferences(target, cursor)
        && literal(value, 0)
        && increment_statement(increment, cursor)
}

fn scan_loop(statement: &Statement, map: &str, cursor: &str) -> bool {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    } = statement
    else {
        return false;
    };
    matches!(body.as_slice(), [delimiter, increment]
        if delimiter_if(delimiter, map, cursor) && increment_statement(increment, cursor))
        && nonzero_dereference(condition, cursor)
}

fn continuation_store(statement: &Statement, next_token: &str, cursor: &str) -> bool {
    matches!(statement, Statement::Store { target, value }
        if dereferences(target, next_token) && casted_variable(value, cursor))
}

fn empty_result_if(statement: &Statement, string: &str, cursor: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let Some((left, right)) = binary(condition, BinaryOperator::Equal) else {
        return false;
    };
    else_body.is_empty()
        && variable(left) == Some(string)
        && casted_variable(right, cursor)
        && matches!(then_body.as_slice(), [Statement::Assign { name, value }]
            if name == string && literal(value, 0))
}

pub(super) fn recognize(function: &Function) -> Option<TokenizerPlan<'_>> {
    if !is_byte_pointer(function.return_type)
        || !function.guards.is_empty()
        || function_makes_call(function)
        || function.asm_body.is_some()
    {
        return None;
    }
    let [string, control, next_token] = function.parameters.as_slice() else {
        return None;
    };
    if !is_byte_pointer(string.parameter_type)
        || !is_byte_pointer(control.parameter_type)
        || next_token.parameter_type != Type::Pointer(Pointee::Pointer)
    {
        return None;
    }
    let [cursor, control_cursor, map, unused_count, index] = function.locals.as_slice() else {
        return None;
    };
    if !is_plain_local(cursor, Type::Pointer(Pointee::UnsignedChar))
        || !is_plain_local(control_cursor, Type::Pointer(Pointee::UnsignedChar))
        || !is_plain_byte_array(map, 32)
        || !is_plain_local(unused_count, Type::Int)
        || !is_plain_local(index, Type::Int)
        || !matches!(function.return_expression.as_ref(), Some(value)
            if variable(value) == Some(string.name.as_str()))
    {
        return None;
    }
    let [zero_map, assign_control, build_map, choose_cursor, skip, save_start, scan, save_next, empty_result] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !zero_map_loop(zero_map, &map.name, &index.name)
        || !matches!(assign_control, Statement::Assign { name, value }
            if name == &control_cursor.name && casted_variable(value, &control.name))
        || !control_map_loop(build_map, &map.name, &control_cursor.name)
        || !matches!(choose_cursor, Statement::Assign { name, value }
            if name == &cursor.name && select_start(value, &string.name, &next_token.name))
        || !skip_loop(skip, &map.name, &cursor.name)
        || !matches!(save_start, Statement::Assign { name, value }
            if name == &string.name && casted_variable(value, &cursor.name))
        || !scan_loop(scan, &map.name, &cursor.name)
        || !continuation_store(save_next, &next_token.name, &cursor.name)
        || !empty_result_if(empty_result, &string.name, &cursor.name)
    {
        return None;
    }
    Some(TokenizerPlan {
        string: &string.name,
        control: &control.name,
        next_token: &next_token.name,
    })
}
