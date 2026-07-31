//! O0 source-image homes for a repeated floating-point period normalization.

#[allow(unused_imports)]
use super::*;

/// MWCC retains the source images in this common angle-normalization shape:
/// the first argument crosses both calls in a saved FPR, the other incoming
/// values are spilled, and each call result passes through a distinct recovered
/// `var_fN` home before updating the accumulator.
pub(super) struct StructuredPeriodicFloatNormalization<'a> {
    pub(super) preserved_parameter: &'a str,
    pub(super) frame_parameters: [&'a str; 2],
    pub(super) result_homes: [&'a str; 2],
    result_statement_indices: [usize; 2],
}

impl<'a> StructuredPeriodicFloatNormalization<'a> {
    pub(super) fn plan(function: &'a Function) -> Option<Self> {
        if function.return_type != Type::Float
            || !function.guards.is_empty()
            || function.parameters.len() != 3
            || function
                .parameters
                .iter()
                .any(|parameter| parameter.parameter_type != Type::Float)
            || function.statements.len() != 6
        {
            return None;
        }

        let [preserved, first_frame, second_frame] = function.parameters.as_slice() else {
            return None;
        };
        let accumulator = recovered_local(function, 31)?;
        let first_result = recovered_local(function, 30)?;
        let second_result = recovered_local(function, 29)?;
        if !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &accumulator.name)
        {
            return None;
        }

        let first_call = assigned_call(&function.statements[0], &accumulator.name)?;
        let second_call = assigned_call(&function.statements[3], &accumulator.name)?;
        if first_call.0 != second_call.0
            || !is_period_call(first_call.1)
            || !is_period_call(second_call.1)
            || !is_first_delta(first_call.1, &first_frame.name, &preserved.name)
            || !is_interpolated_delta(
                second_call.1,
                &preserved.name,
                &second_frame.name,
                &accumulator.name,
            )
            || !is_adjustment(
                &function.statements[1],
                &accumulator.name,
                BinaryOperator::Less,
                0.0,
                BinaryOperator::Add,
            )
            || !is_adjustment(
                &function.statements[2],
                &accumulator.name,
                BinaryOperator::Greater,
                180.0,
                BinaryOperator::Subtract,
            )
            || !is_adjustment(
                &function.statements[4],
                &accumulator.name,
                BinaryOperator::Less,
                0.0,
                BinaryOperator::Add,
            )
            || !is_adjustment(
                &function.statements[5],
                &accumulator.name,
                BinaryOperator::GreaterEqual,
                180.0,
                BinaryOperator::Subtract,
            )
        {
            return None;
        }

        Some(Self {
            preserved_parameter: &preserved.name,
            frame_parameters: [&first_frame.name, &second_frame.name],
            result_homes: [&first_result.name, &second_result.name],
            result_statement_indices: [0, 3],
        })
    }

    pub(super) fn owns_frame_parameter(&self, name: &str) -> bool {
        self.frame_parameters.contains(&name)
    }

    pub(super) fn result_home(&self, statement_index: usize) -> Option<&'a str> {
        self.result_statement_indices
            .iter()
            .position(|candidate| *candidate == statement_index)
            .map(|index| self.result_homes[index])
    }
}

impl Generator {
    /// Restore MWCC's source-side operand placement after physical allocation.
    /// The operations are commutative, but their encoded A/B(C) fields retain
    /// the normalized value on the left in this O0 source-image schedule.
    pub(crate) fn schedule_periodic_float_normalization(&mut self, function: &Function) {
        if StructuredPeriodicFloatNormalization::plan(function).is_some() {
            rewrite_commutative_operands(&mut self.output.instructions);
        }
    }
}

