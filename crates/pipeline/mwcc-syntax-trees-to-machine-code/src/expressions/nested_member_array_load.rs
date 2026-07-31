//! Loads from a scalar element of a row-indexed aggregate member array.
//!
//! MWCC scales the row directly into the result/argument register, folds the
//! member and column displacement through r0, and finishes with an indexed
//! load against the preserved aggregate base.

#[allow(unused_imports)]
use super::*;

struct NestedMemberArrayLoad<'a> {
    aggregate: &'a Expression,
    member_offset: u32,
    row_stride: u32,
    row_index: &'a Expression,
    column: i64,
    element: Pointee,
}

fn classify<'a>(row: &'a Expression, column: &'a Expression) -> Option<NestedMemberArrayLoad<'a>> {
    let column = constant_value(column)?;
    let Expression::Index {
        base: member,
        index: row_index,
    } = row
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
    Some(NestedMemberArrayLoad {
        aggregate,
        member_offset: *member_offset,
        row_stride: *row_stride,
        row_index,
        column,
        element: *element,
    })
}

impl Generator {
    pub(crate) fn try_emit_nested_member_array_load(
        &mut self,
        row: &Expression,
        column: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(load) = classify(row, column) else {
            return Ok(false);
        };
        if destination == GENERAL_SCRATCH
            || matches!(
                load.element,
                Pointee::Char
                    | Pointee::Float
                    | Pointee::Double
                    | Pointee::LongLong
                    | Pointee::UnsignedLongLong
            )
        {
            return Ok(false);
        }
        let aggregate = self.general_register_of_leaf(load.aggregate)?;
        let row_index = self.general_register_of_leaf(load.row_index)?;
        if destination == aggregate || destination == row_index {
            return Ok(false);
        }
        let row_stride = i16::try_from(load.row_stride)
            .map_err(|_| Diagnostic::error("member-array row stride is out of range"))?;
        if load.row_stride.is_power_of_two() {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: destination,
                    s: row_index,
                    shift: load.row_stride.trailing_zeros() as u8,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::MultiplyImmediate {
                    d: destination,
                    a: row_index,
                    immediate: row_stride,
                });
        }
        let offset = load
            .column
            .checked_mul(i64::from(load.element.size()))
            .and_then(|column| column.checked_add(i64::from(load.member_offset)))
            .and_then(|offset| i16::try_from(offset).ok())
            .ok_or_else(|| Diagnostic::error("nested member-array offset is out of range"))?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: destination,
            immediate: offset,
        });
        self.output.instructions.push(indexed_load(
            load.element,
            destination,
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
    fn recognizes_a_constant_column_of_a_row_index() {
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

        assert!(classify(&row, &Expression::IntegerLiteral(0)).is_some());
        assert!(classify(&row, &Expression::Variable("column".into())).is_none());
    }
}
