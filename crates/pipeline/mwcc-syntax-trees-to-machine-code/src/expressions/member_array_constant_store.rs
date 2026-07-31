//! Constant stores into a variable-indexed aggregate member array.
//!
//! O0 keeps the aggregate base intact, materializes the stored value in r4,
//! scales the index into r3, folds the member offset into r0, and finishes with
//! an indexed store.

use super::*;

struct MemberArrayConstantStore<'a> {
    aggregate: &'a Expression,
    member_offset: u32,
    index: &'a Expression,
    element: Pointee,
    value: i64,
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<MemberArrayConstantStore<'a>> {
    let Expression::Index { base, index } = target else {
        return None;
    };
    let Expression::MemberAddress {
        base: aggregate,
        offset,
        element,
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    if !matches!(index.as_ref(), Expression::Variable(_)) {
        return None;
    }
    let value = constant_value(value)?;
    Some(MemberArrayConstantStore {
        aggregate,
        member_offset: *offset,
        index,
        element: *element,
        value,
    })
}

impl Generator {
    pub(crate) fn try_emit_member_array_constant_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if self.behavior.optimization != mwcc_versions::Optimization::O0 {
            return Ok(false);
        }
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
        let index = self.general_register_of_leaf(store.index)?;
        let member_offset = i16::try_from(store.member_offset)
            .map_err(|_| Diagnostic::error("member-array offset is out of range"))?;

        let source = self.fresh_virtual_general_preferring(4);
        self.load_integer_constant(source, store.value);
        let scaled = if store.element.size() == 1 {
            index
        } else {
            let scaled = self.fresh_virtual_general_preferring(3);
            let size = store.element.size();
            if size.is_power_of_two() {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: scaled,
                        s: index,
                        shift: size.trailing_zeros() as u8,
                    });
            } else {
                self.output
                    .instructions
                    .push(Instruction::MultiplyImmediate {
                        d: scaled,
                        a: index,
                        immediate: i16::from(size),
                    });
            }
            scaled
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: scaled,
            immediate: member_offset,
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
    fn recognizes_a_constant_store_to_a_variable_member_index() {
        let target = Expression::Index {
            base: Box::new(Expression::MemberAddress {
                base: Box::new(Expression::Variable("object".into())),
                offset: 76,
                element: Pointee::UnsignedInt,
                index_stride: None,
            }),
            index: Box::new(Expression::Variable("index".into())),
        };

        let store = classify(&target, &Expression::IntegerLiteral(0))
            .expect("variable-indexed member-array constant store");
        assert_eq!(store.member_offset, 76);
        assert_eq!(store.value, 0);
        assert!(matches!(store.index, Expression::Variable(name) if name == "index"));
    }
}