fn rewrite_commutative_operands(instructions: &mut [Instruction]) -> bool {
    let additions: Vec<_> = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(
                instruction,
                Instruction::FloatAddSingle { d: 31, a: 0, b: 31 }
            )
            .then_some(index)
        })
        .collect();
    let products: Vec<_> = instructions
        .iter()
        .enumerate()
        .filter_map(|(index, instruction)| {
            matches!(
                instruction,
                Instruction::FloatMultiplySingle { d: 0, a: 31, c: 0 }
            )
            .then_some(index)
        })
        .collect();
    if additions.len() != 2 || products.len() != 1 {
        return false;
    }
    for index in additions {
        instructions[index] = Instruction::FloatAddSingle { d: 31, a: 31, b: 0 };
    }
    instructions[products[0]] = Instruction::FloatMultiplySingle { d: 0, a: 0, c: 31 };
    true
}

fn recovered_local(function: &Function, register: u8) -> Option<&LocalDeclaration> {
    function.locals.iter().find(|local| {
        local.declared_type == Type::Float
            && local.initializer.is_none()
            && local.array_length.is_none()
            && super::structured_recovered_float_homes::register(&local.name) == Some(register)
    })
}

fn assigned_call<'a>(
    statement: &'a Statement,
    target: &str,
) -> Option<(&'a str, &'a [Expression])> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = peel_float_casts(value)
    else {
        return None;
    };
    (name == target).then_some((callee, arguments))
}

fn peel_float_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast {
        target_type: Type::Float | Type::Double,
        operand,
    } = expression
    {
        expression = operand;
    }
    expression
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn float_literal(expression: &Expression, expected: f64) -> bool {
    matches!(expression, Expression::FloatLiteral(value) if *value == expected)
}

fn is_period_call(arguments: &[Expression]) -> bool {
    let [_, period] = arguments else {
        return false;
    };
    float_literal(peel_float_casts(period), 360.0)
}

fn is_first_delta(arguments: &[Expression], left_name: &str, right_name: &str) -> bool {
    let [delta, _] = arguments else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = peel_float_casts(delta)
    else {
        return false;
    };
    variable(left, left_name) && variable(right, right_name)
}

fn is_interpolated_delta(
    arguments: &[Expression],
    base_name: &str,
    complement_name: &str,
    accumulator: &str,
) -> bool {
    let [delta, _] = arguments else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: base,
        right,
    } = peel_float_casts(delta)
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: complement,
        right: delta,
    } = right.as_ref()
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: one,
        right: complement,
    } = complement.as_ref()
    else {
        return false;
    };
    variable(base, base_name)
        && float_literal(one, 1.0)
        && variable(complement, complement_name)
        && variable(delta, accumulator)
}

fn is_adjustment(
    statement: &Statement,
    accumulator: &str,
    comparison: BinaryOperator,
    bound: f64,
    update: BinaryOperator,
) -> bool {
    let Statement::If {
        condition:
            Expression::Binary {
                operator,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let [Statement::Assign {
        name,
        value:
            Expression::Binary {
                operator: update_operator,
                left: update_left,
                right: update_right,
            },
    }] = then_body.as_slice()
    else {
        return false;
    };
    else_body.is_empty()
        && *operator == comparison
        && variable(left, accumulator)
        && float_literal(right, bound)
        && name == accumulator
        && *update_operator == update
        && variable(update_left, accumulator)
        && float_literal(update_right, 360.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_source_side_commutative_operand_fields_only_for_the_full_topology() {
        let mut instructions = vec![
            Instruction::FloatAddSingle { d: 31, a: 0, b: 31 },
            Instruction::FloatMultiplySingle { d: 0, a: 31, c: 0 },
            Instruction::FloatAddSingle { d: 31, a: 0, b: 31 },
        ];

        assert!(rewrite_commutative_operands(&mut instructions));
        assert!(matches!(
            instructions[0],
            Instruction::FloatAddSingle { d: 31, a: 31, b: 0 }
        ));
        assert!(matches!(
            instructions[1],
            Instruction::FloatMultiplySingle { d: 0, a: 0, c: 31 }
        ));
    }
}
