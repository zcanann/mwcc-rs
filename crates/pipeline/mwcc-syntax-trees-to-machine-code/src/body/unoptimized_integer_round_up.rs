//! O0 allocation for a computed integer rounded up to a power of two.
//!
//! The ordinary expression evaluator may overwrite the incoming parameter
//! after the scaled value is formed. O0 instead gives the subtraction its own
//! temporary, then uses r0 for the bias and the result register for the mask.

#[allow(unused_imports)]
use super::*;
use mwcc_versions::Optimization;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RoundUpPlan {
    shift: u8,
    bias: i16,
    mask_end: u8,
}

impl Generator {
    pub(crate) fn try_unoptimized_integer_round_up(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.optimization != Optimization::O0
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt)
            || self
                .locations
                .get(&parameter.name)
                .map(|location| (location.class, location.register))
                != Some((ValueClass::General, Eabi::FIRST_GENERAL_ARGUMENT))
        {
            return Ok(false);
        }
        let Some(plan) = round_up_plan(function.return_expression.as_ref(), &parameter.name) else {
            return Ok(false);
        };

        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: Eabi::FIRST_GENERAL_ARGUMENT,
                shift: plan.shift,
            },
            Instruction::SubtractFrom {
                d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                a: Eabi::FIRST_GENERAL_ARGUMENT,
                b: GENERAL_SCRATCH,
            },
            Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                immediate: plan.bias,
            },
            Instruction::AndContiguousMask {
                a: Eabi::general_result().number,
                s: GENERAL_SCRATCH,
                begin: 0,
                end: plan.mask_end,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn round_up_plan(expression: Option<&Expression>, parameter: &str) -> Option<RoundUpPlan> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression?
    else {
        return None;
    };
    let (biased, mask) = if let Some(mask) = constant_value(right) {
        (left.as_ref(), mask)
    } else {
        (right.as_ref(), constant_value(left)?)
    };
    let mask = mask as i32 as u32;
    let cleared_bits = mask.trailing_zeros();
    if !(1..=15).contains(&cleared_bits) || mask != u32::MAX << cleared_bits {
        return None;
    }
    let alignment = 1_i64 << cleared_bits;
    let (inner, bias) = peel_additive_constant(biased)?;
    if bias != alignment - 1 {
        return None;
    }
    let inner = match inner {
        Expression::Cast {
            target_type: Type::UnsignedInt,
            operand,
        } => operand.as_ref(),
        _ => return None,
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = inner
    else {
        return None;
    };
    if !matches!(right.as_ref(), Expression::Variable(name) if name == parameter) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = left.as_ref()
    else {
        return None;
    };
    let scale = match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), constant) | (constant, Expression::Variable(name))
            if name == parameter => u32::try_from(constant_value(constant)?).ok()?,
        _ => return None,
    };
    if !scale.is_power_of_two() {
        return None;
    }
    let shift = u8::try_from(scale.trailing_zeros()).ok()?;
    if !(1..=31).contains(&shift) {
        return None;
    }
    Some(RoundUpPlan {
        shift,
        bias: i16::try_from(bias).ok()?,
        mask_end: 31 - u8::try_from(cleared_bits).ok()?,
    })
}

fn peel_additive_constant(expression: &Expression) -> Option<(&Expression, i64)> {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } => {
            if let Some(constant) = constant_value(right) {
                let (inner, prior) = peel_additive_constant(left)?;
                Some((inner, prior.checked_add(constant)?))
            } else if let Some(constant) = constant_value(left) {
                let (inner, prior) = peel_additive_constant(right)?;
                Some((inner, prior.checked_add(constant)?))
            } else {
                Some((expression, 0))
            }
        }
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } if constant_value(right).is_some() => {
            let (inner, prior) = peel_additive_constant(left)?;
            Some((inner, prior.checked_sub(constant_value(right)?)?))
        }
        _ => Some((expression, 0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }

    #[test]
    fn recognizes_scaled_difference_rounded_to_four() {
        let parameter = "width";
        let inner = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: Box::new(variable(parameter)),
                right: Box::new(Expression::IntegerLiteral(4)),
            }),
            right: Box::new(variable(parameter)),
        };
        let expression = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Cast {
                        target_type: Type::UnsignedInt,
                        operand: Box::new(inner),
                    }),
                    right: Box::new(Expression::IntegerLiteral(4)),
                }),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
            right: Box::new(Expression::IntegerLiteral(-4)),
        };

        assert_eq!(
            round_up_plan(Some(&expression), parameter),
            Some(RoundUpPlan {
                shift: 2,
                bias: 3,
                mask_end: 29,
            })
        );
        assert_eq!(round_up_plan(Some(&expression), "height"), None);
    }
}
