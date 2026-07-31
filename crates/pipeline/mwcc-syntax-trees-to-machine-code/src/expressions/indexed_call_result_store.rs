//! Call results stored through a variable-indexed member pointer.
//!
//! MWCC leaves the result in r3, forms the member pointer after the call, then
//! scales the saved index through r0 and uses an indexed store. Keeping this as
//! one transaction prevents the generic store path from retaining the pointer
//! unnecessarily across the call.

#[allow(unused_imports)]
use super::*;

fn indexed_member_pointer_call<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<(
    &'a Expression,
    u32,
    &'a Expression,
    Pointee,
    &'a str,
    &'a [Expression],
)> {
    let Expression::Index { base, index } = target else {
        return None;
    };
    let Expression::Member {
        base: member_base,
        offset,
        member_type: Type::Pointer(pointee),
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    let Expression::Call { name, arguments } = value else {
        return None;
    };
    Some((member_base, *offset, index, *pointee, name, arguments))
}

impl Generator {
    pub(crate) fn try_emit_indexed_call_result_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some((member_base, member_offset, index, pointee, callee, arguments)) =
            indexed_member_pointer_call(target, value)
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
        if matches!(
            pointee,
            Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
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
        let address = self.fresh_virtual_general_preferring(4);
        self.emit_member_load(
            member_base,
            member_offset,
            Type::Pointer(pointee),
            None,
            address,
        )?;
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
    fn recognizes_only_a_call_stored_through_an_indexed_member_pointer() {
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

        assert!(indexed_member_pointer_call(&target, &call).is_some());
        assert!(indexed_member_pointer_call(&target, &Expression::IntegerLiteral(0)).is_none());
    }
}
