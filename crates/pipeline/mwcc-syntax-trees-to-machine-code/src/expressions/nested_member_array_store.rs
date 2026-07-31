//! Stores to a scalar element of a row-indexed aggregate member array.
//!
//! MWCC converts the still-live source into r4, scales the row index through
//! r3, folds the member and column displacement into r0, and uses an indexed
//! store against the aggregate base.

#[allow(unused_imports)]
use super::*;

struct NestedMemberArrayStore<'a> {
    aggregate: &'a Expression,
    member_offset: u32,
    row_stride: u32,
    row_index: &'a Expression,
    column: i64,
    element: Pointee,
    value: &'a Expression,
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<NestedMemberArrayStore<'a>> {
    let Expression::Index {
        base: row,
        index: column,
    } = target
    else {
        return None;
    };
    let column = constant_value(column)?;
    let Expression::Index {
        base: member,
        index: row_index,
    } = row.as_ref()
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: aggregate,
        offset: member_offset,
        element,
        index_stride: Some(row_stride),
    } = member.as_ref()
    else {
        return None;
    };
    matches!(value, Expression::Variable(_)).then_some(NestedMemberArrayStore {
        aggregate,
        member_offset: *member_offset,
        row_stride: *row_stride,
        row_index,
        column,
        element: *element,
        value,
    })
}

impl Generator {
    pub(crate) fn try_emit_nested_member_array_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some(store) = classify(target, value) else {
            return Ok(false);
        };
        if matches!(
            store.element,
            Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
        ) {
            return Ok(false);
        }
        let aggregate = self.general_register_of_leaf(store.aggregate)?;
        let row_index = self.general_register_of_leaf(store.row_index)?;
        let (source, source_width, _) = self.leaf_info(store.value)?;
        let target_type = store.element.element();
        let target_width = target_type.width();
        let source = if target_width < source_width {
            let converted = self.fresh_virtual_general_preferring(4);
            let instruction = match target_type {
                Type::Char => Instruction::ExtendSignByte {
                    a: converted,
                    s: source,
                },
                Type::UnsignedChar => Instruction::ClearLeftImmediate {
                    a: converted,
                    s: source,
                    clear: 24,
                },
                Type::Short => Instruction::ExtendSignHalfword {
                    a: converted,
                    s: source,
                },
                Type::UnsignedShort => Instruction::ClearLeftImmediate {
                    a: converted,
                    s: source,
                    clear: 16,
                },
                _ => return Ok(false),
            };
            self.output.instructions.push(instruction);
            converted
        } else {
            source
        };
        let row_stride = i16::try_from(store.row_stride)
            .map_err(|_| Diagnostic::error("member-array row stride is out of range"))?;
        let scaled = if !mwcc_vreg::Reg::is_virtual_field(source) && source == 3 {
            self.fresh_virtual_general_avoiding(vec![source])
        } else {
            self.fresh_virtual_general_preferring(3)
        };
        if store.row_stride.is_power_of_two() {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: row_index,
                    shift: store.row_stride.trailing_zeros() as u8,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::MultiplyImmediate {
                    d: scaled,
                    a: row_index,
                    immediate: row_stride,
                });
        }
        let offset = store
            .column
            .checked_mul(i64::from(store.element.size()))
            .and_then(|column| column.checked_add(i64::from(store.member_offset)))
            .and_then(|offset| i16::try_from(offset).ok())
            .ok_or_else(|| Diagnostic::error("nested member-array offset is out of range"))?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: scaled,
            immediate: offset,
        });
        self.output.instructions.push(indexed_store(
            store.element,
            source,
            aggregate,
            GENERAL_SCRATCH,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_a_constant_column() {
        let member = Expression::MemberAddress {
            base: Box::new(Expression::Variable("data".into())),
            offset: 90,
            element: Pointee::UnsignedChar,
            index_stride: Some(2),
        };
        let row = Expression::Index {
            base: Box::new(member),
            index: Box::new(Expression::Variable("row".into())),
        };
        let target = Expression::Index {
            base: Box::new(row),
            index: Box::new(Expression::Variable("column".into())),
        };

        assert!(classify(&target, &Expression::Variable("value".into())).is_none());
    }
}
