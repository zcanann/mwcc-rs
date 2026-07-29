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

    /// Preserve build 163's implicit signed narrowing before a byte/halfword store.
    ///
    /// C assignment converts the right-hand value to the lvalue's type even when
    /// the parser does not need an explicit [`Expression::Cast`] node. Modern mwcc
    /// observes that `stb`/`sth` already performs that truncation. Build 163's older
    /// pass removes it only for low-bit-preserving binary ALU expressions; wider
    /// leaves, loads, calls, shifts, unary expressions, division, and remainder keep
    /// an `extsb`/`extsh`. Same-width sources and unsigned destinations never need it.
    pub(crate) fn try_place_implicit_narrow_store_value(
        &mut self,
        value: &Expression,
        pointee: Pointee,
    ) -> Compilation<Option<u8>> {
        if self.behavior.narrow_store_conversion_style
            != mwcc_versions::NarrowStoreConversionStyle::PreserveOutsideBinaryAlu
        {
            return Ok(None);
        }

        let target = pointee.element();
        let target_width = target.width();
        if target_width >= 32
            || !self.signed_of(target)
            || self.is_float_value(value)
            || self.is_float_operand(value)
            || constant_value(value).is_some()
            || matches!(value, Expression::Cast { .. })
            || legacy_narrow_store_binary_alu(value)
        {
            return Ok(None);
        }

        if self
            .implicit_store_source_width(value)
            .is_some_and(|source_width| source_width <= target_width)
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
    use super::fold_representable_float_to_integer;

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
}
