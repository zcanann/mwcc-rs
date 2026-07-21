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

fn byte_pointer(value: Type) -> bool {
    matches!(value, Type::Pointer(Pointee::Char | Pointee::UnsignedChar))
}

fn plain_flag(local: &LocalDeclaration) -> bool {
    local.declared_type == Type::Char
        && local.initializer.is_none()
        && !local.is_const
        && !local.is_static
        && !local.is_volatile
        && local.array_length.is_none()
        && local.data_bytes.is_none()
        && local.data_relocations.is_empty()
        && local.row_bytes.is_none()
}

fn dereferences(expression: &Expression, pointer: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer: value }
        if variable(value) == Some(pointer))
}

fn pointer_step(
    expression: &Expression,
    pointer: &str,
    operator: BinaryOperator,
    amount: i64,
) -> bool {
    binary(expression, operator)
        .is_some_and(|(left, right)| variable(left) == Some(pointer) && literal(right, amount))
}

fn assign_literal(statement: &Statement, name: &str, value: i64) -> bool {
    matches!(statement, Statement::Assign { name: assigned, value: expression }
        if assigned == name && literal(expression, value))
}

fn assign_pointer_step(
    statement: &Statement,
    pointer: &str,
    operator: BinaryOperator,
    amount: i64,
) -> bool {
    matches!(statement, Statement::Assign { name, value }
        if name == pointer && pointer_step(value, pointer, operator, amount))
}

fn nonzero_flag(expression: &Expression, flag: &str) -> bool {
    binary(expression, BinaryOperator::NotEqual)
        .is_some_and(|(left, right)| variable(left) == Some(flag) && literal(right, 0))
}

fn flag_set_if(statement: &Statement, flag: &str, condition: impl Fn(&Expression) -> bool) -> bool {
    let Statement::If {
        condition: actual,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    condition(actual)
        && else_body.is_empty()
        && matches!(then_body.as_slice(), [set] if assign_literal(set, flag, 1))
}

fn special_first_test(expression: &Expression, pointer: &str) -> bool {
    let Some((not_z, is_a)) = binary(expression, BinaryOperator::LogicalAnd) else {
        return false;
    };
    binary(not_z, BinaryOperator::NotEqual)
        .is_some_and(|(byte, value)| dereferences(byte, pointer) && literal(value, 122))
        && binary(is_a, BinaryOperator::Equal)
            .is_some_and(|(byte, value)| dereferences(byte, pointer) && literal(value, 97))
}

fn ascii_lowercase_test(expression: &Expression, pointer: &str) -> bool {
    let Some((at_least_a, at_most_z)) = binary(expression, BinaryOperator::LogicalAnd) else {
        return false;
    };
    binary(at_least_a, BinaryOperator::GreaterEqual)
        .is_some_and(|(byte, value)| dereferences(byte, pointer) && literal(value, 97))
        && binary(at_most_z, BinaryOperator::LessEqual)
            .is_some_and(|(byte, value)| dereferences(byte, pointer) && literal(value, 122))
}

fn pointer_adjust_if(statement: &Statement, flag: &str, pointer: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    nonzero_flag(condition, flag)
        && else_body.is_empty()
        && matches!(then_body.as_slice(), [adjust]
            if assign_pointer_step(adjust, pointer, BinaryOperator::Subtract, 32))
}

fn both_zero(expression: &Expression, first: &str, second: &str) -> bool {
    let Some((first_zero, second_zero)) = binary(expression, BinaryOperator::LogicalAnd) else {
        return false;
    };
    binary(first_zero, BinaryOperator::Equal)
        .is_some_and(|(byte, zero)| dereferences(byte, first) && literal(zero, 0))
        && binary(second_zero, BinaryOperator::Equal)
            .is_some_and(|(byte, zero)| dereferences(byte, second) && literal(zero, 0))
}

fn loop_tail(statement: &Statement, first: &str, second: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    both_zero(condition, first, second)
        && matches!(then_body.as_slice(), [first_step, second_step]
            if assign_pointer_step(first_step, first, BinaryOperator::Add, 1)
                && assign_pointer_step(second_step, second, BinaryOperator::Add, 1))
        && matches!(else_body.as_slice(), [Statement::Break])
}

fn comparison_return(statement: &Statement, first: &str, second: &str) -> bool {
    let Statement::Return(Some(Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    })) = statement
    else {
        return false;
    };
    let Some((left, right)) = binary(condition, BinaryOperator::Less) else {
        return false;
    };
    let casted_dereference = |expression: &Expression, pointer: &str| {
        matches!(expression, Expression::Cast { target_type: Type::Int, operand }
            if dereferences(operand, pointer))
    };
    casted_dereference(left, first)
        && casted_dereference(right, second)
        && literal(when_true, -1)
        && literal(when_false, 1)
}

fn mismatch_tail(
    statement: &Statement,
    first: &str,
    second: &str,
    first_flag: &str,
    second_flag: &str,
) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let Some((first_byte, second_byte)) = binary(condition, BinaryOperator::NotEqual) else {
        return false;
    };
    let [zero_first, set_first, adjust_first, zero_second, set_second, adjust_second, result] =
        then_body.as_slice()
    else {
        return false;
    };
    else_body.is_empty()
        && dereferences(first_byte, first)
        && dereferences(second_byte, second)
        && assign_literal(zero_first, first_flag, 0)
        && flag_set_if(set_first, first_flag, |value| {
            ascii_lowercase_test(value, first)
        })
        && pointer_adjust_if(adjust_first, first_flag, first)
        && assign_literal(zero_second, second_flag, 0)
        && flag_set_if(set_second, second_flag, |value| {
            ascii_lowercase_test(value, second)
        })
        && pointer_adjust_if(adjust_second, second_flag, second)
        && comparison_return(result, first, second)
}

pub(super) fn recognize(function: &Function) -> Option<AsciiPointerCompare<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function_makes_call(function)
        || function.asm_body.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if literal(value, 0))
    {
        return None;
    }
    let [first, second] = function.parameters.as_slice() else {
        return None;
    };
    let [first_flag, second_flag] = function.locals.as_slice() else {
        return None;
    };
    if !byte_pointer(first.parameter_type)
        || !byte_pointer(second.parameter_type)
        || !plain_flag(first_flag)
        || !plain_flag(second_flag)
    {
        return None;
    }
    let [loop_statement, mismatch] = function.statements.as_slice() else {
        return None;
    };
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(loop_condition),
        step: None,
        body,
    } = loop_statement
    else {
        return None;
    };
    let [zero_first, set_first, adjust_first, zero_second, set_second, adjust_second, tail] =
        body.as_slice()
    else {
        return None;
    };
    if !literal(loop_condition, 1)
        || !assign_literal(zero_first, &first_flag.name, 0)
        || !flag_set_if(set_first, &first_flag.name, |value| {
            special_first_test(value, &first.name)
        })
        || !pointer_adjust_if(adjust_first, &first_flag.name, &first.name)
        || !assign_literal(zero_second, &second_flag.name, 0)
        || !flag_set_if(set_second, &second_flag.name, |value| {
            ascii_lowercase_test(value, &second.name)
        })
        || !pointer_adjust_if(adjust_second, &second_flag.name, &second.name)
        || !loop_tail(tail, &first.name, &second.name)
        || !mismatch_tail(
            mismatch,
            &first.name,
            &second.name,
            &first_flag.name,
            &second_flag.name,
        )
    {
        return None;
    }
    Some(AsciiPointerCompare {
        first: &first.name,
        second: &second.name,
    })
}
