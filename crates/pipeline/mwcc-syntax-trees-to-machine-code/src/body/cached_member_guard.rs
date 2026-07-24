//! Boolean guards that compare one cached member more than once.
//!
//! Copy propagation must not duplicate the member load across the comparisons.
//! This owner keeps the cached value in r0 through the equality chain, then
//! reuses r0 for the final unsigned member threshold.

#[allow(unused_imports)]
use super::*;

struct Shape {
    cached_offset: i16,
    first_value: i16,
    second_value: i16,
    threshold_offset: i16,
    threshold: u16,
    when_true: i16,
    when_false: i16,
}

fn literal_i16(expression: &Expression) -> Option<i16> {
    let Expression::IntegerLiteral(value) = expression else {
        return None;
    };
    i16::try_from(*value).ok()
}

fn member_offset(expression: &Expression, base: &str, member_type: Type) -> Option<i16> {
    let Expression::Member {
        base: member_base,
        offset,
        member_type: actual_type,
        ..
    } = expression
    else {
        return None;
    };
    (matches!(member_base.as_ref(), Expression::Variable(name) if name == base)
        && *actual_type == member_type)
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn equality_value(expression: &Expression, local: &str) -> Option<i16> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if matches!(left.as_ref(), Expression::Variable(name) if name == local) {
        literal_i16(right)
    } else if matches!(right.as_ref(), Expression::Variable(name) if name == local) {
        literal_i16(left)
    } else {
        None
    }
}

fn classify(function: &Function) -> Option<Shape> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    let [local] = function.locals.as_slice() else {
        return None;
    };
    let [guard] = function.guards.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || !function.statements.is_empty()
        || local.declared_type != Type::Int
        || !matches!(
            parameter.parameter_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
    {
        return None;
    }
    let cached_offset = member_offset(
        local.initializer.as_ref()?,
        &parameter.name,
        Type::Int,
    )?;
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: alternatives,
        right: threshold,
    } = &guard.condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LogicalOr,
        left: first,
        right: second,
    } = alternatives.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: threshold_member,
        right: threshold_value,
    } = threshold.as_ref()
    else {
        return None;
    };
    Some(Shape {
        cached_offset,
        first_value: equality_value(first, &local.name)?,
        second_value: equality_value(second, &local.name)?,
        threshold_offset: member_offset(
            threshold_member,
            &parameter.name,
            Type::UnsignedChar,
        )?,
        threshold: u16::try_from(literal_i16(threshold_value)?).ok()?,
        when_true: literal_i16(&guard.value)?,
        when_false: literal_i16(function.return_expression.as_ref()?)?,
    })
}

impl Generator {
    pub(crate) fn try_cached_member_guard(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.general_register_of(&function.parameters[0].name)? != 3 {
            return Ok(false);
        }
        let threshold = self.fresh_label();
        let false_result = self.fresh_label();
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.cached_offset,
            },
            Instruction::CompareWordImmediate {
                a: 0,
                immediate: shape.first_value,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, threshold);
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 0,
            immediate: shape.second_value,
        });
        self.emit_branch_conditional_to(4, 2, false_result);
        self.bind_label(threshold);
        self.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: 3,
                offset: shape.threshold_offset,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: shape.threshold,
            },
        ]);
        self.emit_branch_conditional_to(12, 0, false_result);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: shape.when_true,
            },
            Instruction::BranchToLinkRegister,
        ]);
        self.bind_label(false_result);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: shape.when_false,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_equality_with_the_local_on_either_side() {
        let local = Expression::Variable("cached".into());
        let value = Expression::IntegerLiteral(7);
        for (left, right) in [(local.clone(), value.clone()), (value.clone(), local.clone())] {
            let comparison = Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(left),
                right: Box::new(right),
            };
            assert_eq!(equality_value(&comparison, "cached"), Some(7));
        }
    }
}
