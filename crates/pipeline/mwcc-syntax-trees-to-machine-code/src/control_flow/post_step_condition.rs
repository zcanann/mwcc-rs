//! Direct comparisons whose value operand is a postfix scalar step.
//!
//! A postfix expression exposes the old value and mutates its target. In a
//! branch condition mwcc can consume the local's existing register directly:
//! compare the old value, advance the local, then branch. Ordinary expression
//! materialization would needlessly copy the old value into a temporary.

use super::*;

impl Generator {
    pub(super) fn try_emit_post_step_immediate_condition(
        &mut self,
        comparison: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some(plan) = recognize(left, right) else {
            return Ok(None);
        };
        let Some(location) = self.locations.get(plan.target).cloned() else {
            return Ok(None);
        };
        if location.class != ValueClass::General
            || location.width != 32
            || location.pointee.is_some()
            || location.stride.is_some()
        {
            return Ok(None);
        }

        let signed = if matches!(
            comparison,
            BinaryOperator::Equal | BinaryOperator::NotEqual
        ) {
            self.signedness_of(left)? && self.signedness_of(right)?
        } else {
            self.usual_integer_binary_signedness(left, right)?
        };
        if signed {
            let Ok(immediate) = i16::try_from(plan.immediate) else {
                return Ok(None);
            };
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate {
                    a: location.register,
                    immediate,
                });
        } else {
            let Ok(immediate) = u16::try_from(plan.immediate) else {
                return Ok(None);
            };
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate {
                    a: location.register,
                    immediate,
                });
        }
        if !self.emit_post_step_update_after_use(
            &Expression::Variable(plan.target.to_owned()),
            plan.step,
            None,
        )? {
            return Err(Diagnostic::error(
                "a recognized postfix comparison lost its register-local target",
            ));
        }
        Ok(false_branch_bo_bi(comparison))
    }
}

struct Plan<'a> {
    target: &'a str,
    step: BinaryOperator,
    immediate: i64,
}

fn recognize<'a>(left: &'a Expression, right: &Expression) -> Option<Plan<'a>> {
    let Expression::PostStep {
        target,
        operator: step,
        pointer_link: None,
    } = left
    else {
        return None;
    };
    let Expression::Variable(target) = target.as_ref() else {
        return None;
    };
    Some(Plan {
        target,
        step: *step,
        immediate: constant_value(right)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_scalar_post_increment_against_a_folded_constant() {
        let left = Expression::PostStep {
            target: Box::new(Expression::Variable("i".into())),
            operator: BinaryOperator::Add,
            pointer_link: None,
        };
        let right = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::IntegerLiteral(12)),
            right: Box::new(Expression::IntegerLiteral(4)),
        };
        let plan = recognize(&left, &right).expect("postfix comparison plan");
        assert_eq!(plan.target, "i");
        assert_eq!(plan.step, BinaryOperator::Add);
        assert_eq!(plan.immediate, 16);
    }

    #[test]
    fn rejects_an_overloaded_postfix_step() {
        let left = Expression::PostStep {
            target: Box::new(Expression::Variable("it".into())),
            operator: BinaryOperator::Add,
            pointer_link: Some((4, 8)),
        };
        assert!(recognize(&left, &Expression::IntegerLiteral(16)).is_none());
    }
}
