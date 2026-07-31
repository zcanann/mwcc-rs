//! Products between an integer factor and an integer-derived fraction.
//!
//! In `integer * (integer / floating)`, MWCC promotes the outer factor before
//! evaluating the fraction. This keeps that factor in f2 while the numerator
//! crosses through f1 and the divisor occupies f0. Treating the tree as two
//! independent mixed binary expressions reverses both conversion images and
//! creates an unnecessary GPR copy for a live outer operand.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn try_emit_integer_scaled_fraction(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Multiply
            || double
            || destination != FLOAT_SCRATCH
            || !self.non_leaf
            || self.behavior.optimization != Optimization::O0
        {
            return Ok(false);
        }
        let Some((factor, numerator, divisor)) =
            integer_scaled_fraction(left, right, |value| self.is_float_value(value))
        else {
            return Ok(false);
        };
        let Ok((factor_source, factor_width, factor_signed)) = self.leaf_info(factor) else {
            return Ok(false);
        };
        if factor_width < 32 || !factor_signed || !self.signedness_of(numerator)? {
            return Ok(false);
        }

        // Materialize the numerator before either promotion. Composed inline
        // transactions expose their source/caller images as ordinary locals;
        // other expressions use the general conversion operand planner.
        let numerator_source = self.materialize_integer_conversion_operand(numerator)?;

        let factor_value = self.fresh_virtual_float_preferring(2);
        let factor_bias = self.fresh_virtual_float_preferring(1);
        let factor_scratch = self.claim_int_to_float_scratch()?;
        self.emit_preserved_signed_int_to_float_body_at(
            factor_source,
            factor_value,
            factor_bias,
            factor_scratch,
        );

        let fraction_value = self.fresh_virtual_float_preferring(1);
        let numerator_scratch = self.claim_int_to_float_scratch()?;
        self.emit_preserved_signed_int_to_float_body_at(
            numerator_source,
            fraction_value,
            fraction_value,
            numerator_scratch,
        );

        self.load_float_literal(FLOAT_SCRATCH, divisor, false);
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle {
                d: FLOAT_SCRATCH,
                a: fraction_value,
                b: FLOAT_SCRATCH,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: destination,
                a: factor_value,
                c: FLOAT_SCRATCH,
            });
        Ok(true)
    }
}

fn integer_scaled_fraction<'e>(
    left: &'e Expression,
    right: &'e Expression,
    is_float: impl Fn(&Expression) -> bool,
) -> Option<(&'e Expression, &'e Expression, f64)> {
    let (factor, fraction) = match (is_float(left), is_float(right)) {
        (false, true) => (left, right),
        (true, false) => (right, left),
        _ => return None,
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: numerator,
        right: divisor,
    } = fraction
    else {
        return None;
    };
    let Expression::FloatLiteral(divisor) = divisor.as_ref() else {
        return None;
    };
    (!is_float(numerator)).then_some((factor, numerator, *divisor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_integer_factor_over_a_literal_scaled_integer() {
        let fraction = Expression::Binary {
            operator: BinaryOperator::Divide,
            left: Box::new(Expression::Variable("random".into())),
            right: Box::new(Expression::FloatLiteral(65536.0)),
        };
        let factor = Expression::Variable("count".into());
        let (_, numerator, divisor) = integer_scaled_fraction(&factor, &fraction, |value| {
            matches!(value, Expression::FloatLiteral(_))
                || matches!(
                    value,
                    Expression::Binary {
                        operator: BinaryOperator::Divide,
                        ..
                    }
                )
        })
        .expect("integer-scaled fraction");

        assert!(matches!(numerator, Expression::Variable(name) if name == "random"));
        assert_eq!(divisor, 65536.0);
    }
}
