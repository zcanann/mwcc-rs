//! Count-register loops implementing the CARD unlock XNOR feedback step.
//!
//! These loops update one word for a runtime-supplied number of rounds. The
//! source counter disappears into CTR, while the loop-carried word remains in
//! its incoming/result register.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FeedbackDirection {
    Left,
    Right,
}

impl Generator {
    pub(crate) fn try_xnor_feedback_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::UnsignedInt
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [data_parameter, count_parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if data_parameter.parameter_type != Type::UnsignedInt
            || count_parameter.parameter_type != Type::UnsignedInt
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &data_parameter.name)
        {
            return Ok(false);
        }
        let [counter] = function.locals.as_slice() else {
            return Ok(false);
        };
        if counter.declared_type != Type::UnsignedInt
            || counter.is_static
            || counter.array_length.is_some()
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !is_zero_initialization(initializer, &counter.name)
            || !is_less_than_count(condition, &counter.name, &count_parameter.name)
            || !is_increment(step, &counter.name)
        {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = body.as_slice() else {
            return Ok(false);
        };
        if name != &data_parameter.name {
            return Ok(false);
        }
        let Some(direction) = feedback_direction(value, &data_parameter.name) else {
            return Ok(false);
        };
        let (Some(data), Some(count)) = (
            self.lookup_general(&data_parameter.name),
            self.lookup_general(&count_parameter.name),
        ) else {
            return Ok(false);
        };
        if data != 3 || count != 4 {
            return Ok(false);
        }

        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: count,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: count });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 1,
            });
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        match direction {
            FeedbackDirection::Right => {
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: 0,
                        s: data,
                        shift: 7,
                    });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: count,
                        s: data,
                        shift: 15,
                    });
                self.output
                    .instructions
                    .push(Instruction::Xor { a: 0, s: data, b: 0 });
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: 5,
                        s: data,
                        shift: 23,
                    });
                self.output.instructions.push(Instruction::Xor {
                    a: 0,
                    s: count,
                    b: 0,
                });
                self.emit_xnor_feedback_tail(
                    data,
                    30,
                    1,
                    Instruction::ShiftRightLogicalImmediate {
                        a: data,
                        s: data,
                        shift: 1,
                    },
                );
            }
            FeedbackDirection::Left => {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: 0,
                        s: data,
                        shift: 7,
                    });
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: count,
                        s: data,
                        shift: 15,
                    });
                self.output
                    .instructions
                    .push(Instruction::Xor { a: 0, s: data, b: 0 });
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: 5,
                        s: data,
                        shift: 23,
                    });
                self.output.instructions.push(Instruction::Xor {
                    a: 0,
                    s: count,
                    b: 0,
                });
                self.emit_xnor_feedback_tail(
                    data,
                    2,
                    30,
                    Instruction::ShiftLeftImmediate {
                        a: data,
                        s: data,
                        shift: 1,
                    },
                );
            }
        }
        self.emit_branch_conditional_to(16, 0, body_label);
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_xnor_feedback_tail(
        &mut self,
        data: u8,
        rotate: u8,
        bit: u8,
        data_shift: Instruction,
    ) {
        self.output
            .instructions
            .push(Instruction::Eqv { a: 0, s: 5, b: 0 });
        self.output.instructions.push(data_shift);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: rotate,
            begin: bit,
            end: bit,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: data, s: data, b: 0 });
    }
}

fn is_zero_initialization(expression: &Expression, counter: &str) -> bool {
    matches!(expression,
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == counter)
                && constant_value(value) == Some(0))
}

fn is_less_than_count(expression: &Expression, counter: &str, count: &str) -> bool {
    matches!(expression,
        Expression::Binary { operator: BinaryOperator::Less, left, right }
            if matches!(left.as_ref(), Expression::Variable(name) if name == counter)
                && matches!(right.as_ref(), Expression::Variable(name) if name == count))
}

fn is_increment(expression: &Expression, counter: &str) -> bool {
    matches!(expression,
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == counter)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } if matches!(left.as_ref(), Expression::Variable(name) if name == counter)
                    && constant_value(right) == Some(1)))
}

fn feedback_direction(expression: &Expression, data: &str) -> Option<FeedbackDirection> {
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = expression
    else {
        return None;
    };
    for direction in [FeedbackDirection::Right, FeedbackDirection::Left] {
        if is_shift(left, data, direction, 1) && is_feedback_bit(right, data, direction) {
            return Some(direction);
        }
    }
    None
}

