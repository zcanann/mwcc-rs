//! Structural recognition for a three-element quadratic float map.

#[allow(unused_imports)]
use super::super::*;

pub(super) struct QuadraticFloatMap<'a> {
    pub(super) inputs: [&'a str; 3],
    pub(super) output: &'a str,
    pub(super) weight: &'a str,
}

fn variable(expression: &Expression) -> Option<&str> {
    let Expression::Variable(name) = expression else {
        return None;
    };
    Some(name)
}

fn constant(expression: &Expression, expected: f64) -> bool {
    match expression {
        Expression::Cast { operand, .. } => constant(operand, expected),
        Expression::FloatLiteral(value) => *value == expected,
        Expression::IntegerLiteral(value) => *value as f64 == expected,
        _ => false,
    }
}

fn binary<'a>(
    expression: &'a Expression,
    operator: BinaryOperator,
) -> Option<(&'a Expression, &'a Expression)> {
    let Expression::Binary {
        operator: found,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*found == operator).then_some((left, right))
}

fn assignment<'a>(expression: &'a Expression, target: &str) -> Option<&'a Expression> {
    let Expression::Assign {
        target: assigned,
        value,
    } = expression
    else {
        return None;
    };
    (variable(assigned) == Some(target)).then_some(value)
}

fn one_step(expression: &Expression, name: &str) -> bool {
    let Some(value) = assignment(expression, name) else {
        return false;
    };
    matches!(binary(value, BinaryOperator::Add), Some((left, right))
        if variable(left) == Some(name) && constant_value(right) == Some(1))
}

fn flatten_comma<'a>(expression: &'a Expression, output: &mut Vec<&'a Expression>) {
    if let Expression::Comma { left, right } = expression {
        output.push(left);
        flatten_comma(right, output);
    } else {
        output.push(expression);
    }
}

fn post_step_float_read(expression: &Expression, pointer: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer: dereferenced }
        if matches!(dereferenced.as_ref(), Expression::PostStep {
            target,
            operator: BinaryOperator::Add,
            pointer_link: None,
        } if variable(target) == Some(pointer)))
}

fn multiply_pair(expression: &Expression, first: &str, second: &str) -> bool {
    matches!(binary(expression, BinaryOperator::Multiply), Some((left, right))
        if variable(left) == Some(first) && variable(right) == Some(second))
}

fn squared_product(expression: &Expression, factor: &str, squared: &str) -> bool {
    let Some((left, right)) = binary(expression, BinaryOperator::Multiply) else {
        return false;
    };
    variable(left) == Some(factor) && multiply_pair(right, squared, squared)
}

fn doubled_product(expression: &Expression, middle: &str, inverse: &str, weight: &str) -> bool {
    let Some((two, product)) = binary(expression, BinaryOperator::Multiply) else {
        return false;
    };
    let Some((middle_value, factors)) = binary(product, BinaryOperator::Multiply) else {
        return false;
    };
    constant(two, 2.0)
        && variable(middle_value) == Some(middle)
        && multiply_pair(factors, inverse, weight)
}

fn quadratic(
    expression: &Expression,
    left: &str,
    middle: &str,
    right: &str,
    inverse: &str,
    weight: &str,
) -> bool {
    let Some((right_term, remaining)) = binary(expression, BinaryOperator::Add) else {
        return false;
    };
    let Some((left_term, middle_term)) = binary(remaining, BinaryOperator::Add) else {
        return false;
    };
    squared_product(right_term, right, weight)
        && squared_product(left_term, left, inverse)
        && doubled_product(middle_term, middle, inverse, weight)
}

pub(super) fn recognize(function: &Function) -> Option<QuadraticFloatMap<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [first, middle, last, output, weight] = function.parameters.as_slice() else {
        return None;
    };
    if [first, middle, last, output]
        .iter()
        .any(|parameter| parameter.parameter_type != Type::Pointer(Pointee::Float))
        || weight.parameter_type != Type::Float
    {
        return None;
    }
    let [counter, first_value, middle_value, last_value, inverse, result] =
        function.locals.as_slice()
    else {
        return None;
    };
    if counter.declared_type != Type::Int
        || [first_value, middle_value, last_value, inverse, result]
            .iter()
            .any(|local| {
                local.declared_type != Type::Float
                    || local.initializer.is_some()
                    || local.is_static
                    || local.is_volatile
                    || local.array_length.is_some()
            })
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if assignment(initializer, &counter.name).and_then(constant_value) != Some(0)
        || !one_step(step, &counter.name)
        || !matches!(binary(condition, BinaryOperator::Less), Some((left, right))
            if variable(left) == Some(&counter.name) && constant_value(right) == Some(3))
    {
        return None;
    }
    let [Statement::Store {
        target: Expression::Dereference { pointer },
        value,
    }, Statement::Assign {
        name: advanced_output,
        value: output_step,
    }] = body.as_slice()
    else {
        return None;
    };
    if variable(pointer) != Some(&output.name)
        || advanced_output != &output.name
        || !matches!(binary(output_step, BinaryOperator::Add), Some((left, right))
            if variable(left) == Some(&output.name) && constant_value(right) == Some(1))
    {
        return None;
    }

    let mut sequence = Vec::new();
    flatten_comma(value, &mut sequence);
    let [read_first, read_middle, read_last, make_inverse, make_result, returned] =
        sequence.as_slice()
    else {
        return None;
    };
    let first_read = assignment(read_first, &first_value.name)?;
    let middle_read = assignment(read_middle, &middle_value.name)?;
    let last_read = assignment(read_last, &last_value.name)?;
    let inverse_value = assignment(make_inverse, &inverse.name)?;
    let result_value = assignment(make_result, &result.name)?;
    if !post_step_float_read(first_read, &first.name)
        || !post_step_float_read(middle_read, &middle.name)
        || !post_step_float_read(last_read, &last.name)
        || !matches!(binary(inverse_value, BinaryOperator::Subtract), Some((left, right))
            if constant(left, 1.0) && variable(right) == Some(&weight.name))
        || !quadratic(
            result_value,
            &first_value.name,
            &middle_value.name,
            &last_value.name,
            &inverse.name,
            &weight.name,
        )
        || variable(returned) != Some(&result.name)
    {
        return None;
    }

    Some(QuadraticFloatMap {
        inputs: [&first.name, &middle.name, &last.name],
        output: &output.name,
        weight: &weight.name,
    })
}
