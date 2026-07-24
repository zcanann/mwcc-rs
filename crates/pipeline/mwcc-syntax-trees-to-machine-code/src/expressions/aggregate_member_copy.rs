//! Word-copy scheduling for aggregate members exposed by inline expansion.

#[allow(unused_imports)]
use super::*;

fn scalarized_vec3_assignments(
    expression: &Expression,
) -> Option<[(&Expression, &Expression); 3]> {
    fn assignment(expression: &Expression) -> Option<(&Expression, &Expression)> {
        let Expression::Assign { target, value } = expression else {
            return None;
        };
        Some((target.as_ref(), value.as_ref()))
    }

    let Expression::Comma { left, right: third } = expression else {
        return None;
    };
    let Expression::Comma { left: first, right: second } = left.as_ref() else {
        return None;
    };
    Some([
        assignment(first)?,
        assignment(second)?,
        assignment(third)?,
    ])
}

fn float_member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Float,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    Some((base.as_ref(), *offset))
}

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
        let source_offset = i16::try_from(*source_offset)
            .map_err(|_| Diagnostic::error("a Vec3 source offset is out of range"))?;
        let (target_register, target_offset) = match target {
            Expression::Member {
                base,
                offset,
                member_type: Type::Struct { size: 12, .. },
                index_stride: None,
            } => (
                self.member_base_register(base)?,
                i16::try_from(*offset)
                    .map_err(|_| Diagnostic::error("a Vec3 target offset is out of range"))?,
            ),
            Expression::Variable(name) => {
                let Some(slot) = self.frame_slots.get(name).copied() else {
                    return Ok(false);
                };
                if !matches!(slot.value_type, Type::Struct { size: 12, .. }) || slot.is_array {
                    return Ok(false);
                }
                (1, slot.offset)
            }
            _ => return Ok(false),
        };
        self.emit_vec3_word_copy(
            source_register,
            source_offset,
            target_register,
            target_offset,
        )
    }

    /// Recover the three scalar assignments produced when inline expansion
    /// decomposes `*destination = source_vec3`, then re-form MWCC's word-copy
    /// schedule. The source and destination bases are evaluated once.
    pub(crate) fn try_emit_scalarized_vec3_copy(
        &mut self,
        expression: &Expression,
    ) -> Compilation<bool> {
        let Some(assignments) = scalarized_vec3_assignments(expression) else {
            return Ok(false);
        };
        let targets = assignments
            .iter()
            .map(|(target, _)| float_member(target))
            .collect::<Option<Vec<_>>>();
        let sources = assignments
            .iter()
            .map(|(_, source)| float_member(source))
            .collect::<Option<Vec<_>>>();
        let (Some(targets), Some(sources)) = (targets, sources) else {
            return Ok(false);
        };
        if !matches!(targets[0].0, Expression::Variable(_))
            || !matches!(sources[0].0, Expression::Variable(_))
            || !targets.iter().all(|(base, _)| structurally_equal(base, targets[0].0))
            || !sources.iter().all(|(base, _)| structurally_equal(base, sources[0].0))
            || targets[1].1 != targets[0].1.checked_add(4).unwrap_or(u32::MAX)
            || targets[2].1 != targets[0].1.checked_add(8).unwrap_or(u32::MAX)
            || sources[1].1 != sources[0].1.checked_add(4).unwrap_or(u32::MAX)
            || sources[2].1 != sources[0].1.checked_add(8).unwrap_or(u32::MAX)
        {
            return Ok(false);
        }

        let Some((source_register, source_offset)) =
            self.vec3_scalar_base(sources[0].0, sources[0].1)?
        else {
            return Ok(false);
        };
        let Some((target_register, target_offset)) =
            self.vec3_scalar_base(targets[0].0, targets[0].1)?
        else {
            return Ok(false);
        };
        self.emit_vec3_word_copy(
            source_register,
            source_offset,
            target_register,
            target_offset,
        )
    }

    fn vec3_scalar_base(
        &mut self,
        base: &Expression,
        offset: u32,
    ) -> Compilation<Option<(u8, i16)>> {
        let Expression::Variable(name) = base else {
            return Ok(None);
        };
        if let Some(slot) = self.frame_slots.get(name).copied() {
            if !matches!(slot.value_type, Type::Struct { size: 12, .. }) || slot.is_array {
                return Ok(None);
            }
            let offset = i16::try_from(offset)
                .ok()
                .and_then(|offset| slot.offset.checked_add(offset))
                .ok_or_else(|| Diagnostic::error("a scalarized Vec3 frame offset is out of range"))?;
            return Ok(Some((1, offset)));
        }
        let offset = i16::try_from(offset)
            .map_err(|_| Diagnostic::error("a scalarized Vec3 member offset is out of range"))?;
        Ok(Some((self.member_base_register(base)?, offset)))
    }

    fn emit_vec3_word_copy(
        &mut self,
        source_register: u8,
        source_offset: i16,
        target_register: u8,
        target_offset: i16,
    ) -> Compilation<bool> {
        let first = Eabi::general_result().number;
        if source_register == first || target_register == first {
            return Ok(false);
        }
        let offset = |base: i16, add: i16| -> Compilation<i16> {
            base.checked_add(add)
                .ok_or_else(|| Diagnostic::error("a Vec3 aggregate-copy offset is out of range"))
        };

        self.output.instructions.push(Instruction::LoadWord {
            d: first,
            a: source_register,
            offset: offset(source_offset, 0)?,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: GENERAL_SCRATCH,
            a: source_register,
            offset: offset(source_offset, 4)?,
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
            offset: offset(source_offset, 8)?,
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
