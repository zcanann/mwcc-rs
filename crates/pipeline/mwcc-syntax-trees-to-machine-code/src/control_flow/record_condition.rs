//! Arithmetic conditions that set CR0 on their final value instruction.
//!
//! mwcc turns a computed truth test into the record form of the expression's
//! final operation when PowerPC provides one. The branch can then consume CR0
//! directly, avoiding a separate `cmpwi` and avoiding materializing 0/1.

use super::*;

impl Generator {
    /// A named signed quotient can feed its guard directly through CR0.
    /// Run after symbolic edges are resolved so an independently reachable
    /// compare is never removed, and every remaining index can be retargeted.
    pub(crate) fn fold_signed_quotient_zero_tests(&mut self) {
        if self.behavior.optimization == mwcc_versions::Optimization::O0
            || !self.output.jump_tables.is_empty()
            || self
                .output
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::VerbatimWord(_)))
        {
            return;
        }
        for start in (0..self.output.instructions.len().saturating_sub(4)).rev() {
            if let Some(record) = quotient_zero_record(&self.output.instructions, start) {
                self.output.instructions[start + 2] = record;
                crate::remove_instruction_retargeting_to_next(self, start + 3);
            }
        }
    }

    /// Emit a signed comparison against zero by recording the arithmetic result.
    ///
    /// PowerPC arithmetic record forms set CR0 from the result itself, so an
    /// expression such as `(member + width) <= 0` needs no following `cmpwi`.
    /// Keep this conversion next to the other record-form selection rather than
    /// teaching condition operand placement to pretend a computed value is a
    /// leaf.
    pub(super) fn try_emit_recorded_arithmetic_result(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        if !matches!(
            expression,
            Expression::Binary {
                operator: BinaryOperator::Add,
                ..
            }
        ) {
            return Ok(false);
        }

        self.evaluate_general(expression, GENERAL_SCRATCH)?;
        let Some(last) = self.output.instructions.last_mut() else {
            return Err(Diagnostic::error(
                "computed arithmetic condition emitted no result instruction",
            ));
        };
        let replacement = match *last {
            Instruction::Add { d, a, b } => Some(Instruction::AddRecord { d, a, b }),
            _ => None,
        };
        if let Some(record) = replacement {
            *last = record;
            Ok(true)
        } else {
            Err(Diagnostic::error(
                "a computed add comparison did not end in a recordable add",
            ))
        }
    }

    pub(super) fn try_emit_computed_record_condition(
        &mut self,
        condition: &Expression,
    ) -> Compilation<bool> {
        // A member address used for truth (`if (&p->member)`) folds address
        // formation and the CR0 test into `addic.`. Assertion macros expose this
        // after preprocessing as the condition of a discarded ternary.
        let member_address = match condition {
            Expression::AddressOf { operand } => match operand.as_ref() {
                Expression::Member {
                    base,
                    offset,
                    index_stride: None,
                    ..
                } => Some((base.as_ref(), *offset)),
                _ => None,
            },
            Expression::MemberAddress {
                base,
                offset,
                index_stride: None,
                ..
            } => Some((base.as_ref(), *offset)),
            _ => None,
        };
        if let Some((base, offset)) = member_address {
            if let (Some(base), Ok(immediate)) = (
                leaf_name(base).and_then(|name| self.lookup_general(name)),
                i16::try_from(offset as i64),
            ) {
                self.output
                    .instructions
                    .push(Instruction::AddImmediateCarryingRecord {
                        d: GENERAL_SCRATCH,
                        a: base,
                        immediate,
                    });
                return Ok(true);
            }
        }
        let multiply = matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                ..
            }
        );
        let shifted_mask = is_shifted_mask_truth_test(condition);
        if !multiply && !shifted_mask {
            return Ok(false);
        }

        self.evaluate_general(condition, GENERAL_SCRATCH)?;
        let Some(last) = self.output.instructions.last_mut() else {
            return Ok(false);
        };
        let replacement = match *last {
            Instruction::MultiplyLow { d, a, b } => {
                Some(Instruction::MultiplyLowRecord { d, a, b })
            }
            Instruction::RotateAndMask {
                a,
                s,
                shift,
                begin,
                end,
            } if shifted_mask => Some(Instruction::RotateAndMaskRecord {
                a,
                s,
                shift,
                begin,
                end,
            }),
            _ => None,
        };
        if let Some(record) = replacement {
            *last = record;
            Ok(true)
        } else {
            Err(Diagnostic::error(
                "a computed condition did not end in its expected recordable operation",
            ))
        }
    }
}

fn quotient_zero_record(instructions: &[Instruction], start: usize) -> Option<Instruction> {
    let [Instruction::ShiftRightAlgebraicImmediate { a: quotient, .. }, Instruction::ShiftRightLogicalImmediate {
        a: correction,
        s: sign_source,
        shift: 31,
    }, Instruction::Add { d, a, b }, Instruction::CompareWordImmediate {
        a: compared,
        immediate: 0,
    }, Instruction::BranchConditionalForward {
        options: 4 | 12,
        condition_bit: 2,
        ..
    }] = instructions.get(start..start + 5)?
    else {
        return None;
    };
    if quotient != sign_source
        || a != quotient
        || b != correction
        || d != compared
        || instructions.iter().any(|instruction| {
            matches!(instruction,
            Instruction::Branch { target } | Instruction::BranchConditionalForward { target, .. }
                if *target == start + 3)
        })
    {
        return None;
    }
    Some(Instruction::AddRecord {
        d: *d,
        a: *a,
        b: *b,
    })
}

/// A shifted value subsequently narrowed by a constant mask lowers to one
/// `rlwinm`. In truth position the same instruction can set CR0 directly.
fn is_shifted_mask_truth_test(expression: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return false;
    };
    constant_value(right).is_some()
        && matches!(
            left.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight,
                ..
            }
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotient_record_requires_the_same_value_and_no_entry_at_the_compare() {
        let mut instructions = vec![
            Instruction::ShiftRightAlgebraicImmediate {
                a: 0,
                s: 0,
                shift: 6,
            },
            Instruction::ShiftRightLogicalImmediate {
                a: 4,
                s: 0,
                shift: 31,
            },
            Instruction::Add { d: 4, a: 0, b: 4 },
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
        ];
        assert!(matches!(
            quotient_zero_record(&instructions, 0),
            Some(Instruction::AddRecord { d: 4, a: 0, b: 4 })
        ));
        instructions.push(Instruction::Branch { target: 3 });
        assert!(quotient_zero_record(&instructions, 0).is_none());
        instructions.pop();
        instructions[3] = Instruction::CompareWordImmediate { a: 5, immediate: 0 };
        assert!(quotient_zero_record(&instructions, 0).is_none());
    }

    #[test]
    fn recognizes_a_constant_mask_of_a_shifted_value() {
        let expression = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                left: Box::new(Expression::Variable("bits".into())),
                right: Box::new(Expression::IntegerLiteral(2)),
            }),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        assert!(is_shifted_mask_truth_test(&expression));
    }

    #[test]
    fn leaves_an_unshifted_mask_to_the_direct_mask_owner() {
        let expression = Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(Expression::Variable("bits".into())),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        assert!(!is_shifted_mask_truth_test(&expression));
    }
}
