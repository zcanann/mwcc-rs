//! A global callback call selected against a direct fallback call.
//!
//! Retained inline wrappers commonly spell
//! `callback ? callback(arguments) : fallback(arguments)`.  The callback load
//! is both the null-tested condition and the indirect callee, so this owner
//! keeps it in r12 across the branch and emits only the selected call.

use super::*;

struct CallbackFallback<'a> {
    callback: &'a str,
    callback_arguments: &'a [Expression],
    fallback: &'a str,
    fallback_arguments: &'a [Expression],
}

fn callback_condition(expression: &Expression) -> Option<(&str, bool)> {
    let Expression::Binary {
        operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let callback = match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), zero) if constant_value(zero) == Some(0) => name,
        (zero, Expression::Variable(name)) if constant_value(zero) == Some(0) => name,
        _ => return None,
    };
    Some((
        callback,
        matches!(
            expression,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                ..
            }
        ),
    ))
}

fn classify<'a>(
    condition: &'a Expression,
    when_true: &'a Expression,
    when_false: &'a Expression,
) -> Option<CallbackFallback<'a>> {
    let (callback, callback_on_true) = callback_condition(condition)?;
    let (callback_arm, fallback_arm) = if callback_on_true {
        (when_true, when_false)
    } else {
        (when_false, when_true)
    };
    let (
        Expression::Call {
            name: callback_call,
            arguments: callback_arguments,
        },
        Expression::Call {
            name: fallback,
            arguments: fallback_arguments,
        },
    ) = (callback_arm, fallback_arm)
    else {
        return None;
    };
    if callback_call != callback
        || callback_arguments.len() != fallback_arguments.len()
        || !callback_arguments
            .iter()
            .zip(fallback_arguments)
            .all(|(left, right)| structurally_equal(left, right))
    {
        return None;
    }
    Some(CallbackFallback {
        callback,
        callback_arguments,
        fallback,
        fallback_arguments,
    })
}

pub(crate) fn is_callback_fallback_select(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } if classify(condition, when_true, when_false).is_some()
    )
}

impl Generator {
    pub(crate) fn try_emit_callback_fallback_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<bool> {
        let Some(shape) = classify(condition, when_true, when_false) else {
            return Ok(false);
        };
        if tail
            || destination != mwcc_target::Eabi::general_result().number
            || !self.globals.contains_key(shape.callback)
            || self.globals.contains_key(shape.fallback)
        {
            return Ok(false);
        }

        self.emit_global_load_value(shape.callback, 12)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        let fallback = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, fallback);
        self.emit_arguments(shape.callback_arguments, shape.callback)?;
        self.emit_indirect_branch_and_link(12);
        self.emit_branch_to(join);
        self.bind_label(fallback);
        self.emit_call(
            shape.fallback,
            shape.fallback_arguments,
            Some(destination),
            false,
        )?;
        self.bind_label(join);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str) -> Expression {
        Expression::Call {
            name: name.into(),
            arguments: vec![Expression::Variable("argument".into())],
        }
    }

    #[test]
    fn recognizes_a_callback_call_with_the_same_fallback_arguments() {
        let expression = Expression::Conditional {
            condition: Box::new(Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: Box::new(Expression::Variable("callback".into())),
                right: Box::new(Expression::IntegerLiteral(0)),
            }),
            when_true: Box::new(call("callback")),
            when_false: Box::new(call("fallback")),
            origin: ConditionalOrigin::IfReturns,
        };
        assert!(is_callback_fallback_select(&expression));
    }

    #[test]
    fn rejects_different_callback_and_fallback_arguments() {
        let expression = Expression::Conditional {
            condition: Box::new(Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: Box::new(Expression::Variable("callback".into())),
                right: Box::new(Expression::IntegerLiteral(0)),
            }),
            when_true: Box::new(call("callback")),
            when_false: Box::new(Expression::Call {
                name: "fallback".into(),
                arguments: vec![Expression::Variable("other".into())],
            }),
            origin: ConditionalOrigin::IfReturns,
        };
        assert!(!is_callback_fallback_select(&expression));
    }
}
