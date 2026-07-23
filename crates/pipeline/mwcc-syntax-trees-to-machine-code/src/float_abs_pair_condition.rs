//! Comparison of two source-level floating absolute-value selects.
//!
//! MWCC evaluates the memory-backed ABS first, retaining both its raw value and
//! selected result, then evaluates the register-backed ABS into `f0`. This is a
//! small dependency schedule shared by friction/clamp code and kept separate
//! from ordinary comparison placement.

use crate::analysis::{is_zero_literal, positive_branch, structurally_equal};
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};

impl Generator {
    pub(crate) fn try_place_float_abs_pair_condition(
        &mut self,
        left: &Expression,
        right: &Expression,
        double: bool,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some(left_value) = abs_select_value(left) else {
            return Ok(None);
        };
        let Some(right_value) = abs_select_value(right) else {
            return Ok(None);
        };
        if !self.is_float_leaf(left_value) || !self.is_float_located(right_value) {
            return Ok(None);
        }

        let right_source_home = self.fresh_virtual_float_preferring(3);
        let right_source =
            self.place_condition_float_load(right_value, right_source_home)?;
        let right_result = self.fresh_virtual_float_preferring(2);
        self.emit_abs_select(right_source, right_result, double)?;

        let left_source = self.float_register_of_leaf(left_value)?;
        self.emit_abs_select(left_source, FLOAT_SCRATCH, double)?;
        Ok(Some((FLOAT_SCRATCH, right_result)))
    }

    fn emit_abs_select(&mut self, source: u8, destination: u8, double: bool) -> Compilation<()> {
        self.load_float_literal_into(
            FLOAT_SCRATCH,
            &Expression::IntegerLiteral(0),
            double,
        )?;
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: source,
                b: FLOAT_SCRATCH,
            });
        let (less_options, condition_bit) = positive_branch(BinaryOperator::Less);
        let nonnegative = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(less_options ^ 8, condition_bit, nonnegative);
        self.output.instructions.push(Instruction::FloatNegate {
            d: destination,
            b: source,
        });
        self.emit_branch_to(join);
        self.bind_label(nonnegative);
        self.output.instructions.push(Instruction::FloatMove {
            d: destination,
            b: source,
        });
        self.bind_label(join);
        Ok(())
    }
}

fn abs_select_value(expression: &Expression) -> Option<&Expression> {
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition.as_ref()
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::Negate,
        operand,
    } = when_true.as_ref()
    else {
        return None;
    };
    (is_zero_literal(right)
        && structurally_equal(left, operand)
        && structurally_equal(left, when_false))
    .then_some(left)
}
