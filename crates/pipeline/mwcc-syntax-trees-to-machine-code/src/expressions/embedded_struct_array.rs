//! Constant indexing through an inline array of aggregate members.
//!
//! The compact IR represents `owner->rows[i]` as an `Index` over a
//! struct-valued `Member`.  That intermediate value is inline storage: it must
//! contribute `member_offset + i * sizeof(row)` to a following address or
//! scalar access, rather than being loaded as a pointer.

#[allow(unused_imports)]
use super::*;

struct EmbeddedStructElement<'a> {
    aggregate: &'a Expression,
    offset: u32,
}

struct EmbeddedStructScalar<'a> {
    aggregate: &'a Expression,
    offset: u32,
    element: Pointee,
}

fn element(expression: &Expression) -> Option<EmbeddedStructElement<'_>> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::Member {
        base: aggregate,
        offset,
        member_type: Type::Struct { size, .. },
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    let index = u32::try_from(constant_value(index)?).ok()?;
    let offset = index
        .checked_mul(u32::from(*size))?
        .checked_add(*offset)?;
    Some(EmbeddedStructElement {
        aggregate,
        offset,
    })
}

fn scalar<'a>(
    base: &'a Expression,
    index: &Expression,
) -> Option<EmbeddedStructScalar<'a>> {
    let Expression::MemberAddress {
        base,
        offset: member_offset,
        element: scalar,
        index_stride: None,
    } = base
    else {
        return None;
    };
    let embedded = element(base)?;
    let index = u32::try_from(constant_value(index)?).ok()?;
    let offset = index
        .checked_mul(u32::from(scalar.size()))?
        .checked_add(*member_offset)?
        .checked_add(embedded.offset)?;
    Some(EmbeddedStructScalar {
        aggregate: embedded.aggregate,
        offset,
        element: *scalar,
    })
}

fn member_scalar(target: &Expression) -> Option<EmbeddedStructScalar<'_>> {
    let Expression::Member {
        base,
        offset: member_offset,
        member_type,
        index_stride: Some(index_stride),
    } = target
    else {
        return None;
    };
    let Expression::Index {
        base: inline_array,
        ..
    } = base.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        member_type: Type::Struct { size, .. },
        ..
    } = inline_array.as_ref()
    else {
        return None;
    };
    if index_stride != size {
        return None;
    }
    let embedded = element(base)?;
    let element = pointee_of_type(*member_type)?;
    let offset = embedded.offset.checked_add(*member_offset)?;
    Some(EmbeddedStructScalar {
        aggregate: embedded.aggregate,
        offset,
        element,
    })
}

impl Generator {
    pub(super) fn try_emit_embedded_struct_array_address(
        &mut self,
        base: &Expression,
        trailing_offset: u32,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(embedded) = element(base) else {
            return Ok(false);
        };
        let offset = embedded.offset.checked_add(trailing_offset).ok_or_else(|| {
            Diagnostic::error("embedded struct-array address offset overflow")
        })?;
        self.emit_member_address(embedded.aggregate, offset, destination)?;
        Ok(true)
    }

    pub(super) fn try_emit_embedded_struct_array_load(
        &mut self,
        base: &Expression,
        index: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some(access) = scalar(base, index) else {
            return Ok(false);
        };
        let offset = i16::try_from(access.offset).map_err(|_| {
            Diagnostic::error("embedded struct-array load offset is out of range")
        })?;
        let address = self.member_base_register(access.aggregate)?;
        self.output.instructions.push(displacement_load(
            access.element,
            destination,
            address,
            offset,
        )?);
        Ok(true)
    }

    pub(super) fn try_emit_embedded_struct_array_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let access = match target {
            Expression::Index { base, index } => scalar(base, index),
            Expression::Member { .. } => member_scalar(target),
            _ => None,
        };
        let Some(access) = access else {
            return Ok(false);
        };
        let offset = i16::try_from(access.offset).map_err(|_| {
            Diagnostic::error("embedded struct-array store offset is out of range")
        })?;
        let address = self.member_base_register(access.aggregate)?;
        let restore = address != GENERAL_SCRATCH && self.reserved.insert(address);
        let source = self.place_store_value(value, access.element)?;
        if restore {
            self.reserved.remove(&address);
        }
        self.output.instructions.push(displacement_store(
            access.element,
            source,
            address,
            offset,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Expression {
        Expression::Index {
            base: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("owner".into())),
                offset: 188,
                member_type: Type::Struct { size: 12, align: 4 },
                index_stride: None,
            }),
            index: Box::new(Expression::IntegerLiteral(1)),
        }
    }

    #[test]
    fn folds_the_constant_row_into_inline_storage() {
        let row = rows();
        let embedded = element(&row).expect("constant row should classify");
        assert_eq!(embedded.offset, 200);
        assert!(matches!(embedded.aggregate, Expression::Variable(name) if name == "owner"));
    }

    #[test]
    fn folds_the_scalar_column_after_the_row() {
        let member = Expression::MemberAddress {
            base: Box::new(rows()),
            offset: 0,
            element: Pointee::Float,
            index_stride: None,
        };
        let access = scalar(
            &member,
            &Expression::IntegerLiteral(2),
        )
        .expect("constant scalar access should classify");
        assert_eq!(access.offset, 208);
        assert_eq!(access.element, Pointee::Float);
    }

    #[test]
    fn folds_a_scalar_member_of_the_constant_row() {
        let member = Expression::Member {
            base: Box::new(rows()),
            offset: 3,
            member_type: Type::UnsignedChar,
            index_stride: Some(12),
        };
        let access = member_scalar(&member).expect("constant row member should classify");
        assert_eq!(access.offset, 203);
        assert_eq!(access.element, Pointee::UnsignedChar);
    }
}
