//! Consecutive equality alternatives lowered as one unsigned range test.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::{Statement, Type};

struct EqualityRange<'a> {
    name: &'a str,
    minimum: i64,
    span: u16,
}

impl Generator {
    /// Replace a grouped equality body's one-instruction goto with direct
    /// conditional edges to the same label. This is the structured form MWCC
    /// uses for `if (x == A || ...) break`, and unlike a global forward-branch
    /// peephole it does not rewrite unrelated conditional diamonds.
    pub(crate) fn fold_logical_equality_alternative_goto(
        &mut self,
        then_body: &[Statement],
        body_start: usize,
        enter_body: &[usize],
        skip_body: &mut Vec<usize>,
        pending_gotos: &mut Vec<(usize, String)>,
    ) {
        fold_equality_alternative_goto(
            &mut self.output.instructions,
            then_body,
            body_start,
            enter_body,
            skip_body,
            pending_gotos,
        );
    }

    /// Emit a structured OR of equalities as ordered singleton/range tests.
    ///
    /// The returned vectors contain branches that enter the guarded body and
    /// skip it, respectively.  The caller owns their final targets because it
    /// also owns the structured body.  Keeping this here lets every structured
    /// CFG owner share the same equality grouping policy as early returns.
    pub(crate) fn try_emit_logical_equality_alternative_branches(
        &mut self,
        condition: &Expression,
    ) -> Compilation<Option<(Vec<usize>, Vec<usize>)>> {
        if self.behavior.logical_or_value_style != mwcc_versions::LogicalOrValueStyle::TrueFirst {
            return Ok(None);
        }
        let Some((name, groups)) = equality_alternative_groups(condition) else {
            return Ok(None);
        };
        // A run of unrelated singleton comparisons is already represented
        // faithfully by the ordinary short-circuit CFG.  Take ownership only
        // when grouping removes at least one comparison.
        if groups.iter().all(|(minimum, maximum)| minimum == maximum) {
            return Ok(None);
        }
        let Some(location) = self.locations.get(name) else {
            return Ok(None);
        };
        if location.class != crate::generator::ValueClass::General || location.width != 32 {
            return Ok(None);
        }
        let register = location.register;
        let signed = location.signed;

        enum Test {
            SignedSingleton(i16),
            UnsignedSingleton(u16),
            Range { immediate: i16, span: u16 },
        }
        let mut tests = Vec::with_capacity(groups.len());
        for (minimum, maximum) in groups {
            let span = maximum - minimum;
            if span == 0 {
                if signed {
                    let Ok(immediate) = i16::try_from(minimum) else {
                        return Ok(None);
                    };
                    tests.push(Test::SignedSingleton(immediate));
                } else {
                    let Ok(immediate) = u16::try_from(minimum) else {
                        return Ok(None);
                    };
                    tests.push(Test::UnsignedSingleton(immediate));
                }
            } else {
                let Some(immediate) = minimum
                    .checked_neg()
                    .and_then(|value| i16::try_from(value).ok())
                else {
                    return Ok(None);
                };
                let Ok(span) = u16::try_from(span) else {
                    return Ok(None);
                };
                tests.push(Test::Range { immediate, span });
            }
        }

        let mut enter_body = Vec::new();
        let mut skip_body = Vec::new();
        let last = tests.len() - 1;
        for (index, test) in tests.into_iter().enumerate() {
            let (true_options, false_options, condition_bit) = match test {
                Test::SignedSingleton(immediate) => {
                    self.output
                        .instructions
                        .push(Instruction::CompareWordImmediate {
                            a: register,
                            immediate,
                        });
                    (12, 4, 2)
                }
                Test::UnsignedSingleton(immediate) => {
                    self.output
                        .instructions
                        .push(Instruction::CompareLogicalWordImmediate {
                            a: register,
                            immediate,
                        });
                    (12, 4, 2)
                }
                Test::Range { immediate, span } => {
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
                }
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
                skip_body.push(branch);
            } else {
                enter_body.push(branch);
            }
        }
        Ok(Some((enter_body, skip_body)))
    }

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

fn fold_equality_alternative_goto(
    instructions: &mut Vec<Instruction>,
    then_body: &[Statement],
    body_start: usize,
    enter_body: &[usize],
    skip_body: &mut Vec<usize>,
    pending_gotos: &mut Vec<(usize, String)>,
) {
    let [Statement::Goto(label)] = then_body else {
        return;
    };
    let Some(&terminal) = skip_body.first() else {
        return;
    };
    if instructions.len() != body_start + 1
        || !matches!(
            instructions[body_start],
            Instruction::Branch { target: 0 }
        )
        || !matches!(
            instructions.get(terminal),
            Some(Instruction::BranchConditionalForward { .. })
        )
        || !matches!(
            pending_gotos.last(),
            Some((branch, pending_label))
                if *branch == body_start && pending_label == label
        )
        || skip_body.len() != 1
    {
        return;
    }

    pending_gotos.pop();
    instructions.pop();
    for &branch in enter_body {
        pending_gotos.push((branch, label.clone()));
    }
    skip_body.pop();
    if let Instruction::BranchConditionalForward {
        options, target, ..
    } = &mut instructions[terminal]
    {
        *options ^= 8;
        *target = 0;
        pending_gotos.push((terminal, label.clone()));
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

fn equality_alternative_groups(condition: &Expression) -> Option<(&str, Vec<(i64, i64)>)> {
    let (name, constants) = equality_alternatives(condition)?;
    Some((name, consecutive_groups(&constants)?))
}

fn consecutive_groups(constants: &[i64]) -> Option<Vec<(i64, i64)>> {
    let mut constants = constants.to_vec();
    constants.sort_unstable();
    constants.dedup();
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

    #[test]
    fn sorts_source_alternatives_before_grouping_adjacent_values() {
        assert_eq!(
            consecutive_groups(&[0, -1, 10]),
            Some(vec![(-1, 0), (10, 10)])
        );
    }

    #[test]
    fn grouped_equality_goto_becomes_direct_conditional_edges() {
        let mut instructions = vec![
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 2,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 3,
            },
            Instruction::Branch { target: 0 },
        ];
        let then_body = vec![Statement::Goto("exit".into())];
        let mut skip_body = vec![1];
        let mut pending_gotos = vec![(2, "exit".into())];

        fold_equality_alternative_goto(
            &mut instructions,
            &then_body,
            2,
            &[0],
            &mut skip_body,
            &mut pending_gotos,
        );

        assert_eq!(instructions.len(), 2);
        assert!(skip_body.is_empty());
        assert_eq!(
            pending_gotos,
            vec![(0, "exit".into()), (1, "exit".into())]
        );
        assert!(matches!(
            instructions[1],
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            }
        ));
    }
}
