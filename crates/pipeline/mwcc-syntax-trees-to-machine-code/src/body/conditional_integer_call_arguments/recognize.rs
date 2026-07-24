//! Structural recognition for conditional integer wrapper calls.

#[allow(unused_imports)]
use super::super::*;
use mwcc_syntax_trees::ConditionalOrigin;

pub(super) struct ConditionalIntegerCall<'a> {
    pub(super) callee: &'a str,
    pub(super) local_true: i16,
    pub(super) argument_true: i16,
}

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(stripped(expression), Expression::Variable(name) if name == expected)
}

fn zero(expression: &Expression) -> bool {
    constant_value(expression) == Some(0)
}

fn no_op(statement: &Statement) -> bool {
    matches!(statement, Statement::Expression(Expression::Cast {
        target_type: Type::Void,
        operand,
    }) if zero(operand))
}

fn nonzero_select(expression: &Expression, condition_name: &str) -> Option<i16> {
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        origin: ConditionalOrigin::Ternary,
    } = stripped(expression)
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left,
        right,
    } = condition.as_ref()
    else {
        return None;
    };
    if !variable(left, condition_name) || !zero(right) || !zero(when_false) {
        return None;
    }
    let value = constant_value(when_true)?;
    (value != 0).then(|| i16::try_from(value).ok()).flatten()
}

pub(super) fn recognize(function: &Function) -> Option<ConditionalIntegerCall<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [first, second, argument_condition, local_condition, passthrough] =
        function.parameters.as_slice()
    else {
        return None;
    };
    if first.parameter_type != Type::Int
        || second.parameter_type != Type::Int
        || argument_condition.parameter_type != Type::UnsignedChar
        || local_condition.parameter_type != Type::UnsignedChar
        || passthrough.parameter_type != Type::Int
    {
        return None;
    }
    let [local] = function.locals.as_slice() else {
        return None;
    };
    if local.declared_type != Type::Int
        || local.is_volatile
        || local.is_static
        || local.array_length.is_some()
    {
        return None;
    }
    let local_true = nonzero_select(local.initializer.as_ref()?, &local_condition.name)?;
    let [noop, Statement::Expression(Expression::Call {
        name: callee,
        arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    let [call_first, call_second, zero_2, conditional, call_passthrough, local_5, local_6, zero_7, zero_8, zero_9] =
        arguments.as_slice()
    else {
        return None;
    };
    if !no_op(noop)
        || !variable(call_first, &first.name)
        || !variable(call_second, &second.name)
        || !zero(zero_2)
        || !variable(call_passthrough, &passthrough.name)
        || !variable(local_5, &local.name)
        || !variable(local_6, &local.name)
        || !zero(zero_7)
        || !zero(zero_8)
        || !zero(zero_9)
    {
        return None;
    }
    Some(ConditionalIntegerCall {
        callee,
        local_true,
        argument_true: nonzero_select(conditional, &argument_condition.name)?,
    })
}
