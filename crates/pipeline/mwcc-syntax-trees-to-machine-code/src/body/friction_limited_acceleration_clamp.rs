//! Acceleration clamping with a zero-target friction fallback.
//!
//! The inlined fallback and the two sign-directed limit ladders share all four
//! floating parameters, the object base, and one zero.  Lowering the nested
//! source `if`s independently reloads those values and can overwrite the object
//! base.  This owner recognizes the complete semantic transaction before
//! emitting its measured register schedule.

#[allow(unused_imports)]
use super::*;

struct Shape<'a> {
    base: &'a str,
    velocity: &'a str,
    acceleration: &'a str,
    target: &'a str,
    friction: &'a str,
    current_velocity_offset: i16,
    horizontal_limit_offset: i16,
    output_offset: i16,
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(value) if value == name)
}

fn binary(
    expression: &Expression,
    operator: BinaryOperator,
) -> Option<(&Expression, &Expression)> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*actual == operator).then_some((left, right))
}

fn logical_not(expression: &Expression) -> Option<&Expression> {
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = expression
    else {
        return None;
    };
    Some(operand)
}

fn negated_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Unary { operator: UnaryOperator::Negate, operand }
        if variable(operand, name))
}

fn float_member(expression: &Expression, base: &str) -> Option<i16> {
    let Expression::Member {
        base: member_base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    variable(member_base, base)
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn negated_member(expression: &Expression, base: &str, offset: i16) -> bool {
    matches!(expression, Expression::Unary { operator: UnaryOperator::Negate, operand }
        if float_member(operand, base) == Some(offset))
}

fn absolute_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Conditional { condition, when_true, when_false, .. }
        if matches!(binary(condition, BinaryOperator::Less), Some((left, right))
            if variable(left, name) && is_zero_literal(right))
        && negated_variable(when_true, name)
        && variable(when_false, name))
}

fn absolute_member(expression: &Expression, base: &str, offset: i16) -> bool {
    matches!(expression, Expression::Conditional { condition, when_true, when_false, .. }
        if matches!(binary(condition, BinaryOperator::Less), Some((left, right))
            if float_member(left, base) == Some(offset) && is_zero_literal(right))
        && negated_member(when_true, base, offset)
        && float_member(when_false, base) == Some(offset))
}

fn assignment(statement: &Statement, name: &str, value: &Expression) -> bool {
    matches!(statement, Statement::Assign { name: assigned, value: assigned_value }
        if assigned == name && same_operand(assigned_value, value))
}

fn sum_of_variables(expression: &Expression, left_name: &str, right_name: &str) -> bool {
    matches!(binary(expression, BinaryOperator::Add), Some((left, right))
        if variable(left, left_name) && variable(right, right_name))
}

fn target_minus_velocity(expression: &Expression, target: &str, velocity: &str) -> bool {
    matches!(binary(expression, BinaryOperator::Subtract), Some((left, right))
        if variable(left, target) && variable(right, velocity))
}

fn member_minus_velocity(
    expression: &Expression,
    base: &str,
    member_offset: i16,
    velocity: &str,
    negate_member: bool,
) -> bool {
    let Some((left, right)) = binary(expression, BinaryOperator::Subtract) else {
        return false;
    };
    variable(right, velocity)
        && if negate_member {
            negated_member(left, base, member_offset)
        } else {
            float_member(left, base) == Some(member_offset)
        }
}

fn assignment_if(
    statement: &Statement,
    comparison: BinaryOperator,
    velocity: &str,
    acceleration: &str,
    compared: impl FnOnce(&Expression) -> bool,
    assigned: impl FnOnce(&Expression) -> bool,
) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let Some((sum, right)) = binary(condition, comparison) else {
        return false;
    };
    let [Statement::Assign { name, value }] = then_body.as_slice() else {
        return false;
    };
    else_body.is_empty()
        && sum_of_variables(sum, velocity, acceleration)
        && compared(right)
        && name == acceleration
        && assigned(value)
}

fn friction_fallback(
    statement: &Statement,
    base: &str,
    target: &str,
    friction: &str,
    temporary: &str,
) -> Option<(i16, i16)> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if !variable(logical_not(condition)?, target) || !else_body.is_empty() {
        return None;
    }
    let [initial, select, store, Statement::Return(None)] = then_body.as_slice() else {
        return None;
    };
    let friction_value = Expression::Variable(friction.into());
    if !assignment(initial, temporary, &friction_value) {
        return None;
    }
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = select
    else {
        return None;
    };
    let Some((left, right)) = binary(condition, BinaryOperator::GreaterEqual) else {
        return None;
    };
    let current_velocity_offset = match right {
        Expression::Conditional {
            when_false,
            ..
        } => float_member(when_false, base)?,
        _ => return None,
    };
    if !absolute_variable(left, temporary)
        || !absolute_member(right, base, current_velocity_offset)
        || !matches!(then_body.as_slice(), [Statement::Assign { name, value }]
            if name == temporary && negated_member(value, base, current_velocity_offset))
        || !matches!(else_body.as_slice(), [Statement::If { condition, then_body, else_body }]
            if else_body.is_empty()
                && matches!(binary(condition, BinaryOperator::Greater), Some((left, right))
                    if float_member(left, base) == Some(current_velocity_offset)
                        && is_zero_literal(right))
                && matches!(then_body.as_slice(), [Statement::Assign { name, value }]
                    if name == temporary && negated_variable(value, temporary)))
    {
        return None;
    }
    let Statement::Store { target, value } = store else {
        return None;
    };
    variable(value, temporary).then_some((
        current_velocity_offset,
        float_member(target, base)?,
    ))
}

