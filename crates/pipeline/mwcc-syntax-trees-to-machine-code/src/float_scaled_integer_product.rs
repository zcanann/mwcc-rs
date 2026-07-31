//! Scaled floating products with one integer factor.
//!
//! In `scale * (floating * integer)`, legacy MWCC keeps the scale live in f2,
//! converts the integer through f1/f0, forms the inner product in f0, and then
//! writes the scaled result.  Treating the tree as two ordinary binary nodes
//! loses both the integer promotion and the three overlapping FPR lifetimes.

use crate::casts::IntToFloatSchedule;
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn try_emit_scaled_integer_product(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Multiply
            || double
            || self.behavior.optimization != Optimization::O0
        {
            return Ok(false);
        }
        let Some((scale, product)) = scaled_product(left, right) else {
            return Ok(false);
        };
        let Some((floating, integer)) = mixed_product(product, |factor| {
            self.is_float_leaf(factor) || matches!(factor, Expression::FloatLiteral(_))
        }) else {
            return Ok(false);
        };
        let leaf = self.leaf_info(integer).ok();
        let (width, signed) = leaf
            .map(|(_, width, signed)| (width, signed))
            .unwrap_or((32, self.signedness_of(integer)?));
        if !signed {
            return Ok(false);
        }

        let literal_factor = matches!(floating, Expression::FloatLiteral(_));
        let scale_register = self
            .fresh_virtual_float_preferring(if literal_factor { 3 } else { 2 });
        self.load_float_literal(scale_register, scale, false);

        let floating_register = if let Expression::FloatLiteral(value) = floating {
            let register = self.fresh_virtual_float_preferring(2);
            self.load_float_literal(register, *value, false);
            register
        } else {
            self.float_register_of_leaf(floating)?
        };

        let conversion_source = if let Some((integer_source, _, _)) = leaf {
            if width >= 32 {
                integer_source
            } else {
                let widened = self.fresh_virtual_general_preferring(0);
                self.emit_widen(widened, integer_source, width, true);
                widened
            }
        } else {
            let computed = self.fresh_virtual_general_preferring(0);
            self.evaluate_general(integer, computed)?;
            computed
        };
        let promoted = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
        let bias = self.fresh_virtual_float_preferring(1);
        let scratch = self.claim_int_to_float_scratch()?;
        self.emit_int_to_float_body_at(
            conversion_source,
            promoted,
            false,
            true,
            bias,
            IntToFloatSchedule::LeafValue,
            scratch,
        );
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: promoted,
                a: floating_register,
                c: promoted,
            });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: destination,
                a: scale_register,
                c: promoted,
            });
        Ok(true)
    }
}

fn scaled_product<'e>(
    left: &'e Expression,
    right: &'e Expression,
) -> Option<(f64, &'e Expression)> {
    match (left, right) {
        (Expression::FloatLiteral(scale), product) | (product, Expression::FloatLiteral(scale))
            if matches!(
                product,
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    ..
                }
            ) =>
        {
            Some((*scale, product))
        }
        _ => None,
    }
}

fn mixed_product<'e>(
    expression: &'e Expression,
    is_float: impl Fn(&Expression) -> bool,
) -> Option<(&'e Expression, &'e Expression)> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    match (is_float(left), is_float(right)) {
        (true, false) => Some((left, right)),
        (false, true) => Some((right, left)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{mixed_product, scaled_product};
    use mwcc_syntax_trees::{BinaryOperator, Expression};

    fn multiply(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn recognizes_a_scaled_mixed_product_in_either_commutative_order() {
        let product = multiply(
            Expression::Variable("floating".into()),
            Expression::Variable("integer".into()),
        );
        let expression = multiply(Expression::FloatLiteral(0.008), product.clone());
        let (scale, nested) = match &expression {
            Expression::Binary { left, right, .. } => {
                scaled_product(left, right).expect("scaled product")
            }
            _ => unreachable!(),
        };
        assert_eq!(scale, 0.008);
        let (floating, integer) = mixed_product(
            nested,
            |factor| matches!(factor, Expression::Variable(name) if name == "floating"),
        )
        .expect("mixed product");
        assert!(matches!(floating, Expression::Variable(name) if name == "floating"));
        assert!(matches!(integer, Expression::Variable(name) if name == "integer"));
    }

    #[test]
    fn recognizes_a_literal_factor_beside_a_computed_integer() {
        let integer = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::IntegerLiteral(4)),
            right: Box::new(Expression::Variable("level".into())),
        };
        let product = multiply(Expression::FloatLiteral(0.5), integer);
        let (floating, integer) = mixed_product(&product, |factor| {
            matches!(factor, Expression::FloatLiteral(_))
        })
        .expect("mixed literal product");
        assert!(matches!(floating, Expression::FloatLiteral(0.5)));
        assert!(matches!(
            integer,
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                ..
            }
        ));
    }
}
