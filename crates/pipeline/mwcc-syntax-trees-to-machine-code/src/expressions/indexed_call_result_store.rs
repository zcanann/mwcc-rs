//! Call results stored through a variable-indexed member pointer.
//!
//! MWCC leaves the result in r3, forms the member pointer after the call, then
//! scales the saved index through r0 and uses an indexed store. Keeping this as
//! one transaction prevents the generic store path from retaining the pointer
//! unnecessarily across the call.

#[allow(unused_imports)]
use super::*;

enum IndexedCallBase<'a> {
    Direct(&'a Expression),
    Member {
        base: &'a Expression,
        offset: u32,
        pointee: Pointee,
    },
}

fn indexed_pointer_call<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<(IndexedCallBase<'a>, &'a Expression, &'a str, &'a [Expression])> {
    let Expression::Index { base, index } = target else {
        return None;
    };
    let store_base = match base.as_ref() {
        Expression::Member {
            base,
            offset,
            member_type: Type::Pointer(pointee),
            index_stride: None,
        } => IndexedCallBase::Member {
            base,
            offset: *offset,
            pointee: *pointee,
        },
        direct @ Expression::Variable(_) => IndexedCallBase::Direct(direct),
        _ => return None,
    };
    let Expression::Call { name, arguments } = value else {
        return None;
    };
    Some((store_base, index, name, arguments))
}

impl Generator {
    pub(crate) fn try_emit_indexed_call_result_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((store_base, index, callee, arguments)) = indexed_pointer_call(target, value)
        else {
            return Ok(false);
        };
        let return_type = self.call_return_types.get(callee).copied();
        if matches!(
            return_type,
            Some(
                Type::Float
                    | Type::Double
                    | Type::LongLong
                    | Type::UnsignedLongLong
                    | Type::Struct { .. }
            )
        ) {
            return Ok(false);
        }
        let index_register = self.general_register_of_leaf(index)?;
        let index_register =
            if !mwcc_vreg::Reg::is_virtual_field(index_register) && index_register < 14 {
                let retained = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(Instruction::move_register(retained, index_register));
                retained
            } else {
                index_register
            };
        let result = Eabi::general_result().number;
        self.emit_call(callee, arguments, Some(result), false)?;

        let restore = self.reserved.insert(result);
        let (pointee, address) = match store_base {
            IndexedCallBase::Direct(base) => self.resolve_pointer(base)?,
            IndexedCallBase::Member {
                base,
                offset,
                pointee,
            } => {
                let address = self.fresh_virtual_general_preferring(4);
                self.emit_member_load(
                    base,
                    offset,
                    Type::Pointer(pointee),
                    None,
                    address,
                )?;
                (pointee, address)
            }
        };
        if matches!(
            pointee,
            Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
        ) {
            return Ok(false);
        }
        let size = pointee.size();
        let scaled = if size == 1 {
            index_register
        } else {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index_register,
                    shift: size.trailing_zeros() as u8,
                });
            GENERAL_SCRATCH
        };
        if restore {
            self.reserved.remove(&result);
        }
        self.output
            .instructions
            .push(indexed_store(pointee, result, address, scaled)?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_call_stored_through_an_indexed_member_pointer() {
        let member = Expression::Member {
            base: Box::new(Expression::Variable("object".into())),
            offset: 72,
            member_type: Type::Pointer(Pointee::Short),
            index_stride: None,
        };
        let target = Expression::Index {
            base: Box::new(member),
            index: Box::new(Expression::Variable("index".into())),
        };
        let call = Expression::Call {
            name: "make_value".into(),
            arguments: Vec::new(),
        };

        assert!(indexed_pointer_call(&target, &call).is_some());
        assert!(indexed_pointer_call(&target, &Expression::IntegerLiteral(0)).is_none());
    }

    #[test]
    fn recognizes_a_call_stored_through_a_direct_pointer() {
        let target = Expression::Index {
            base: Box::new(Expression::Variable("table".into())),
            index: Box::new(Expression::Variable("index".into())),
        };
        let call = Expression::Call {
            name: "allocate".into(),
            arguments: vec![Expression::IntegerLiteral(32)],
        };

        assert!(matches!(
            indexed_pointer_call(&target, &call),
            Some((IndexedCallBase::Direct(Expression::Variable(name)), _, "allocate", _))
                if name == "table"
        ));
    }
}