fn acceleration_ladder(
    statement: &Statement,
    shape: &Shape<'_>,
) -> Option<()> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let product_test = logical_not(condition)?;
    let (product, zero) = binary(product_test, BinaryOperator::Less)?;
    let (velocity, acceleration) = binary(product, BinaryOperator::Multiply)?;
    if !variable(velocity, shape.velocity)
        || !variable(acceleration, shape.acceleration)
        || !is_zero_literal(zero)
        || !else_body.is_empty()
    {
        return None;
    }
    let [Statement::If {
        condition: sign,
        then_body: positive,
        else_body: negative,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let (signed_acceleration, sign_zero) = binary(sign, BinaryOperator::Greater)?;
    if !variable(signed_acceleration, shape.acceleration) || !is_zero_literal(sign_zero) {
        return None;
    }

    let [Statement::If {
        condition: positive_guard,
        then_body: positive_body,
        else_body: positive_else,
    }] = positive.as_slice()
    else {
        return None;
    };
    let (positive_sum, positive_target) = binary(positive_guard, BinaryOperator::Greater)?;
    let [set_negative_friction, clamp_target, clamp_high] = positive_body.as_slice() else {
        return None;
    };
    if !positive_else.is_empty()
        || !sum_of_variables(positive_sum, shape.velocity, shape.acceleration)
        || !variable(positive_target, shape.target)
        || !matches!(set_negative_friction, Statement::Assign { name, value }
            if name == shape.acceleration && negated_variable(value, shape.friction))
        || !assignment_if(
            clamp_target,
            BinaryOperator::Less,
            shape.velocity,
            shape.acceleration,
            |right| variable(right, shape.target),
            |value| target_minus_velocity(value, shape.target, shape.velocity),
        )
        || !assignment_if(
            clamp_high,
            BinaryOperator::Greater,
            shape.velocity,
            shape.acceleration,
            |right| float_member(right, shape.base) == Some(shape.horizontal_limit_offset),
            |value| member_minus_velocity(
                value,
                shape.base,
                shape.horizontal_limit_offset,
                shape.velocity,
                false,
            ),
        )
    {
        return None;
    }

    let [Statement::If {
        condition: negative_guard,
        then_body: negative_body,
        else_body: negative_else,
    }] = negative.as_slice()
    else {
        return None;
    };
    let (negative_sum, negative_target) = binary(negative_guard, BinaryOperator::Less)?;
    let [set_friction, clamp_target, clamp_low] = negative_body.as_slice() else {
        return None;
    };
    if !negative_else.is_empty()
        || !sum_of_variables(negative_sum, shape.velocity, shape.acceleration)
        || !variable(negative_target, shape.target)
        || !matches!(set_friction, Statement::Assign { name, value }
            if name == shape.acceleration && variable(value, shape.friction))
        || !assignment_if(
            clamp_target,
            BinaryOperator::Greater,
            shape.velocity,
            shape.acceleration,
            |right| variable(right, shape.target),
            |value| target_minus_velocity(value, shape.target, shape.velocity),
        )
        || !assignment_if(
            clamp_low,
            BinaryOperator::Less,
            shape.velocity,
            shape.acceleration,
            |right| negated_member(
                right,
                shape.base,
                shape.horizontal_limit_offset,
            ),
            |value| member_minus_velocity(
                value,
                shape.base,
                shape.horizontal_limit_offset,
                shape.velocity,
                true,
            ),
        )
    {
        return None;
    }
    Some(())
}

fn classify(function: &Function) -> Option<Shape<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [base, velocity, acceleration, target, friction] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || [velocity, acceleration, target, friction]
            .iter()
            .any(|parameter| parameter.parameter_type != Type::Float)
    {
        return None;
    }
    let [temporary] = function.locals.as_slice() else {
        return None;
    };
    if temporary.declared_type != Type::Float || temporary.initializer.is_some() {
        return None;
    }
    let [fallback, ladder, Statement::Store { target: output, value: stored }] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !variable(stored, &acceleration.name) {
        return None;
    }
    let output_offset = float_member(output, &base.name)?;
    let (current_velocity_offset, fallback_output) = friction_fallback(
        fallback,
        &base.name,
        &target.name,
        &friction.name,
        &temporary.name,
    )?;
    if fallback_output != output_offset {
        return None;
    }

    let horizontal_limit_offset = match ladder {
        Statement::If { then_body, .. } => then_body
            .iter()
            .find_map(|statement| match statement {
                Statement::If { then_body, .. } => then_body.iter().find_map(|statement| {
                    let Statement::If { then_body, .. } = statement else {
                        return None;
                    };
                    then_body.iter().find_map(|statement| {
                        let Statement::If { condition, .. } = statement else {
                            return None;
                        };
                        let (_, right) = binary(condition, BinaryOperator::Greater)?;
                        float_member(right, &base.name)
                    })
                }),
                _ => None,
            })?,
        _ => return None,
    };
    let shape = Shape {
        base: &base.name,
        velocity: &velocity.name,
        acceleration: &acceleration.name,
        target: &target.name,
        friction: &friction.name,
        current_velocity_offset,
        horizontal_limit_offset,
        output_offset,
    };
    acceleration_ladder(ladder, &shape)?;
    Some(shape)
}

