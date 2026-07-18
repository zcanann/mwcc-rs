//! Build-profile lowering for implicit integer assignment conversions consumed by narrow stores.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fold the C assignment conversion of an out-of-range integer constant before
    /// materializing it. Restrict this to values that actually change so ordinary
    /// in-range constant-store runs keep their existing reuse and scheduling paths.
    pub(crate) fn try_place_converted_narrow_store_constant(
        &mut self,
        value: &Expression,
        pointee: Pointee,
    ) -> Option<u8> {
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
