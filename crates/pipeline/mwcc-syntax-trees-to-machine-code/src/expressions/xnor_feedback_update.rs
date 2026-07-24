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
        let Some((name, direction)) = xnor_feedback_update_source(expression) else {
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

        let shift_7 = match direction {
            XnorFeedbackDirection::LowFromLeftShifts => Instruction::ShiftLeftImmediate {
                a: 0,
                s: source,
                shift: 7,
            },
            XnorFeedbackDirection::HighFromRightShifts => Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: source,
                shift: 7,
            },
        };
        let shift_15 = match direction {
            XnorFeedbackDirection::LowFromLeftShifts => Instruction::ShiftLeftImmediate {
                a: 4,
                s: source,
                shift: 15,
            },
            XnorFeedbackDirection::HighFromRightShifts => Instruction::ShiftRightLogicalImmediate {
                a: 4,
                s: source,
                shift: 15,
            },
        };
        let shift_23 = match direction {
            XnorFeedbackDirection::LowFromLeftShifts => Instruction::ShiftLeftImmediate {
                a: 5,
                s: source,
                shift: 23,
            },
            XnorFeedbackDirection::HighFromRightShifts => Instruction::ShiftRightLogicalImmediate {
                a: 5,
                s: source,
                shift: 23,
            },
        };
        let final_shift = match direction {
            XnorFeedbackDirection::LowFromLeftShifts => Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 31,
            },
            XnorFeedbackDirection::HighFromRightShifts => Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 31,
            },
        };

        self.output.instructions.extend([
            shift_7,
            shift_15,
            Instruction::Xor {
                a: 0,
                s: source,
                b: 0,
            },
            shift_23,
            Instruction::Xor { a: 0, s: 4, b: 0 },
            Instruction::Eqv { a: 0, s: 5, b: 0 },
            final_shift,
            Instruction::Or {
                a: destination,
                s: source,
                b: 0,
            },
        ]);
        true
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum XnorFeedbackDirection {
    LowFromLeftShifts,
    HighFromRightShifts,
}

fn xnor_feedback_update_source(expression: &Expression) -> Option<(&str, XnorFeedbackDirection)> {
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
    let (complemented, direction) = match shifted.as_ref() {
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left,
            right,
        } if constant_value(mask) == Some(1) && constant_value(right) == Some(31) => {
            (left.as_ref(), XnorFeedbackDirection::LowFromLeftShifts)
        }
        Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left,
            right,
        } if constant_value(mask) == Some(0x8000_0000) && constant_value(right) == Some(31) => {
            (left.as_ref(), XnorFeedbackDirection::HighFromRightShifts)
        }
        _ => return None,
    };
    let Expression::Unary {
        operator: UnaryOperator::BitNot,
        operand: taps,
    } = complemented
    else {
        return None;
    };
    let mut terms = Vec::new();
    collect_xor_terms(taps, &mut terms);
    let [Expression::Variable(base), shifted_7, shifted_15, shifted_23] = terms.as_slice() else {
        return None;
    };
    let shifts_match = match direction {
        XnorFeedbackDirection::LowFromLeftShifts => {
            is_shift(shifted_7, source, 7, BinaryOperator::ShiftLeft)
                && is_shift(shifted_15, source, 15, BinaryOperator::ShiftLeft)
                && is_shift(shifted_23, source, 23, BinaryOperator::ShiftLeft)
        }
        XnorFeedbackDirection::HighFromRightShifts => {
            is_shift(shifted_7, source, 7, BinaryOperator::ShiftRight)
                && is_shift(shifted_15, source, 15, BinaryOperator::ShiftRight)
                && is_shift(shifted_23, source, 23, BinaryOperator::ShiftRight)
        }
    };
    (base == source && shifts_match).then_some((source, direction))
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

fn is_shift(expression: &Expression, source: &str, amount: i64, expected: BinaryOperator) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator,
            left,
            right,
        } if *operator == expected
            && matches!(left.as_ref(), Expression::Variable(name) if name == source)
            && constant_value(right) == Some(amount)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable() -> Expression {
        Expression::Variable("word".into())
    }

    fn shift(operator: BinaryOperator, amount: i64) -> Expression {
        Expression::Binary {
            operator,
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

    fn feedback(direction: XnorFeedbackDirection, last_tap: i64) -> Expression {
        let (tap_operator, final_operator, mask) = match direction {
            XnorFeedbackDirection::LowFromLeftShifts => {
                (BinaryOperator::ShiftLeft, BinaryOperator::ShiftRight, 1)
            }
            XnorFeedbackDirection::HighFromRightShifts => (
                BinaryOperator::ShiftRight,
                BinaryOperator::ShiftLeft,
                0x8000_0000,
            ),
        };
        let taps = xor(
            xor(
                xor(variable(), shift(tap_operator, 7)),
                shift(tap_operator, 15),
            ),
            shift(tap_operator, last_tap),
        );
        Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: Box::new(variable()),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: Box::new(Expression::Binary {
                    operator: final_operator,
                    left: Box::new(Expression::Unary {
                        operator: UnaryOperator::BitNot,
                        operand: Box::new(taps),
                    }),
                    right: Box::new(Expression::IntegerLiteral(31)),
                }),
                right: Box::new(Expression::IntegerLiteral(mask)),
            }),
        }
    }

    #[test]
    fn recognizes_only_the_four_tap_mirrored_updates() {
        for direction in [
            XnorFeedbackDirection::LowFromLeftShifts,
            XnorFeedbackDirection::HighFromRightShifts,
        ] {
            assert_eq!(
                xnor_feedback_update_source(&feedback(direction, 23)),
                Some(("word", direction))
            );
            assert_eq!(xnor_feedback_update_source(&feedback(direction, 22)), None);
        }
    }
}
