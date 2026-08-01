//! Affine floating expressions with an integer variable.
//!
//! At O0, `base + scale * integer` keeps the two literal loads live in f3/f2,
//! promotes the integer through the magic-bias frame into f0 with the bias in
//! f1, and then emits separate multiply/add instructions when contraction is
//! disabled.

use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn try_emit_integer_affine_float(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Add
            || double
            || self.behavior.optimization != Optimization::O0
            || self.behavior.contract_floating_point
        {
            return Ok(false);
        }
        let Some((base, scale, integer)) = integer_affine(left, right) else {
            return Ok(false);
        };
        let Ok((source, width, signed)) = self.leaf_info(integer) else {
            return Ok(false);
        };
        if !signed {
            return Ok(false);
        }

        let base_register = self.fresh_virtual_float_preferring(3);
        self.load_float_literal(base_register, base, false);
        let scale_register = self.fresh_virtual_float_preferring(2);
        self.load_float_literal(scale_register, scale, false);
        let conversion_source = if width < 32 {
            let widened = self.fresh_virtual_general_preferring(0);
            self.emit_widen(widened, source, width, true);
            widened
        } else {
            source
        };
        let promoted = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
        let bias = self.fresh_virtual_float_preferring(1);
        let scratch = self.claim_int_to_float_scratch()?;
        self.emit_preserved_signed_int_to_float_body_at(
            conversion_source,
            promoted,
            bias,
            scratch,
        );
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: promoted,
                a: scale_register,
                c: promoted,
            });
        self.output.instructions.push(Instruction::FloatAddSingle {
            d: destination,
            a: base_register,
            b: promoted,
        });
        Ok(true)
    }
}

fn integer_affine<'e>(
    left: &'e Expression,
    right: &'e Expression,
) -> Option<(f64, f64, &'e Expression)> {
    let Expression::FloatLiteral(base) = left else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: factor_left,
        right: factor_right,
    } = right
    else {
        return None;
    };
    match (factor_left.as_ref(), factor_right.as_ref()) {
        (Expression::FloatLiteral(scale), integer) | (integer, Expression::FloatLiteral(scale)) => {
            Some((*base, *scale, integer))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_literal_affine_integer_expression() {
        let product = Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(Expression::FloatLiteral(100.0)),
            right: Box::new(Expression::Variable("index".into())),
        };
        let (base, scale, integer) = integer_affine(&Expression::FloatLiteral(-400.0), &product)
            .expect("literal affine integer expression");

        assert_eq!(base, -400.0);
        assert_eq!(scale, 100.0);
        assert!(matches!(integer, Expression::Variable(name) if name == "index"));
    }
}
