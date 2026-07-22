//! Lower value-shaped expressions whose result is discarded.

use super::*;

impl Generator {
    /// Lower `condition ? (void)0 : call()` (and its mirrored form) as a
    /// guarded call. Macro assertions use this expression-statement shape after
    /// preprocessing; mwcc branches over the cold call without materializing a
    /// ternary value.
    pub(crate) fn try_emit_conditional_call_statement(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        if self.try_emit_discarded_assertion(expression)? {
            return Ok(true);
        }
        let Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } = expression
        else {
            return Ok(false);
        };

        let is_void_noop = |arm: &Expression| {
            matches!(
                arm,
                Expression::Cast {
                    target_type: Type::Void,
                    operand,
                } if matches!(operand.as_ref(), Expression::IntegerLiteral(_))
            )
        };
        let (call, call_when_true) = match (when_true.as_ref(), when_false.as_ref()) {
            (call @ Expression::Call { .. }, noop) if is_void_noop(noop) => (call, true),
            (noop, call @ Expression::Call { .. }) if is_void_noop(noop) => (call, false),
            _ => return Ok(false),
        };
        let Expression::Call { name, arguments } = call else {
            unreachable!("the arm matcher restricts this to a direct call")
        };

        let (skip_when_false, condition_bit) = self.emit_condition_test(condition)?;
        let skip_call = if call_when_true {
            skip_when_false
        } else {
            skip_when_false ^ 8
        };
        let end = self.fresh_label();
        self.emit_branch_conditional_to(skip_call, condition_bit, end);
        self.emit_call(name, arguments, None, false)?;
        self.bind_label(end);
        Ok(true)
    }
}
