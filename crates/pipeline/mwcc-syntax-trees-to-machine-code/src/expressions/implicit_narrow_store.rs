//! Build-profile lowering for implicit integer assignment conversions consumed by narrow stores.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fold a floating literal through C's assignment conversion when the
    /// integer destination can represent the truncated value.
    ///
    /// Keeping this query separate from emission lets framed functions omit
    /// the `fctiwz` scratch image at planning time as well as the conversion
    /// instructions at store time.
    pub(crate) fn folded_float_store_constant(
        &self,
        value: &Expression,
        pointee: Pointee,
    ) -> Option<i64> {
        let Expression::FloatLiteral(value) = value else {
            return None;
        };
        let target = pointee.element();
        let width = target.width();
        if width > 32 || matches!(target, Type::Float | Type::Double) {
            return None;
        }
        fold_representable_float_to_integer(*value, width, self.signed_of(target))
    }

    /// Fold the C assignment conversion of an out-of-range integer constant before
    /// materializing it. Restrict this to values that actually change so ordinary
    /// in-range constant-store runs keep their existing reuse and scheduling paths.
    pub(crate) fn try_place_converted_narrow_store_constant(
        &mut self,
        value: &Expression,
        pointee: Pointee,
    ) -> Option<u8> {
        if let Some(constant) = self.folded_float_store_constant(value, pointee) {
            self.load_integer_constant(GENERAL_SCRATCH, constant);
            return Some(GENERAL_SCRATCH);
        }

        let target = pointee.element();
        let width = target.width();
        if width >= 32 {
            return None;
        }
        let constant = constant_value(value)?;
        let modulus = 1i64 << width;
        let mask = modulus - 1;
        let low = constant & mask;
        let converted = if self.signed_of(target) && low >= (1i64 << (width - 1)) {
            low - modulus
        } else {
            low
        };
        if converted == constant {
            return None;
        }
        self.load_integer_constant(GENERAL_SCRATCH, converted);
        Some(GENERAL_SCRATCH)
    }

    /// Preserve an implicit narrowing before a byte/halfword store when the
    /// resolved optimization policy keeps the assignment conversion.
    ///
    /// C assignment converts the right-hand value to the lvalue's type even when
    /// the parser does not need an explicit [`Expression::Cast`] node. Modern mwcc
    /// observes that `stb`/`sth` already performs that truncation. O0/O1 preserve
    /// both signed and unsigned conversions. Build 163's older optimized pass removes
    /// signed conversions only for low-bit-preserving binary ALU expressions.
    pub(crate) fn try_place_implicit_narrow_store_value(
        &mut self,
        value: &Expression,
        pointee: Pointee,
    ) -> Compilation<Option<u8>> {
        let style = self.behavior.narrow_store_conversion_style;
        if style == mwcc_versions::NarrowStoreConversionStyle::ElideRedundantConversion {
            return Ok(None);
        }

        let target = pointee.element();
        let target_width = target.width();
        if target_width >= 32
            || self.is_float_value(value)
            || self.is_float_operand(value)
            || constant_value(value).is_some()
            || matches!(value, Expression::Cast { .. })
            || narrow_memory_step(value)
        {
            return Ok(None);
        }
        if style == mwcc_versions::NarrowStoreConversionStyle::PreserveOutsideBinaryAlu
            && (!self.signed_of(target) || legacy_narrow_store_binary_alu(value))
        {
            return Ok(None);
        }

        let source_width = self.implicit_store_source_width(value);
        if source_width.is_some_and(|source_width| source_width <= target_width)
        {
            return Ok(None);
        }

        self.emit_cast_to_integer(target, value, GENERAL_SCRATCH)?;
        Ok(Some(GENERAL_SCRATCH))
    }

    /// Source width before assignment conversion. Compound expressions normally
    /// undergo integer promotion and therefore return `None` here (treated as an
    /// int-width value); the cases whose declared result can remain narrow are
    /// described explicitly.
    fn implicit_store_source_width(&self, value: &Expression) -> Option<u8> {
        match value {
            Expression::Call { name, .. } => Some(
                self.call_return_types
                    .get(name)
                    .copied()
                    .unwrap_or(Type::Int)
                    .width(),
            ),
            Expression::Assign { target, .. } => {
                self.cast_operand_width(target).map(|width| width as u8)
            }
            Expression::Comma { right, .. } => self.implicit_store_source_width(right),
            _ => self.cast_operand_width(value).map(|width| width as u8),
        }
    }
}

/// A prefix/postfix step can lose its syntax wrapper while its lvalue is
/// canonicalized from a member into an indexed member-address. MWCC still lets
/// the store perform the narrowing and extends only the yielded value afterward.
fn narrow_memory_step(value: &Expression) -> bool {
    let value = match value {
        Expression::IndexedUpdateValue { value } => value.as_ref(),
        value => value,
    };
    matches!(value,
        Expression::Binary {
            operator: BinaryOperator::Add | BinaryOperator::Subtract,
            left,
            right,
        } if matches!(left.as_ref(),
            Expression::Member { .. }
                | Expression::Index { .. }
                | Expression::Dereference { .. }
        ) && constant_value(right) == Some(1)
    )
}

fn fold_representable_float_to_integer(value: f64, width: u8, signed: bool) -> Option<i64> {
    if !value.is_finite() || width == 0 || width > 32 {
        return None;
    }
    let truncated = value.trunc();
    let (minimum, maximum) = if signed {
        let bound = 1i64 << (width - 1);
        (-(bound as f64), (bound - 1) as f64)
    } else {
        (0.0, ((1u64 << width) - 1) as f64)
    };
    (truncated >= minimum && truncated <= maximum).then_some(truncated as i64)
}

/// Build 163's older redundant-conversion pass recognizes only binary operations
/// whose low result bits are independent of the discarded high bits.
pub(super) fn legacy_narrow_store_binary_alu(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Add
                | BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::BitAnd
                | BinaryOperator::BitOr
                | BinaryOperator::BitXor,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::{fold_representable_float_to_integer, narrow_memory_step};
    use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee};

    #[test]
    fn folds_representable_float_assignment_constants_toward_zero() {
        assert_eq!(fold_representable_float_to_integer(0.0, 16, false), Some(0));
        assert_eq!(
            fold_representable_float_to_integer(-12.75, 16, true),
            Some(-12)
        );
        assert_eq!(
            fold_representable_float_to_integer(255.99, 8, false),
            Some(255)
        );
    }

    #[test]
    fn rejects_undefined_or_runtime_float_assignment_conversions() {
        assert_eq!(fold_representable_float_to_integer(-1.0, 16, false), None);
        assert_eq!(fold_representable_float_to_integer(32768.0, 16, true), None);
        assert_eq!(
            fold_representable_float_to_integer(f64::NAN, 16, true),
            None
        );
    }

    #[test]
    fn memory_steps_use_the_narrow_store_as_the_conversion() {
        let value = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::Index {
                base: Box::new(Expression::MemberAddress {
                    base: Box::new(Expression::Variable("p".into())),
                    offset: 112,
                    element: Pointee::Short,
                    index_stride: None,
                }),
                index: Box::new(Expression::IntegerLiteral(2)),
            }),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        assert!(narrow_memory_step(&value));
    }
}
