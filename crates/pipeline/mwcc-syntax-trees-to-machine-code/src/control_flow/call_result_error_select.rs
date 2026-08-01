//! Canonical zero-or-error tail select over a retained call result.
//!
//! For `result == 0 ? 0 : -1`, MWCC keeps the call result in the ABI result
//! lane and emits the source diamond.  The general consecutive-constant masks
//! do not apply because the zero arm is represented by control flow here.

use super::*;

impl Generator {
    pub(crate) fn try_emit_call_result_error_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if destination == GENERAL_SCRATCH
            || !call_result_error_select(condition, when_true, when_false)
        {
            return Ok(false);
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let error = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, error);
        self.load_integer_constant(destination, 0);
        self.emit_branch_to(join);
        self.bind_label(error);
        self.load_integer_constant(destination, -1);
        self.bind_label(join);
        self.output.anonymous_label_bump += 3;
        Ok(true)
    }
}

fn call_result_error_select(
    condition: &Expression,
    when_true: &Expression,
    when_false: &Expression,
) -> bool {
    if constant_value(when_true) != Some(0) || constant_value(when_false) != Some(-1) {
        return false;
    }
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return false;
    };
    let Expression::Variable(_) = left.as_ref() else {
        return false;
    };
    constant_value(right) == Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_zero_or_error_call_result_tail() {
        let condition = Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(Expression::Variable("result".into())),
            right: Box::new(Expression::IntegerLiteral(0)),
        };

        assert!(
            call_result_error_select(
                &condition,
                &Expression::IntegerLiteral(0),
                &Expression::IntegerLiteral(-1),
            )
        );
    }
}
