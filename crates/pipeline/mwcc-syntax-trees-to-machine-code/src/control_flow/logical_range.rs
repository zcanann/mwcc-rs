//! Consecutive equality alternatives lowered as one unsigned range test.

#[allow(unused_imports)]
use super::*;

struct EqualityRange<'a> {
    name: &'a str,
    minimum: i64,
    span: u16,
}

impl Generator {
    /// Recognize `(x == A) || ... || (x == B)` when the alternatives cover one
    /// contiguous interval. MWCC canonicalizes that chain to
    /// `(unsigned)(x - A) <= B - A`, which tests the lower and upper bounds in
    /// one unsigned comparison.
    pub(crate) fn try_emit_logical_equality_range_condition(
        &mut self,
        condition: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if self.behavior.logical_or_value_style != mwcc_versions::LogicalOrValueStyle::TrueFirst {
            return Ok(None);
        }
        let Some(range) = equality_range(condition) else {
            return Ok(None);
        };
        let Some(register) = self.lookup_general(range.name) else {
            return Ok(None);
        };
        let Some(immediate) = range
            .minimum
            .checked_neg()
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(None);
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: register,
            immediate,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: GENERAL_SCRATCH,
                immediate: range.span,
            });
        // The guarded body is skipped when the unsigned difference exceeds
        // the interval span (`bgt` from CR0).
        Ok(Some((12, 1)))
    }
}

fn equality_range(condition: &Expression) -> Option<EqualityRange<'_>> {
    fn collect<'e>(
        expression: &'e Expression,
        name: &mut Option<&'e str>,
        constants: &mut Vec<i64>,
    ) -> bool {
        match expression {
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left,
                right,
            } => collect(left, name, constants) && collect(right, name, constants),
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } => {
                let pair = match (left.as_ref(), right.as_ref()) {
                    (Expression::Variable(candidate), constant) => {
                        constant_value(constant).map(|value| (candidate.as_str(), value))
                    }
                    (constant, Expression::Variable(candidate)) => {
                        constant_value(constant).map(|value| (candidate.as_str(), value))
                    }
                    _ => None,
                };
                let Some((candidate, value)) = pair else {
                    return false;
                };
                if name.is_some_and(|current| current != candidate) {
                    return false;
                }
                *name = Some(candidate);
                constants.push(value);
                true
            }
            _ => false,
        }
    }

    let mut name = None;
    let mut constants = Vec::new();
    if !collect(condition, &mut name, &mut constants) || constants.len() < 2 {
        return None;
    }
    constants.sort_unstable();
    constants.dedup();
    if constants.len() < 2
        || constants
            .windows(2)
            .any(|pair| pair[0].checked_add(1) != Some(pair[1]))
    {
        return None;
    }
    let minimum = constants[0];
    let maximum = *constants.last()?;
    let span = u16::try_from(maximum.checked_sub(minimum)?).ok()?;
    Some(EqualityRange {
        name: name?,
        minimum,
        span,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn equal(name: &str, value: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(Expression::Variable(name.into())),
            right: Box::new(Expression::IntegerLiteral(value)),
        }
    }

    fn or(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn recognizes_a_four_value_interval_across_a_nested_or_tree() {
        let condition = or(
            or(
                or(equal("command", 9), equal("command", 10)),
                equal("command", 11),
            ),
            equal("command", 12),
        );
        let range = equality_range(&condition).expect("consecutive equality range");
        assert_eq!(range.name, "command");
        assert_eq!(range.minimum, 9);
        assert_eq!(range.span, 3);
    }

    #[test]
    fn rejects_a_gap_or_a_different_operand() {
        assert!(equality_range(&or(equal("command", 9), equal("command", 11))).is_none());
        assert!(equality_range(&or(equal("command", 9), equal("other", 10))).is_none());
    }
}
