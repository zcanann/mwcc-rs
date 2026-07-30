//! Source-level bit-field read-modify-write lowering.
//!
//! The frontend retains both the promoted extraction used by reads and the
//! containing storage lvalue. Stores operate on that storage exactly once, merge
//! the new low field bits with `rlwimi`, and write the original unit width back.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit a bit-field assignment expression, preserving the converted field
    /// value in `destination` for a surrounding chained assignment.
    pub(crate) fn try_emit_bit_field_assign(
        &mut self,
        target: &Expression,
        value: &Expression,
        destination: u8,
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
            || destination == GENERAL_SCRATCH
        {
            return Ok(false);
        }

        let storage_pointee = pointee_of_type(*member_type)
            .ok_or_else(|| Diagnostic::error("unsupported bit-field storage type"))?;
        let address = self.member_base_register(base)?;
        self.output.instructions.push(displacement_load(
            storage_pointee,
            GENERAL_SCRATCH,
            address,
            *offset as i16,
        )?);
        self.evaluate_general(value, destination)?;
        let begin = 32 - *shift - *width;
        let end = 31 - *shift;
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: GENERAL_SCRATCH,
                s: destination,
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
        self.output.instructions.push(Instruction::RotateAndMask {
            a: destination,
            s: GENERAL_SCRATCH,
            shift: (32 - *shift) % 32,
            begin: 32 - *width,
            end: 31,
        });
        Ok(true)
    }

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
        if let Expression::BitFieldRead {
            storage: source_storage,
            shift: source_shift,
            width: source_width,
            ..
        } = value
        {
            if width == source_width {
                if u16::from(*source_shift) + u16::from(*source_width)
                    <= u16::from(member_type.width())
                    && structurally_equal(storage, source_storage)
                {
                    // When both fields occupy the same storage unit, MWCC rotates
                    // the loaded unit into itself.  This preserves all unrelated
                    // bits and avoids a second load plus a separate extraction.
                    let storage_value =
                        self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
                    let address = self.member_base_register(base)?;
                    self.output.instructions.push(displacement_load(
                        storage_pointee,
                        storage_value,
                        address,
                        *offset as i16,
                    )?);
                    let begin = 32 - *shift - *width;
                    let end = 31 - *shift;
                    self.output
                        .instructions
                        .push(Instruction::RotateAndMaskInsert {
                            a: storage_value,
                            s: storage_value,
                            shift: (*shift + 32 - *source_shift) % 32,
                            begin,
                            end,
                        });
                    self.output.instructions.push(displacement_store(
                        storage_pointee,
                        storage_value,
                        address,
                        *offset as i16,
                    )?);
                    return Ok(true);
                }
                if let Expression::Member {
                    base: source_base,
                    offset: source_offset,
                    member_type: source_member_type,
                    index_stride: None,
                } = source_storage.as_ref()
                {
                    if matches!(source_base.as_ref(), Expression::Variable(_))
                        && matches!(
                            source_member_type,
                            Type::UnsignedChar | Type::UnsignedShort | Type::UnsignedInt
                        )
                        && u16::from(*source_shift) + u16::from(*source_width)
                            <= u16::from(source_member_type.width())
                        && i16::try_from(*source_offset).is_ok()
                    {
                        // Equal-width fields in separate storage units can be
                        // transferred directly from the raw source word. The
                        // insert's rotation maps the source field coordinates
                        // to the destination coordinates, so extracting to bit
                        // zero and shifting back would be redundant.
                        let source_pointee =
                            pointee_of_type(*source_member_type).ok_or_else(|| {
                                Diagnostic::error("unsupported source bit-field storage type")
                            })?;
                        let source =
                            self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
                        let source_address = self.member_base_register(source_base)?;
                        self.output.instructions.push(displacement_load(
                            source_pointee,
                            source,
                            source_address,
                            *source_offset as i16,
                        )?);
                        let address = self.member_base_register(base)?;
                        self.output.instructions.push(displacement_load(
                            storage_pointee,
                            GENERAL_SCRATCH,
                            address,
                            *offset as i16,
                        )?);
                        let begin = 32 - *shift - *width;
                        let end = 31 - *shift;
                        self.output
                            .instructions
                            .push(Instruction::RotateAndMaskInsert {
                                a: GENERAL_SCRATCH,
                                s: source,
                                shift: (*shift + 32 - *source_shift) % 32,
                                begin,
                                end,
                            });
                        self.output.instructions.push(displacement_store(
                            storage_pointee,
                            GENERAL_SCRATCH,
                            address,
                            *offset as i16,
                        )?);
                        return Ok(true);
                    }
                }
            }
        }
        let source = self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
        let address = self.member_base_register(base)?;
        let chained_assignment = matches!(value, Expression::Assign {
            target: inner,
            ..
        } if matches!(inner.as_ref(), Expression::BitFieldRead { .. }));
        if chained_assignment {
            // The inner assignment updates the same storage before the outer
            // field is merged. Preserve its yielded field value, then reload
            // the unit so the outer write observes that update.
            self.evaluate_general(value, source)?;
            self.output.instructions.push(displacement_load(
                storage_pointee,
                GENERAL_SCRATCH,
                address,
                *offset as i16,
            )?);
        } else {
            self.output.instructions.push(displacement_load(
                storage_pointee,
                GENERAL_SCRATCH,
                address,
                *offset as i16,
            )?);
            // In an ordinary store MWCC starts the memory dependency before an
            // independent constant materialization (`lbz; li; rlwimi; stb`).
            self.evaluate_general(value, source)?;
        }
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
