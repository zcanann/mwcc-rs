//! Floating comparisons between a computed value and a direct memory value.
//!
//! The computed side is consumed immediately by `fcmp*`, so MWCC gives it the
//! volatile `f0` scratch.  The independent memory side joins register
//! allocation: it may reuse a dead parameter home, while live parameters and
//! retained condition values remain pinned.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};

impl Generator {
    /// Combine two direct float values already loaded by earlier terms in the
    /// same short-circuit path. Extending those proven lifetimes lets allocation
    /// retain the shared zero and coalesce the dead left operand with the sum.
    pub(crate) fn try_place_cached_condition_arithmetic(
        &mut self,
        expression: &Expression,
    ) -> Option<u8> {
        let (operator, left, right) = cached_condition_arithmetic_parts(expression)?;
        let left_register = self.condition_float_register(left)?;
        let right_register = self.condition_float_register(right)?;
        let destination = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
        let double = self.is_double_value(left) || self.is_double_value(right);
        let instruction = match (operator, double) {
            (BinaryOperator::Add, false) => Instruction::FloatAddSingle {
                d: destination,
                a: left_register,
                b: right_register,
            },
            (BinaryOperator::Add, true) => Instruction::FloatAddDouble {
                d: destination,
                a: left_register,
                b: right_register,
            },
            (BinaryOperator::Subtract, false) => Instruction::FloatSubtractSingle {
                d: destination,
                a: left_register,
                b: right_register,
            },
            (BinaryOperator::Subtract, true) => Instruction::FloatSubtractDouble {
                d: destination,
                a: left_register,
                b: right_register,
            },
            _ => return None,
        };
        self.output.instructions.push(instruction);
        Some(destination)
    }

    /// Place two direct memory values without borrowing a live f1 argument.
    /// MWCC keeps the source-left value in the next available FPR and uses f0
    /// for the source-right value consumed immediately by the comparison.
    pub(crate) fn try_place_loaded_pair_with_live_float_argument(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if !self.f1_holds_float_argument()
            || !self.is_float_located(left)
            || !self.is_float_located(right)
        {
            return Ok(None);
        }
        let left_home = self.fresh_virtual_float_preferring(2);
        let a = self.place_condition_float_load(left, left_home)?;
        let b = self.with_reserved_inputs(left, |generator| {
            generator.place_condition_float_load(right, FLOAT_SCRATCH)
        })?;
        Ok(Some((a, b)))
    }

    /// Place `loaded < -loaded` while a float argument may keep f1 live.
    /// The plain comparison path historically hard-pinned the first memory
    /// value to f1; joining the loaded side to virtual allocation lets the
    /// argument survive and leaves f0 available for the negated value.
    pub(crate) fn try_place_loaded_left_negated_loaded_float_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } = right
        else {
            return Ok(None);
        };
        if !self.f1_holds_float_argument()
            || !self.is_float_located(left)
            || !self.is_float_located(operand)
        {
            return Ok(None);
        }

        let loaded_home = self.fresh_virtual_float_preferring(2);
        let loaded = self.place_condition_float_load(left, loaded_home)?;
        self.with_reserved_inputs(left, |generator| {
            generator.place_condition_float_load(operand, FLOAT_SCRATCH)
        })?;
        self.output.instructions.push(Instruction::FloatNegate {
            d: FLOAT_SCRATCH,
            b: FLOAT_SCRATCH,
        });
        Ok(Some((loaded, FLOAT_SCRATCH)))
    }

    pub(crate) fn try_place_loaded_literal_with_live_float_argument(
        &mut self,
        loaded: &Expression,
        literal: &Expression,
        double: bool,
    ) -> Compilation<Option<(u8, u8)>> {
        if !self.behavior.float_compare_value_before_const
            || !self.f1_holds_float_argument()
            || !self.is_float_located(loaded)
            || !matches!(
                literal,
                Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
            )
        {
            return Ok(None);
        }

        let value = self.place_condition_float_load(loaded, FLOAT_SCRATCH)?;
        let literal_home = self.fresh_virtual_float_preferring(2);
        self.load_float_literal_into(literal_home, literal, double)?;
        Ok(Some((value, literal_home)))
    }

    pub(crate) fn try_place_loaded_left_negated_leaf_float_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } = right
        else {
            return Ok(None);
        };
        if !self.is_float_located(left) || !self.is_float_leaf(operand) {
            return Ok(None);
        }

        self.evaluate_float(right, FLOAT_SCRATCH)?;
        let loaded_home = self.fresh_virtual_float_preferring(2);
        let loaded = self.place_condition_float_load(left, loaded_home)?;
        Ok(Some((loaded, FLOAT_SCRATCH)))
    }

    pub(crate) fn try_place_computed_left_loaded_float_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        if !matches!(
            left,
            Expression::Binary { .. }
                | Expression::Unary { .. }
                | Expression::Cast { .. }
                | Expression::Conditional { .. }
        ) {
            return Ok(None);
        }

        if let Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } = right
        {
            if self.is_float_located(operand) {
                self.place_condition_float_load(operand, FLOAT_SCRATCH)?;
                let computed = self.fresh_virtual_float_preferring(1);
                self.evaluate_float(left, computed)?;
                self.output.instructions.push(Instruction::FloatNegate {
                    d: FLOAT_SCRATCH,
                    b: FLOAT_SCRATCH,
                });
                return Ok(Some((computed, FLOAT_SCRATCH)));
            }
        }

        if !self.is_float_located(right) {
            return Ok(None);
        }
        self.evaluate_float(left, FLOAT_SCRATCH)?;
        let loaded_home = self.fresh_virtual_float_preferring(1);
        let loaded = self.place_condition_float_load(right, loaded_home)?;
        Ok(Some((FLOAT_SCRATCH, loaded)))
    }
}

fn cached_condition_arithmetic_parts(
    expression: &Expression,
) -> Option<(BinaryOperator, &Expression, &Expression)> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
        && crate::condition_float_cache::is_direct_float_memory_load(left)
        && crate::condition_float_cache::is_direct_float_memory_load(right))
    .then_some((*operator, left.as_ref(), right.as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    #[test]
    fn recognizes_arithmetic_over_two_direct_condition_values() {
        let member = |offset| Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset,
            member_type: Type::Float,
            index_stride: None,
        };
        let expression = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(member(24)),
            right: Box::new(member(28)),
        };

        assert!(cached_condition_arithmetic_parts(&expression).is_some());
    }
}
