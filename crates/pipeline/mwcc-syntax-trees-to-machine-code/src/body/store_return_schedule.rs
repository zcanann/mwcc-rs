//! Cross-statement scheduling between a pointer store and its trailing return.
//!
//! Build 163 moves the first independent comparison instruction ahead of a
//! single `*p = value` store. This pass runs on virtual instructions before
//! allocation, so the allocator observes the extended live range and selects
//! the same non-pointer temporary as mwcc.

use super::*;

impl Generator {
    pub(super) fn schedule_legacy_single_pointer_store_return(
        &mut self,
        function: &Function,
        statements_start: usize,
        return_start: usize,
    ) {
        if self.behavior.integer_comparison_value_style
            != IntegerComparisonValueStyle::LegacyCarryChain
            || statements_start >= return_start
            || return_start >= self.output.instructions.len()
        {
            return;
        }

        let [Statement::Store {
            target: Expression::Dereference { .. },
            value: Expression::Variable(stored),
        }] = function.statements.as_slice()
        else {
            return;
        };
        let Some(Expression::Binary {
            operator,
            left,
            right,
        }) = function.return_expression.as_ref()
        else {
            return;
        };
        if !matches!(
            operator,
            BinaryOperator::Less | BinaryOperator::GreaterEqual | BinaryOperator::Equal
        ) || !is_zero_literal(right)
            || !matches!(left.as_ref(), Expression::Variable(name) if name == stored)
        {
            return;
        }

        let first = &self.output.instructions[return_start];
        let independent_preamble = match operator {
            BinaryOperator::Less | BinaryOperator::GreaterEqual => {
                matches!(
                    first,
                    Instruction::AddImmediate {
                        a: 0,
                        immediate: 0,
                        ..
                    }
                )
            }
            BinaryOperator::Equal => matches!(first, Instruction::Negate { .. }),
            _ => false,
        };
        if independent_preamble {
            let instruction = self.output.instructions.remove(return_start);
            self.output
                .instructions
                .insert(statements_start, instruction);
        }
    }
}
