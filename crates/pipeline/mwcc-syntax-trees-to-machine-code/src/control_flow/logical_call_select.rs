//! Short-circuit logical values selecting a call result or constant fallback.
//!
//! MWCC materializes the logical condition into a temporary boolean, tests its
//! low byte, and then joins a call arm with a constant arm in the EABI result
//! register. Keeping this separate from the ordinary leaf selector prevents
//! call scheduling and boolean normalization from leaking into its branchless
//! arithmetic policies.

use super::*;

impl Generator {
    pub(crate) fn try_emit_logical_call_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<bool> {
        let Some((operator, left, right)) = logical_parts(condition) else {
            return Ok(false);
        };
        if tail
            || destination != mwcc_target::Eabi::general_result().number
            || !call_and_constant_arms(when_true, when_false)
        {
            return Ok(false);
        }

        let boolean = self.fresh_virtual_general_preferring(5);
        self.emit_logical_select_boolean(operator, left, right, boolean)?;
        self.emit_widen_record(GENERAL_SCRATCH, boolean, 8, false);

        let false_arm = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, false_arm);
        self.evaluate_general(when_true, destination)?;
        self.emit_branch_to(join);
        self.bind_label(false_arm);
        self.evaluate_general(when_false, destination)?;
        self.bind_label(join);
        Ok(true)
    }

    fn emit_logical_select_boolean(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        let initial = if operator == BinaryOperator::LogicalAnd {
            0
        } else {
            1
        };
        let final_value = 1 - initial;
        let decisive_is_true = operator == BinaryOperator::LogicalOr;

        let (left_false, left_bit) = self.emit_condition_test(left)?;
        let preload = Instruction::load_immediate(destination, initial);
        if !self.insert_before_terminal_compare(preload.clone()) {
            self.output.instructions.push(preload);
        }
        let join = self.fresh_label();
        self.emit_branch_conditional_to(
            if decisive_is_true {
                left_false ^ 8
            } else {
                left_false
            },
            left_bit,
            join,
        );

        let (right_false, right_bit) = self.emit_condition_test(right)?;
        self.emit_branch_conditional_to(
            if decisive_is_true {
                right_false ^ 8
            } else {
                right_false
            },
            right_bit,
            join,
        );
        self.output
            .instructions
            .push(Instruction::load_immediate(destination, final_value));
        self.bind_label(join);
        Ok(())
    }
}

pub(crate) fn is_logical_call_select(expression: &Expression) -> bool {
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = expression
    else {
        return false;
    };
    logical_parts(condition).is_some() && call_and_constant_arms(when_true, when_false)
}

fn logical_parts(condition: &Expression) -> Option<(BinaryOperator, &Expression, &Expression)> {
    let Expression::Binary {
        operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr),
        left,
        right,
    } = condition
    else {
        return None;
    };
    if [left.as_ref(), right.as_ref()].into_iter().any(|term| {
        matches!(
            term,
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
                ..
            }
        )
    }) {
        return None;
    }
    Some((*operator, left, right))
}

fn call_and_constant_arms(when_true: &Expression, when_false: &Expression) -> bool {
    let is_call = |arm: &Expression| matches!(arm, Expression::Call { .. });
    let is_constant = |arm: &Expression| constant_value(arm).is_some();
    (is_call(when_true) && is_constant(when_false))
        || (is_constant(when_true) && is_call(when_false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comparison(name: &str) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Variable(name.into())),
            right: Box::new(Expression::IntegerLiteral(0)),
        }
    }

    #[test]
    fn recognizes_logical_call_and_constant_selects_only() {
        let logical = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(comparison("left")),
            right: Box::new(comparison("right")),
        };
        let call = Expression::Call {
            name: "probe".into(),
            arguments: Vec::new(),
        };
        let expression = Expression::Conditional {
            condition: Box::new(logical.clone()),
            when_true: Box::new(call),
            when_false: Box::new(Expression::IntegerLiteral(1)),
            origin: ConditionalOrigin::Ternary,
        };
        assert!(is_logical_call_select(&expression));

        let ordinary = Expression::Conditional {
            condition: Box::new(logical),
            when_true: Box::new(Expression::IntegerLiteral(2)),
            when_false: Box::new(Expression::IntegerLiteral(1)),
            origin: ConditionalOrigin::Ternary,
        };
        assert!(!is_logical_call_select(&ordinary));
    }
}
