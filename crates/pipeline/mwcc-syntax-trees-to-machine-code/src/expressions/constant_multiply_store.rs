//! Constant multiplication and narrow assignment conversion phase ordering.
//!
//! Older MWCC removes conversions from raw products, but keeps them after the
//! early identity, negation, and positive-power-of-two rewrites. Negative powers
//! other than -1 are reduced later and keep the raw product's conversion policy.

use super::*;
use mwcc_versions::{NarrowStoreConversionStyle, Optimization};

impl Generator {
    pub(super) fn try_place_constant_multiply_narrow_store(
        &mut self,
        value: &Expression,
        pointee: Pointee,
    ) -> Compilation<Option<u8>> {
        if !matches!(
            pointee,
            Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort
        ) {
            return Ok(None);
        }
        let target = pointee.element();
        let product = match value {
            Expression::Cast {
                target_type,
                operand,
            } if *target_type == target => operand.as_ref(),
            value => value,
        };
        let Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } = product
        else {
            return Ok(None);
        };
        let (operand, factor) = match (constant_value(left), constant_value(right)) {
            (None, Some(factor)) => (left.as_ref(), factor),
            (Some(factor), None) => (right.as_ref(), factor),
            _ => return Ok(None),
        };
        if !(-32768..=32767).contains(&factor) || !self.word_multiply_store_operand(operand) {
            return Ok(None);
        }
        if factor == 0 {
            // A register leaf has no observable read. Leave memory/compound
            // operands to ordinary evaluation so this fold cannot erase reads.
            if !matches!(operand, Expression::Variable(name) if self.lookup_general(name).is_some() && !self.frame_slots.contains_key(name))
            {
                return Ok(None);
            }
            self.load_integer_constant(GENERAL_SCRATCH, 0);
            return Ok(Some(GENERAL_SCRATCH));
        }
        let signed = self.signedness_of(operand)?;
        let optimized = self.behavior.optimization != Optimization::O0;
        let extend = preserves_product_conversion(
            self.behavior.constant_multiply_store_conversion_style,
            factor,
            signed,
            self.signed_of(target),
            optimized,
        );
        let source = if factor == 1 {
            self.place_operand_or_scratch(operand, GENERAL_SCRATCH)?
        } else if self.behavior.small_constant_multiply_style
            == mwcc_versions::SmallConstantMultiplyStyle::ShiftSubtract
            && (factor.unsigned_abs() + 1).is_power_of_two()
            && (factor != -1 || !optimized || !signed)
        {
            let home = self.fresh_virtual_general();
            let inputs = self.registers_used_by(operand);
            if let Some(preferred) =
                (3..=12).find(|r| !self.reserved.contains(r) && !inputs.contains(r))
            {
                self.prefer_virtual_general(home, preferred);
            }
            // The subtraction still needs the unshifted operand. Compound
            // expressions therefore need their own home instead of r0.
            let source = match self.place_operand(operand, home, true)? {
                Some(source) => source,
                None => {
                    self.evaluate_general(operand, home)?;
                    home
                }
            };
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: source,
                    shift: (factor.unsigned_abs() + 1).trailing_zeros() as u8,
                });
            let (a, b) = if factor < 0 {
                (GENERAL_SCRATCH, source)
            } else {
                (source, GENERAL_SCRATCH)
            };
            self.output.instructions.push(Instruction::SubtractFrom {
                d: GENERAL_SCRATCH,
                a,
                b,
            });
            GENERAL_SCRATCH
        } else if factor == -1 && (!optimized || !signed) {
            // The early negation rewrite applies only to optimized signed
            // products. Immediate-multiply profiles retain an unsigned -1 factor.
            let source = self.place_operand_or_scratch(operand, GENERAL_SCRATCH)?;
            self.output
                .instructions
                .push(Instruction::MultiplyImmediate {
                    d: GENERAL_SCRATCH,
                    a: source,
                    immediate: -1,
                });
            GENERAL_SCRATCH
        } else {
            self.evaluate_general(product, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        };
        if extend {
            self.emit_widen(
                GENERAL_SCRATCH,
                source,
                target.width(),
                self.signed_of(target),
            );
            Ok(Some(GENERAL_SCRATCH))
        } else {
            Ok(Some(source))
        }
    }

    fn word_multiply_store_operand(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(name) => {
                self.locations.get(name).is_some_and(|location| {
                    location.class == ValueClass::General
                        && location.width == 32
                        && location.pointee.is_none()
                }) || matches!(self.globals.get(name), Some(Type::Int | Type::UnsignedInt))
            }
            Expression::IntegerLiteral(value) => {
                (i32::MIN as i64..=u32::MAX as i64).contains(value)
            }
            Expression::Binary { left, right, .. } => {
                self.word_multiply_store_operand(left) && self.word_multiply_store_operand(right)
            }
            Expression::Unary { operand, .. } => self.word_multiply_store_operand(operand),
            _ => false,
        }
    }
}

fn preserves_product_conversion(
    style: NarrowStoreConversionStyle,
    factor: i64,
    source_signed: bool,
    target_signed: bool,
    optimized: bool,
) -> bool {
    match style {
        NarrowStoreConversionStyle::ElideRedundantConversion => false,
        NarrowStoreConversionStyle::PreserveAll => true,
        NarrowStoreConversionStyle::PreserveOutsideBinaryAlu => {
            target_signed
                && (factor == 1
                    || optimized
                        && (factor == -1 && source_signed
                            || factor >= 2 && (factor as u64).is_power_of_two()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_conversion_distinguishes_early_rewrites_from_late_products() {
        let style = NarrowStoreConversionStyle::PreserveOutsideBinaryAlu;
        for factor in [-8, -3, -2, -1, 1, 2, 3, 8] {
            assert_eq!(
                preserves_product_conversion(style, factor, true, true, false),
                factor == 1
            );
            assert_eq!(
                preserves_product_conversion(style, factor, true, true, true),
                [-1, 1, 2, 8].contains(&factor)
            );
            assert!(!preserves_product_conversion(
                style, factor, true, false, true
            ));
        }
        assert!(!preserves_product_conversion(style, -1, false, true, true));
    }

    #[test]
    fn modern_conversion_policy_is_independent_of_the_reduced_operation() {
        for factor in [-8, -3, -2, -1, 1, 2, 3, 8] {
            for signed in [false, true] {
                assert!(preserves_product_conversion(
                    NarrowStoreConversionStyle::PreserveAll,
                    factor,
                    signed,
                    signed,
                    false
                ));
                assert!(!preserves_product_conversion(
                    NarrowStoreConversionStyle::ElideRedundantConversion,
                    factor,
                    signed,
                    signed,
                    true
                ));
            }
        }
    }
}