impl Generator {
    pub(crate) fn try_friction_limited_acceleration_clamp(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.base)?;
        let velocity = self.float_register_of(shape.velocity)?;
        let acceleration = self.float_register_of(shape.acceleration)?;
        let target = self.float_register_of(shape.target)?;
        let friction = self.float_register_of(shape.friction)?;
        if (base, velocity, acceleration, target, friction) != (3, 1, 2, 3, 4) {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output.has_float_branch = true;

        let nonzero_target = self.fresh_label();
        let fallback_velocity_nonnegative = self.fresh_label();
        let fallback_velocity_absolute = self.fresh_label();
        let fallback_friction_nonnegative = self.fresh_label();
        let fallback_friction_absolute = self.fresh_label();
        let fallback_adjust_sign = self.fresh_label();
        let fallback_done = self.fresh_label();

        self.load_float_constant(5, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: target, b: 5 });
        self.emit_branch_conditional_to(4, 2, nonzero_target); // bne
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: base,
            offset: shape.current_velocity_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 2, b: 5 });
        self.emit_branch_conditional_to(4, 0, fallback_velocity_nonnegative); // bge
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 2 });
        self.emit_branch_to(fallback_velocity_absolute);
        self.bind_label(fallback_velocity_nonnegative);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 2 });
        self.bind_label(fallback_velocity_absolute);

        self.load_float_constant(0, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: friction, b: 0 });
        self.emit_branch_conditional_to(4, 0, fallback_friction_nonnegative); // bge
        self.output.instructions.push(Instruction::FloatNegate {
            d: 0,
            b: friction,
        });
        self.emit_branch_to(fallback_friction_absolute);
        self.bind_label(fallback_friction_nonnegative);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 0, b: friction });
        self.bind_label(fallback_friction_absolute);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, fallback_adjust_sign); // bne
        self.output.instructions.push(Instruction::FloatNegate {
            d: friction,
            b: 2,
        });
        self.emit_branch_to(fallback_done);
        self.bind_label(fallback_adjust_sign);
        self.load_float_constant(0, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 2, b: 0 });
        self.emit_branch_conditional_to(4, 1, fallback_done); // ble
        self.output.instructions.push(Instruction::FloatNegate {
            d: friction,
            b: friction,
        });
        self.bind_label(fallback_done);
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: friction,
            a: base,
            offset: shape.output_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.bind_label(nonzero_target);
        let negative = self.fresh_label();
        let positive_second_clamp = self.fresh_label();
        let negative_bound = self.fresh_label();
        let store = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: 0,
                a: velocity,
                c: acceleration,
            });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 5 });
        self.emit_branch_conditional_to(12, 0, store); // blt
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: acceleration,
                b: 5,
            });
        self.emit_branch_conditional_to(4, 1, negative); // ble

        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: velocity,
            b: acceleration,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: target });
        self.emit_branch_conditional_to(4, 1, store); // ble
        self.output.instructions.push(Instruction::FloatNegate {
            d: acceleration,
            b: friction,
        });
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: velocity,
            b: acceleration,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: target });
        self.emit_branch_conditional_to(4, 0, positive_second_clamp); // bge
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: acceleration,
                a: target,
                b: velocity,
            });
        self.bind_label(positive_second_clamp);
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: velocity,
            b: acceleration,
        });
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: target,
            a: base,
            offset: shape.horizontal_limit_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: target });
        self.emit_branch_conditional_to(4, 1, store); // ble
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: acceleration,
                a: target,
                b: velocity,
            });
        self.emit_branch_to(store);

        self.bind_label(negative);
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: velocity,
            b: acceleration,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: target });
        self.emit_branch_conditional_to(4, 0, store); // bge
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: velocity,
            b: friction,
        });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: acceleration, b: friction });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: target });
        self.emit_branch_conditional_to(4, 1, negative_bound); // ble
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: acceleration,
                a: target,
                b: velocity,
            });
        self.bind_label(negative_bound);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: base,
            offset: shape.horizontal_limit_offset,
        });
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: target,
            a: velocity,
            b: acceleration,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 0, b: 0 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: target, b: 0 });
        self.emit_branch_conditional_to(4, 0, store); // bge
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: acceleration,
                a: 0,
                b: velocity,
            });

        self.bind_label(store);
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: acceleration,
            a: base,
            offset: shape.output_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
