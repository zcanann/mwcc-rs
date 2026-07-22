//! Source-level bit-field read-modify-write lowering.
//!
//! The frontend retains both the promoted extraction used by reads and the
//! containing storage lvalue. Stores operate on that storage exactly once, merge
//! the new low field bits with `rlwimi`, and write the original unit width back.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_emit_bit_field_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::BitFieldRead {
            storage,
            shift,
            width,
            ..
        } = target
        else {
            return Ok(false);
        };
        let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } = storage.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(base.as_ref(), Expression::Variable(_))
            || !matches!(
                member_type,
                Type::UnsignedChar | Type::UnsignedShort | Type::UnsignedInt
            )
            || *width == 0
            || u16::from(*shift) + u16::from(*width) > u16::from(member_type.width())
            || i16::try_from(*offset).is_err()
        {
            return Ok(false);
        }
        let storage_pointee = pointee_of_type(*member_type)
            .ok_or_else(|| Diagnostic::error("unsupported bit-field storage type"))?;
        let source = self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
        let address = self.member_base_register(base)?;
        self.output.instructions.push(displacement_load(
            storage_pointee,
            GENERAL_SCRATCH,
            address,
            *offset as i16,
        )?);
        // In an ordinary store MWCC starts the memory dependency before an
        // independent constant materialization (`lbz; li; rlwimi; stb`). Owners
        // of larger schedules may hoist the materialization earlier.
        self.evaluate_general(value, source)?;
        let begin = 32 - *shift - *width;
        let end = 31 - *shift;
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: GENERAL_SCRATCH,
                s: source,
                shift: *shift,
                begin,
                end,
            });
        self.output.instructions.push(displacement_store(
            storage_pointee,
            GENERAL_SCRATCH,
            address,
            *offset as i16,
        )?);
        Ok(true)
    }
}
