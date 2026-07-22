//! Single-step CARD-style XNOR feedback updates.
//!
//! The loop owner handles the count-register form. This owner handles the same
//! four-tap polynomial after a call, where MWCC evaluates the independent shifts
//! in parallel and folds the final complement into `eqv`.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn try_emit_xnor_feedback_update(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> bool {
        let Some(name) = xnor_feedback_update_source(expression) else {
            return false;
        };
        let Some(source) = self.lookup_general(name) else {
            return false;
        };
        let follows_call = self
            .output
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }));
        if !follows_call || source != Eabi::general_result().number || destination != 0 {
            return false;
        }

        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: source,
                shift: 7,
            },
            Instruction::ShiftLeftImmediate {
                a: 4,
                s: source,
                shift: 15,
            },
            Instruction::Xor {
                a: 0,
                s: source,
                b: 0,
            },
            Instruction::ShiftLeftImmediate {
                a: 5,
                s: source,
                shift: 23,
            },
            Instruction::Xor { a: 0, s: 4, b: 0 },
            Instruction::Eqv { a: 0, s: 5, b: 0 },
            Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            },
            Instruction::Or {
                a: destination,
                s: source,
                b: 0,
            },
        ]);
        true
    }
}

fn xnor_feedback_update_source(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right: feedback,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(source) = left.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: shifted,
        right: mask,
    } = feedback.as_ref()
    else {
        return None;
    };
    if constant_value(mask) != Some(1) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::ShiftRight,
        left: complemented,
        right: shift,
    } = shifted.as_ref()
    else {
        return None;
    };
    if constant_value(shift) != Some(31) {
        return None;
    }
    let Expression::Unary {
        operator: UnaryOperator::BitNot,
        operand: taps,
    } = complemented.as_ref()
    else {
        return None;
    };
    let mut terms = Vec::new();
    collect_xor_terms(taps, &mut terms);
    let [Expression::Variable(base), shifted_7, shifted_15, shifted_23] = terms.as_slice()
    else {
        return None;
    };
    (base == source
        && is_left_shift(shifted_7, source, 7)
        && is_left_shift(shifted_15, source, 15)
        && is_left_shift(shifted_23, source, 23))
    .then_some(source)
}

fn collect_xor_terms<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::BitXor,
        left,
        right,
    } = expression
    {
        collect_xor_terms(left, terms);
        collect_xor_terms(right, terms);
    } else {
        terms.push(expression);
    }
}

fn is_left_shift(expression: &Expression, source: &str, amount: i64) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name) if name == source)
            && constant_value(right) == Some(amount)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable() -> Expression {
        Expression::Variable("word".into())
    }

    fn shift(amount: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: Box::new(variable()),
            right: Box::new(Expression::IntegerLiteral(amount)),
        }
    }

    fn xor(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::BitXor,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn feedback(last_tap: i64) -> Expression {
        let taps = xor(xor(xor(variable(), shift(7)), shift(15)), shift(last_tap));
        Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: Box::new(variable()),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::ShiftRight,
                    left: Box::new(Expression::Unary {
                        operator: UnaryOperator::BitNot,
                        operand: Box::new(taps),
                    }),
                    right: Box::new(Expression::IntegerLiteral(31)),
                }),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        }
    }

    #[test]
    fn recognizes_only_the_four_tap_left_shift_update() {
        assert_eq!(xnor_feedback_update_source(&feedback(23)), Some("word"));
        assert_eq!(xnor_feedback_update_source(&feedback(22)), None);
    }
}
