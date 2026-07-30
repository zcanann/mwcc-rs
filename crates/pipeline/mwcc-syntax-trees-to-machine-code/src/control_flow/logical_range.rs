//! Consecutive equality alternatives lowered as one unsigned range test.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Type;

struct EqualityRange<'a> {
    name: &'a str,
    minimum: i64,
    span: u16,
}

impl Generator {
    /// Emit an equality-alternative chain whose true path immediately returns
    /// one integer value. Adjacent alternatives are coalesced into unsigned
    /// ranges; earlier groups branch to one shared return block and the final
    /// group's false edge reaches the continuation.
    pub(crate) fn try_emit_logical_alternative_early_return(
        &mut self,
        condition: &Expression,
        value: &Expression,
        return_type: Type,
    ) -> Compilation<bool> {
        if self.behavior.logical_or_value_style != mwcc_versions::LogicalOrValueStyle::TrueFirst
            || !matches!(
                return_type,
                Type::Int | Type::UnsignedInt | Type::Short | Type::UnsignedShort
            )
        {
            return Ok(false);
        }
        let Some(return_value) = constant_value(value) else {
            return Ok(false);
        };
        let Some((name, constants)) = equality_alternatives(condition) else {
            return Ok(false);
        };
        let Some(register) = self.lookup_general(name) else {
            return Ok(false);
        };
        let Some(groups) = consecutive_groups(&constants) else {
            return Ok(false);
        };

        let mut taken_branches = Vec::new();
        let mut final_false = None;
        let last = groups.len() - 1;
        for (index, (minimum, maximum)) in groups.into_iter().enumerate() {
            let span = maximum - minimum;
            let (true_options, false_options, condition_bit) = if span == 0 {
                let Ok(immediate) = u16::try_from(minimum) else {
                    return Ok(false);
                };
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: register,
                        immediate,
                    });
                (12, 4, 2)
            } else {
                let Some(immediate) = minimum
                    .checked_neg()
                    .and_then(|value| i16::try_from(value).ok())
                else {
                    return Ok(false);
                };
                let Ok(span) = u16::try_from(span) else {
                    return Ok(false);
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
                        immediate: span,
                    });
                (4, 12, 1)
            };
            let branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: if index == last {
                        false_options
                    } else {
                        true_options
                    },
                    condition_bit,
                    target: 0,
                });
            if index == last {
                final_false = Some(branch);
            } else {
                taken_branches.push(branch);
            }
        }

        let taken = self.output.instructions.len();
        for branch in taken_branches {
            self.patch_forward(branch, taken);
        }
        self.load_integer_constant(mwcc_target::Eabi::general_result().number, return_value);
        self.emit_epilogue_and_return();
        let continuation = self.output.instructions.len();
        self.patch_forward(
            final_false.expect("a non-empty alternative chain has a final branch"),
            continuation,
        );
        Ok(true)
    }

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

fn equality_alternatives(condition: &Expression) -> Option<(&str, Vec<i64>)> {
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
    (collect(condition, &mut name, &mut constants) && constants.len() >= 2)
        .then_some((name?, constants))
}

fn consecutive_groups(constants: &[i64]) -> Option<Vec<(i64, i64)>> {
    let (&first, rest) = constants.split_first()?;
    let mut groups = vec![(first, first)];
    for &value in rest {
        let current = groups.last_mut()?;
        if value == current.1.checked_add(1)? {
            current.1 = value;
        } else if value > current.1 {
            groups.push((value, value));
        } else {
            return None;
        }
    }
    Some(groups)
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

    #[test]
    fn groups_ordered_singletons_around_one_consecutive_range() {
        assert_eq!(
            consecutive_groups(&[1, 4, 5, 14]),
            Some(vec![(1, 1), (4, 5), (14, 14)])
        );
    }
}
