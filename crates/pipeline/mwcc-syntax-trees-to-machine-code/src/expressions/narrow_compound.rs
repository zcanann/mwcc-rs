//! Generation-specific compound updates to narrow global values.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Build 163 lowers `narrow_global >>= C` through a materialized count even
    /// though `C` is an immediate: load the promoted value into r3, put C in r0,
    /// use `sraw`, then explicitly re-narrow signed destinations.
    pub(super) fn try_legacy_narrow_global_compound_shift(
        &mut self,
        name: &str,
        pointee: Pointee,
        value: &Expression,
    ) -> Compilation<Option<u8>> {
        if self.behavior.narrow_compound_shift_style
            != mwcc_versions::NarrowCompoundShiftStyle::MaterializedCount
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || !matches!(
                pointee,
                Pointee::Char | Pointee::UnsignedChar | Pointee::Short | Pointee::UnsignedShort
            )
        {
            return Ok(None);
        }
        let Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left,
            right,
        } = value
        else {
            return Ok(None);
        };
        if !matches!(left.as_ref(), Expression::Variable(left_name) if left_name == name) {
            return Ok(None);
        }
        let Some(amount) = constant_value(right).and_then(|value| u8::try_from(value).ok()) else {
            return Ok(None);
        };
        if !(1..=31).contains(&amount) {
            return Ok(None);
        }

        self.record_relocation(RelocationKind::EmbSda21, name);
        self.output
            .instructions
            .push(displacement_load(pointee, 3, 0, 0)?);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, amount as i16));
        if pointee == Pointee::Char {
            self.output
                .instructions
                .push(Instruction::ExtendSignByte { a: 3, s: 3 });
        }
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicWord { a: 0, s: 3, b: 0 });
        match pointee {
            Pointee::Char => self
                .output
                .instructions
                .push(Instruction::ExtendSignByte { a: 0, s: 0 }),
            Pointee::Short => self
                .output
                .instructions
                .push(Instruction::ExtendSignHalfword { a: 0, s: 0 }),
            _ => {}
        }
        Ok(Some(0))
    }
}
