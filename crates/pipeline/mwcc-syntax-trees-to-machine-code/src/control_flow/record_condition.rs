//! Arithmetic conditions that set CR0 on their final value instruction.
//!
//! mwcc turns a computed truth test into the record form of the expression's
//! final operation when PowerPC provides one. The branch can then consume CR0
//! directly, avoiding a separate `cmpwi` and avoiding materializing 0/1.

use super::*;

impl Generator {
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
