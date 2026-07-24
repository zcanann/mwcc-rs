//! Member-backed float friction selection with source-level absolute locals.
//!
//! This sibling of `float_friction_select` owns the no-parameter form where
//! both the candidate friction and current velocity come from one object. MWCC
//! keeps those member values in f2/f3 through four control diamonds while
//! reusing f1/f0 for their absolute values.

#[allow(unused_imports)]
use super::*;

struct MemberFloatFrictionSelect<'a> {
    pointer: &'a str,
    friction_offset: i16,
    velocity_offset: i16,
    output_offset: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn member_offset(expression: &Expression, pointer: &str) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    variable(base, pointer).then_some(i16::try_from(*offset).ok()?)
}

fn negates(expression: &Expression, operand: &Expression) -> bool {
    matches!(expression,
        Expression::Unary { operator: UnaryOperator::Negate, operand: inner }
            if same_operand(inner, operand))
}

fn absolute_select(expression: &Expression) -> Option<&Expression> {
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition.as_ref()
    else {
        return None;
    };
    (is_zero_literal(right) && negates(when_true, left) && same_operand(when_false, left))
        .then_some(left)
}

fn classify(function: &Function) -> Option<MemberFloatFrictionSelect<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let [pointer] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(pointer.parameter_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let [result, unused_absolute, absolute_velocity] = function.locals.as_slice() else {
        return None;
    };
    if [result, unused_absolute, absolute_velocity]
        .iter()
        .any(|local| {
            local.declared_type != Type::Float
                || local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }
    let friction = result.initializer.as_ref()?;
    let friction_offset = member_offset(friction, &pointer.name)?;
    let unused_operand = absolute_select(unused_absolute.initializer.as_ref()?)?;
    if !variable(unused_operand, &result.name) {
        return None;
    }
    let velocity = absolute_select(absolute_velocity.initializer.as_ref()?)?;
    let velocity_offset = member_offset(velocity, &pointer.name)?;

    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::GreaterEqual,
                left: absolute_result,
                right: compared_velocity,
            },
        then_body,
        else_body,
    }, Statement::Store {
        target,
        value: stored,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !variable(absolute_select(absolute_result)?, &result.name)
        || !variable(compared_velocity, &absolute_velocity.name)
        || !variable(stored, &result.name)
    {
        return None;
    }
    let [Statement::Assign {
        name: clamped,
        value: clamped_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::Negate,
        operand: clamped_velocity,
    } = clamped_value
    else {
        return None;
    };
    if clamped != &result.name
        || member_offset(clamped_velocity, &pointer.name)? != velocity_offset
    {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: positive_velocity,
                right: positive_zero,
            },
        then_body: sign_body,
        else_body: sign_else,
    }] = else_body.as_slice()
    else {
        return None;
    };
    let [Statement::Assign {
        name: adjusted,
        value:
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand: adjusted_friction,
            },
    }] = sign_body.as_slice()
    else {
        return None;
    };
    if !sign_else.is_empty()
        || adjusted != &result.name
        || member_offset(positive_velocity, &pointer.name)? != velocity_offset
        || !is_zero_literal(positive_zero)
        || member_offset(adjusted_friction, &pointer.name)? != friction_offset
    {
        return None;
    }
    Some(MemberFloatFrictionSelect {
        pointer: &pointer.name,
        friction_offset,
        velocity_offset,
        output_offset: member_offset(target, &pointer.name)?,
    })
}

impl Generator {
    pub(crate) fn try_member_float_friction_select(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let pointer = self.general_register_of(shape.pointer)?;
        if pointer != 3 {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.output.has_float_branch = true;
        self.output.anonymous_label_bump += 9;
        let velocity_nonnegative = self.fresh_label();
        let velocity_absolute = self.fresh_label();
        let friction_nonnegative = self.fresh_label();
        let friction_absolute = self.fresh_label();
        let adjust_sign = self.fresh_label();
        let done = self.fresh_label();

        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 3,
            a: pointer,
            offset: shape.velocity_offset,
        });
        self.load_float_constant(0, 0.0);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 2,
            a: pointer,
            offset: shape.friction_offset,
        });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 0, velocity_nonnegative);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 1, b: 3 });
        self.emit_branch_to(velocity_absolute);
        self.bind_label(velocity_nonnegative);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 3 });
        self.bind_label(velocity_absolute);

        self.load_float_constant(0, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 2, b: 0 });
        self.emit_branch_conditional_to(4, 0, friction_nonnegative);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 0, b: 2 });
        self.emit_branch_to(friction_absolute);
        self.bind_label(friction_nonnegative);
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 0, b: 2 });
        self.bind_label(friction_absolute);

        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 0, b: 1 });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterOr { d: 2, a: 1, b: 2 });
        self.emit_branch_conditional_to(4, 2, adjust_sign);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 2, b: 3 });
        self.emit_branch_to(done);
        self.bind_label(adjust_sign);
        self.load_float_constant(0, 0.0);
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 3, b: 0 });
        self.emit_branch_conditional_to(4, 1, done);
        self.output
            .instructions
            .push(Instruction::FloatNegate { d: 2, b: 2 });
        self.bind_label(done);
        self.output.instructions.push(Instruction::StoreFloatSingle {
            s: 2,
            a: pointer,
            offset: shape.output_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
