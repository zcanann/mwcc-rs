//! Word-copy scheduling for aggregate members exposed by inline expansion.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower the measured `*destination = source->vec3` shape.
    ///
    /// MWCC overlaps the first two loads with their stores, then copies the
    /// final word through r0.  The destination may be another struct member or
    /// an address-taken frame local; both are lvalues after peeling `*&`.
    pub(crate) fn try_emit_member_vec3_copy(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::Member {
            base: source_base,
            offset: source_offset,
            member_type: Type::Struct { size: 12, .. },
            index_stride: None,
        } = value
        else {
            return Ok(false);
        };
        let target = match target {
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::AddressOf { operand } => operand.as_ref(),
                _ => return Ok(false),
            },
            target => target,
        };

        let source_register = self.member_base_register(source_base)?;
        let (target_register, target_offset) = match target {
            Expression::Member {
                base,
                offset,
                member_type: Type::Struct { size: 12, .. },
                index_stride: None,
            } => (self.member_base_register(base)?, *offset),
            Expression::Variable(name) => {
                let Some(slot) = self.frame_slots.get(name).copied() else {
                    return Ok(false);
                };
                if !matches!(slot.value_type, Type::Struct { size: 12, .. }) || slot.is_array {
                    return Ok(false);
                }
                (1, u32::try_from(slot.offset).map_err(|_| {
                    Diagnostic::error("a Vec3 frame destination has a negative offset")
                })?)
            }
            _ => return Ok(false),
        };
        let first = Eabi::general_result().number;
        if source_register == first || target_register == first {
            return Ok(false);
        }
        let offset = |base: u32, add: u32| -> Compilation<i16> {
            i16::try_from(base.checked_add(add).ok_or_else(|| {
                Diagnostic::error("a Vec3 aggregate-copy offset overflowed")
            })?)
            .map_err(|_| Diagnostic::error("a Vec3 aggregate-copy offset is out of range"))
        };

        self.output.instructions.push(Instruction::LoadWord {
            d: first,
            a: source_register,
            offset: offset(*source_offset, 0)?,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: source_register,
            offset: offset(*source_offset, 4)?,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: first,
            a: target_register,
            offset: offset(target_offset, 0)?,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: target_register,
            offset: offset(target_offset, 4)?,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: source_register,
            offset: offset(*source_offset, 8)?,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: target_register,
            offset: offset(target_offset, 8)?,
        });
        if target_register == 1 {
            for add in [0, 4, 8] {
                self.written_slots.insert(offset(target_offset, add)?);
            }
        }
        Ok(true)
    }
}
