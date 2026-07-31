//! Constant stores into a global array indexed by an aggregate member.
//!
//! Build 163 schedules `array[object->word[index]] = constant` value-first,
//! then loads and scales the member index before materializing the global base.
//! Keeping this transaction out of the generic global-array path avoids asking
//! leaf placement to flatten the nested member subscript.

use super::*;

struct MemberIndex<'a> {
    owner: &'a Expression,
    offset: u32,
}

fn classify(index: &Expression) -> Option<MemberIndex<'_>> {
    let Expression::Index { base, index } = index else {
        return None;
    };
    let Expression::MemberAddress {
        base: owner,
        offset,
        element: Pointee::Int | Pointee::UnsignedInt,
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    let element = constant_value(index)?;
    let offset = i64::from(*offset).checked_add(element.checked_mul(4)?)?;
    Some(MemberIndex {
        owner,
        offset: u32::try_from(offset).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_emit_member_indexed_global_array_constant_store(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        index: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if self.behavior.optimization != mwcc_versions::Optimization::O0
            || self.behavior.global_array_index_style
                != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
            || !matches!(pointee, Pointee::Int | Pointee::UnsignedInt)
        {
            return Ok(false);
        }
        let Some(member) = classify(index) else {
            return Ok(false);
        };
        let Some(value) = constant_value(value).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        let owner = self.general_register_of_leaf(member.owner)?;
        let member_offset = i16::try_from(member.offset)
            .map_err(|_| Diagnostic::error("global-array member index is out of range"))?;

        let stored = self.fresh_virtual_general_preferring(5);
        self.output.instructions.push(Instruction::AddImmediate {
            d: stored,
            a: 0,
            immediate: value,
        });
        let scaled = self.fresh_virtual_general_preferring(GENERAL_SCRATCH);
        self.output.instructions.push(Instruction::LoadWord {
            d: scaled,
            a: owner,
            offset: member_offset,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: scaled,
                shift: pointee.size().trailing_zeros() as u8,
            });
        let high = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(high, name);
        let address = self.fresh_virtual_general_preferring(3);
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: address,
            a: high,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: address,
            b: scaled,
        });
        self.output
            .instructions
            .push(displacement_store(pointee, stored, address, 0)?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_a_constant_subscript_into_the_member_index_offset() {
        let expression = Expression::Index {
            base: Box::new(Expression::MemberAddress {
                base: Box::new(Expression::Variable("object".into())),
                offset: 76,
                element: Pointee::UnsignedInt,
                index_stride: None,
            }),
            index: Box::new(Expression::IntegerLiteral(2)),
        };

        let member = classify(&expression).expect("member index");
        assert_eq!(member.offset, 84);
        assert!(matches!(member.owner, Expression::Variable(name) if name == "object"));
    }
}
