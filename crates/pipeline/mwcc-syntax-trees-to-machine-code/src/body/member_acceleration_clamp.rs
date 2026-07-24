//! Member acceleration adjusted toward a nonzero target velocity.
//!
//! MWCC keeps one member load and one pooled zero live through the nested sign
//! and overshoot tests. Lowering the source `if` statements independently
//! otherwise reloads both values and loses the compact shared store tail.

#[allow(unused_imports)]
use super::*;

struct Member<'a> {
    base: &'a str,
    offset: i16,
}

struct MemberAccelerationClamp<'a> {
    input: Member<'a>,
    output_offset: i16,
    acceleration: &'a str,
    target: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn float_member(expression: &Expression) -> Option<Member<'_>> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some(Member {
        base,
        offset: i16::try_from(*offset).ok()?,
    })
}

fn same_member(expression: &Expression, expected: &Member<'_>) -> bool {
    float_member(expression)
        .is_some_and(|member| member.base == expected.base && member.offset == expected.offset)
}

fn binary<'a>(
    expression: &'a Expression,
    expected: BinaryOperator,
) -> Option<(&'a Expression, &'a Expression)> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*operator == expected).then_some((left, right))
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

fn acceleration_assignment(
    statement: &Statement,
    acceleration: &str,
    target: &str,
    input: &Member<'_>,
) -> bool {
    let Statement::Assign { name, value } = statement else {
        return false;
    };
    let Some((left, right)) = binary(value, BinaryOperator::Subtract) else {
        return false;
    };
    name == acceleration && variable(left, target) && same_member(right, input)
}

fn overshoot_guard(
    statement: &Statement,
    comparison: BinaryOperator,
    acceleration: &str,
    target: &str,
    input: &Member<'_>,
) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let Some((sum, compared_target)) = binary(condition, comparison) else {
        return false;
    };
    let Some((member, adjustment)) = binary(sum, BinaryOperator::Add) else {
        return false;
    };
    matches!(then_body.as_slice(), [assignment]
        if acceleration_assignment(assignment, acceleration, target, input))
        && else_body.is_empty()
        && same_member(member, input)
        && variable(adjustment, acceleration)
        && variable(compared_target, target)
}

fn classify(function: &Function) -> Option<MemberAccelerationClamp<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [base, acceleration, target, unused] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(base.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || acceleration.parameter_type != Type::Float
        || target.parameter_type != Type::Float
        || unused.parameter_type != Type::Float
    {
        return None;
    }
    let [Statement::If {
        condition: target_guard,
        then_body: zero_target,
        else_body: nonzero_target,
    }, Statement::Store {
        target: output,
        value: stored_acceleration,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !variable(logical_not(target_guard)?, &target.name)
        || !variable(stored_acceleration, &acceleration.name)
    {
        return None;
    }
    let [Statement::Assign {
        name: zero_assignment,
        value:
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand: zero_member,
            },
    }] = zero_target.as_slice()
    else {
        return None;
    };
    let input = float_member(zero_member)?;
    let output = float_member(output)?;
    if zero_assignment != &acceleration.name
        || input.base != base.name
        || output.base != base.name
    {
        return None;
    }
    let [Statement::If {
        condition: direction_guard,
        then_body: same_direction,
        else_body: direction_else,
    }] = nonzero_target.as_slice()
    else {
        return None;
    };
    let (product, zero) = binary(logical_not(direction_guard)?, BinaryOperator::Less)?;
    let (member, adjustment) = binary(product, BinaryOperator::Multiply)?;
    if !same_member(member, &input)
        || !variable(adjustment, &acceleration.name)
        || !is_zero_literal(zero)
        || !direction_else.is_empty()
    {
        return None;
    }
    let [Statement::If {
        condition: sign_test,
        then_body: positive,
        else_body: negative,
    }] = same_direction.as_slice()
    else {
        return None;
    };
    let (signed_acceleration, sign_zero) = binary(sign_test, BinaryOperator::Greater)?;
    let [positive_guard] = positive.as_slice() else {
        return None;
    };
    let [negative_guard] = negative.as_slice() else {
        return None;
    };
    if !variable(signed_acceleration, &acceleration.name)
        || !is_zero_literal(sign_zero)
        || !overshoot_guard(
            positive_guard,
            BinaryOperator::Greater,
            &acceleration.name,
            &target.name,
            &input,
        )
        || !overshoot_guard(
            negative_guard,
            BinaryOperator::Less,
            &acceleration.name,
            &target.name,
            &input,
        )
    {
        return None;
    }
    Some(MemberAccelerationClamp {
        input,
        output_offset: output.offset,
        acceleration: &acceleration.name,
        target: &target.name,
    })
}

impl Generator {
    pub(crate) fn try_member_acceleration_clamp(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let base = self.general_register_of(shape.input.base)?;
        let acceleration = self.float_register_of(shape.acceleration)?;
        let target = self.float_register_of(shape.target)?;
        if base != 3 || acceleration != 1 || target != 2 {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let nonzero_target = self.fresh_label();
        let negative = self.fresh_label();
        let store = self.fresh_label();

        self.load_float_constant(3, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: target, b: 3 });
        self.emit_branch_conditional_to(4, 2, nonzero_target); // bne
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 0,
            a: base,
            offset: shape.input.offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: acceleration, b: 0 });
        self.emit_branch_to(store);

        self.bind_label(nonzero_target);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 4,
            a: base,
            offset: shape.input.offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: 0,
                a: 4,
                c: acceleration,
            });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 3 });
        self.emit_branch_conditional_to(12, 0, store); // blt
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: acceleration, b: 3 });
        self.emit_branch_conditional_to(4, 1, negative); // ble

        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: 4,
            b: acceleration,
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
                b: 4,
            });
        self.emit_branch_to(store);

        self.bind_label(negative);
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: 0,
            a: 4,
            b: acceleration,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: target });
        self.emit_branch_conditional_to(4, 0, store); // bge
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle {
                d: acceleration,
                a: target,
                b: 4,
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
