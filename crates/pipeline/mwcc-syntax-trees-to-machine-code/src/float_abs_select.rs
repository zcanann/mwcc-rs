//! Source-level `ABS` diamonds for floating values.
//!
//! The frontend preserves the macro as `x < 0 ? -x : x`. This owner recognizes
//! that single-evaluation shape, materializes memory-backed operands once, and
//! emits the shared compare/negate/move diamond. Pairwise comparison scheduling
//! builds on the same primitive in `float_abs_pair_condition`.

use crate::analysis::{is_zero_literal, positive_branch, structurally_equal};
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};

impl Generator {
    pub(crate) fn try_emit_float_abs_select(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(value) = abs_select_value(expression) else {
            return Ok(false);
        };
        let source = if self.is_float_located(value) {
            let double = self.is_double_value(value);
            // MWCC materializes the comparison zero before an independent
            // memory operand. Keeping the literal live in f0 also lets the
            // following diamond reuse the single load.
            self.load_float_literal_into(
                FLOAT_SCRATCH,
                &Expression::IntegerLiteral(0),
                double,
            )?;
            let source = if destination == FLOAT_SCRATCH {
                self.fresh_virtual_float_preferring(1)
            } else {
                destination
            };
            let source = self.place_condition_float_load(value, source)?;
            self.emit_float_abs_select_after_zero(source, destination)?;
            return Ok(true);
        } else if self.is_float_leaf(value) {
            self.float_register_of_leaf(value)?
        } else {
            return Ok(false);
        };
        self.emit_float_abs_select(source, destination, self.is_double_value(value))?;
        Ok(true)
    }

    pub(crate) fn emit_float_abs_select(
        &mut self,
        source: u8,
        destination: u8,
        double: bool,
    ) -> Compilation<()> {
        self.load_float_literal_into(
            FLOAT_SCRATCH,
            &Expression::IntegerLiteral(0),
            double,
        )?;
        self.emit_float_abs_select_after_zero(source, destination)
    }

    fn emit_float_abs_select_after_zero(
        &mut self,
        source: u8,
        destination: u8,
    ) -> Compilation<()> {
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered {
                a: source,
                b: FLOAT_SCRATCH,
            });
        let (less_options, condition_bit) = positive_branch(BinaryOperator::Less);
        if source == destination {
            // The nonnegative arm is already in its final register. MWCC emits
            // a one-arm conditional here (`bge done; fneg fD,fD`) instead of
            // retaining the general move/join diamond after the self-move is
            // eliminated.
            let done = self.fresh_label();
            self.emit_branch_conditional_to(less_options ^ 8, condition_bit, done);
            self.output.instructions.push(Instruction::FloatNegate {
                d: destination,
                b: source,
            });
            self.bind_label(done);
            return Ok(());
        }
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

pub(crate) fn abs_select_value(expression: &Expression) -> Option<&Expression> {
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
