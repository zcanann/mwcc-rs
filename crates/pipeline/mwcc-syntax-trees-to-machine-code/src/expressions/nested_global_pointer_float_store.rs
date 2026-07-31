//! Legacy O0 floating stores through a nested global pointer table.
//!
//! The measured transaction loads the floating source first, materializes the
//! root global owner, indexes its first pointer table, loads the nested pointer,
//! and folds a constant second index into the final `stfs` displacement.

use super::*;

struct NestedGlobalPointerFloatStore<'a> {
    global: &'a str,
    root_pointer_offset: u32,
    first_index: &'a Expression,
    first_stride: u32,
    nested_pointer_offset: u32,
    second_index: i64,
    second_stride: u32,
    final_offset: u32,
    value_base: &'a Expression,
    value_offset: u32,
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<NestedGlobalPointerFloatStore<'a>> {
    let Expression::Member {
        base: second_indexed,
        offset: final_offset,
        member_type: Type::Float,
        index_stride: Some(second_stride),
    } = target
    else {
        return None;
    };
    let Expression::Index {
        base: nested_pointer,
        index: second_index,
    } = second_indexed.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base: first_indexed,
        offset: nested_pointer_offset,
        member_type: Type::StructPointer {
            element_size: second_element_size,
        },
        index_stride: Some(first_stride),
    } = nested_pointer.as_ref()
    else {
        return None;
    };
    let Expression::Index {
        base: root_pointer,
        index: first_index,
    } = first_indexed.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base: owner,
        offset: root_pointer_offset,
        member_type: Type::StructPointer {
            element_size: first_element_size,
        },
        index_stride: None,
    } = root_pointer.as_ref()
    else {
        return None;
    };
    let Expression::Variable(global) = owner.as_ref() else {
        return None;
    };
    let Expression::Member {
        base: value_base,
        offset: value_offset,
        member_type: Type::Float,
        index_stride: None,
    } = value
    else {
        return None;
    };
    (*first_element_size == *first_stride && *second_element_size == *second_stride).then_some(
        NestedGlobalPointerFloatStore {
            global,
            root_pointer_offset: *root_pointer_offset,
            first_index,
            first_stride: *first_stride,
            nested_pointer_offset: *nested_pointer_offset,
            second_index: constant_value(second_index)?,
            second_stride: *second_stride,
            final_offset: *final_offset,
            value_base,
            value_offset: *value_offset,
        },
    )
}

impl Generator {
    pub(crate) fn try_emit_legacy_nested_global_pointer_float_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if self.behavior.optimization != mwcc_versions::Optimization::O0
            || self.behavior.global_array_index_style
                != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
        {
            return Ok(false);
        }
        let Some(store) = classify(target, value) else {
            return Ok(false);
        };
        if !matches!(
            self.addressable_globals.get(store.global),
            Some(Type::Struct { size, .. }) if *size > 8
        ) {
            return Ok(false);
        }

        let value_base = self.general_register_of_leaf(store.value_base)?;
        let value_offset = signed_offset(store.value_offset, "floating source")?;
        let root_pointer_offset = signed_offset(store.root_pointer_offset, "root pointer")?;
        let first_stride = signed_offset(store.first_stride, "first stride")?;
        let nested_pointer_offset = signed_offset(store.nested_pointer_offset, "nested pointer")?;
        let final_offset = i64::from(store.final_offset)
            .checked_add(
                store
                    .second_index
                    .checked_mul(i64::from(store.second_stride))
                    .ok_or_else(|| Diagnostic::error("nested float-store offset overflow"))?,
            )
            .and_then(|offset| i16::try_from(offset).ok())
            .ok_or_else(|| Diagnostic::error("nested float-store offset is out of range"))?;

        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: FLOAT_SCRATCH,
            a: value_base,
            offset: value_offset,
        });

        let owner = self.fresh_virtual_general_preferring(3);
        self.emit_address_high(owner, store.global);
        self.emit_address_low(owner, store.global);
        let root_pointer = self.fresh_virtual_general_preferring(4);
        self.output.instructions.push(Instruction::LoadWord {
            d: root_pointer,
            a: owner,
            offset: root_pointer_offset,
        });

        self.evaluate_general(store.first_index, GENERAL_SCRATCH)?;
        let scaled = self.fresh_virtual_general_preferring(3);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: scaled,
                a: GENERAL_SCRATCH,
                immediate: first_stride,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: scaled,
            immediate: nested_pointer_offset,
        });
        let nested_pointer = self.fresh_virtual_general_preferring(3);
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: nested_pointer,
            a: root_pointer,
            b: GENERAL_SCRATCH,
        });
        self.output
            .instructions
            .push(Instruction::StoreFloatSingle {
                s: FLOAT_SCRATCH,
                a: nested_pointer,
                offset: final_offset,
            });
        Ok(true)
    }
}

fn signed_offset(value: u32, label: &str) -> Compilation<i16> {
    i16::try_from(value).map_err(|_| Diagnostic::error(format!("{label} offset is out of range")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nested_target() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Index {
                base: Box::new(Expression::Member {
                    base: Box::new(Expression::Index {
                        base: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("owner".into())),
                            offset: 16,
                            member_type: Type::StructPointer { element_size: 160 },
                            index_stride: None,
                        }),
                        index: Box::new(Expression::Variable("row".into())),
                    }),
                    offset: 60,
                    member_type: Type::StructPointer { element_size: 144 },
                    index_stride: Some(160),
                }),
                index: Box::new(Expression::IntegerLiteral(0)),
            }),
            offset: 88,
            member_type: Type::Float,
            index_stride: Some(144),
        }
    }

    #[test]
    fn recognizes_the_two_level_float_store_and_rejects_an_integer_value() {
        let floating = Expression::Member {
            base: Box::new(Expression::Variable("source".into())),
            offset: 4,
            member_type: Type::Float,
            index_stride: None,
        };

        let target = nested_target();
        let store = classify(&target, &floating).expect("nested float store");
        assert_eq!(store.second_index, 0);
        assert_eq!(store.second_stride, 144);
        assert!(classify(&target, &Expression::IntegerLiteral(0)).is_none());
    }
}