fn is_feedback_bit(expression: &Expression, data: &str, direction: FeedbackDirection) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return false;
    };
    let (feedback_shift, mask) = match direction {
        FeedbackDirection::Right => (30, 0x4000_0000),
        FeedbackDirection::Left => (30, 0x0000_0002),
    };
    let Expression::Binary {
        operator,
        left: taps,
        right: amount,
    } = left.as_ref()
    else {
        return false;
    };
    let expected_operator = match direction {
        FeedbackDirection::Right => BinaryOperator::ShiftLeft,
        FeedbackDirection::Left => BinaryOperator::ShiftRight,
    };
    *operator == expected_operator
        && constant_value(amount) == Some(feedback_shift)
        && constant_value(right) == Some(mask)
        && has_xnor_taps(taps, data, direction)
}

fn has_xnor_taps(expression: &Expression, data: &str, direction: FeedbackDirection) -> bool {
    let Expression::Unary {
        operator: UnaryOperator::BitNot,
        operand,
    } = expression
    else {
        return false;
    };
    let mut terms = Vec::new();
    collect_xor_terms(operand, &mut terms);
    if terms.len() != 4 || !is_variable(terms[0], data) {
        return false;
    }
    [7, 15, 23]
        .into_iter()
        .zip(&terms[1..])
        .all(|(amount, term)| is_shift(term, data, direction, amount))
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

fn is_shift(
    expression: &Expression,
    data: &str,
    direction: FeedbackDirection,
    amount: i64,
) -> bool {
    let expected = match direction {
        FeedbackDirection::Right => BinaryOperator::ShiftRight,
        FeedbackDirection::Left => BinaryOperator::ShiftLeft,
    };
    matches!(expression,
        Expression::Binary { operator, left, right }
            if *operator == expected
                && is_variable(left, data)
                && constant_value(right) == Some(amount))
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(value: &str) -> Expression {
        Expression::Variable(value.into())
    }

    fn integer(value: i64) -> Expression {
        Expression::IntegerLiteral(value)
    }

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn shifted(direction: FeedbackDirection, amount: i64) -> Expression {
        binary(
            match direction {
                FeedbackDirection::Left => BinaryOperator::ShiftLeft,
                FeedbackDirection::Right => BinaryOperator::ShiftRight,
            },
            name("data"),
            integer(amount),
        )
    }

    fn feedback(direction: FeedbackDirection) -> Expression {
        let taps = [7, 15, 23].into_iter().fold(name("data"), |left, amount| {
            binary(BinaryOperator::BitXor, left, shifted(direction, amount))
        });
        let xnor = Expression::Unary {
            operator: UnaryOperator::BitNot,
            operand: Box::new(taps),
        };
        let (feedback_operator, mask) = match direction {
            FeedbackDirection::Right => (BinaryOperator::ShiftLeft, 0x4000_0000),
            FeedbackDirection::Left => (BinaryOperator::ShiftRight, 2),
        };
        binary(
            BinaryOperator::BitOr,
            shifted(direction, 1),
            binary(
                BinaryOperator::BitAnd,
                binary(feedback_operator, xnor, integer(30)),
                integer(mask),
            ),
        )
    }

    #[test]
    fn recognizes_both_xnor_feedback_directions() {
        assert_eq!(
            feedback_direction(&feedback(FeedbackDirection::Right), "data"),
            Some(FeedbackDirection::Right)
        );
        assert_eq!(
            feedback_direction(&feedback(FeedbackDirection::Left), "data"),
            Some(FeedbackDirection::Left)
        );
    }

    #[test]
    fn rejects_a_feedback_polynomial_with_a_different_tap() {
        let mut expression = feedback(FeedbackDirection::Right);
        let Expression::Binary { right, .. } = &mut expression else {
            unreachable!()
        };
        let Expression::Binary { left, .. } = right.as_mut() else {
            unreachable!()
        };
        let Expression::Binary { left: taps, .. } = left.as_mut() else {
            unreachable!()
        };
        let Expression::Unary { operand, .. } = taps.as_mut() else {
            unreachable!()
        };
        let Expression::Binary { right: last, .. } = operand.as_mut() else {
            unreachable!()
        };
        *last = Box::new(shifted(FeedbackDirection::Right, 22));
        assert_eq!(feedback_direction(&expression, "data"), None);
    }
}
