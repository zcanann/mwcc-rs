//! Versioned lowering for a negated logical operator in tail position.

use super::*;

impl Generator {
    pub(crate) fn emit_negated_short_circuit(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        result: u8,
    ) -> Compilation<()> {
        let is_logical = |expression: &Expression| {
            matches!(
                expression,
                Expression::Binary {
                    operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
                    ..
                }
            )
        };
        if is_logical(left) || is_logical(right) {
            return Err(Diagnostic::error(
                "a nested negated logical needs the general short-circuit (roadmap)",
            ));
        }

        if self.behavior.logical_or_value_style == mwcc_versions::LogicalOrValueStyle::TrueFirst {
            // Build 163 first computes the written logical operator into r0,
            // then applies integer logical-not to that normalized 0/1 value.
            self.emit_short_circuit_via_scratch(
                operator,
                left,
                right,
                GENERAL_SCRATCH,
            )?;
            self.output
                .instructions
                .push(Instruction::CountLeadingZeros {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                });
            self.output
                .instructions
                .push(Instruction::ShiftRightLogicalImmediate {
                    a: result,
                    s: GENERAL_SCRATCH,
                    shift: 5,
                });
            return Ok(());
        }

        // Mainline applies De Morgan and folds the negation into the
        // short-circuit exits instead of materializing an intermediate value.
        let flipped = if operator == BinaryOperator::LogicalAnd {
            BinaryOperator::LogicalOr
        } else {
            BinaryOperator::LogicalAnd
        };
        let not_left = Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(left.clone()),
        };
        let not_right = Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand: Box::new(right.clone()),
        };
        self.emit_short_circuit(flipped, &not_left, &not_right, result)
    }
}
