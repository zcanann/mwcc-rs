//! Legacy O0 stores through two indexed pointer-member levels of a global.
//!
//! MWCC rematerializes the global owner for the root pointer and for each
//! independent index member. The complete transaction keeps the stored value
//! in r5 and the evolving pointer in r4 while r3/r0 form each scaled offset.

#[allow(unused_imports)]
use super::*;

struct NestedGlobalPointerStore<'a> {
    global: &'a str,
    root_pointer_offset: u32,
    first_index_offset: u32,
    first_index_type: Type,
    first_stride: u32,
    nested_pointer_offset: u32,
    second_index_offset: u32,
    second_index_type: Type,
    second_stride: u32,
    final_offset: i64,
    element: Pointee,
    value: &'a Expression,
}

fn global_member<'a>(
    expression: &'a Expression,
    expected_global: Option<&str>,
) -> Option<(&'a str, u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    if expected_global.is_some_and(|expected| expected != global) {
        return None;
    }
    Some((global, *offset, *member_type))
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<NestedGlobalPointerStore<'a>> {
    let Expression::Index {
        base: final_array,
        index: final_index,
    } = target
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: second_indexed,
        offset: final_member_offset,
        element,
        index_stride: None,
    } = final_array.as_ref()
    else {
        return None;
    };
    let final_index = constant_value(final_index)?;
    let final_offset = i64::from(*final_member_offset)
        .checked_add(final_index.checked_mul(i64::from(element.size()))?)?;
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
    let (_, first_index_offset, first_index_type) = global_member(first_index, Some(global))?;
    let (_, second_index_offset, second_index_type) = global_member(second_index, Some(global))?;
    let value_pointee = match value {
        Expression::Member {
            member_type,
            index_stride: None,
            ..
        } => Some(pointee_of_type(*member_type)?),
        Expression::IntegerLiteral(_) => None,
        _ => return None,
    };
    if *first_element_size != *first_stride
        || *second_element_size == 0
        || !matches!(
            pointee_of_type(first_index_type),
            Some(
                Pointee::Char
                    | Pointee::UnsignedChar
                    | Pointee::Short
                    | Pointee::UnsignedShort
                    | Pointee::Int
                    | Pointee::UnsignedInt
            )
        )
        || !matches!(
            pointee_of_type(second_index_type),
            Some(
                Pointee::Char
                    | Pointee::UnsignedChar
                    | Pointee::Short
                    | Pointee::UnsignedShort
                    | Pointee::Int
                    | Pointee::UnsignedInt
            )
        )
        || matches!(
            value_pointee,
            Some(
                Pointee::Char
                    | Pointee::Float
                    | Pointee::Double
                    | Pointee::LongLong
                    | Pointee::UnsignedLongLong
            )
        )
    {
        return None;
    }
    Some(NestedGlobalPointerStore {
        global,
        root_pointer_offset: *root_pointer_offset,
        first_index_offset,
        first_index_type,
        first_stride: *first_stride,
        nested_pointer_offset: *nested_pointer_offset,
        second_index_offset,
        second_index_type,
        second_stride: *second_element_size,
        final_offset,
        element: *element,
        value,
    })
}

impl Generator {
    fn emit_nested_store_global_address(&mut self, global: &str, register: u8) {
        self.emit_address_high(register, global);
        self.emit_address_low(register, global);
    }

    pub(crate) fn try_emit_legacy_nested_global_member_pointer_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        if self.behavior.optimization != mwcc_versions::Optimization::O0
            || self.behavior.function_address_store_style != FunctionAddressStoreStyle::ScratchValue
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
        let source_member = match store.value {
            Expression::Member {
                base,
                offset,
                member_type,
                ..
            } => Some((
                self.general_register_of_leaf(base)?,
                i16::try_from(*offset)
                    .map_err(|_| Diagnostic::error("source member offset is out of range"))?,
                pointee_of_type(*member_type)
                    .expect("nested store source was classified as scalar"),
            )),
            Expression::IntegerLiteral(_) => None,
            _ => unreachable!("nested store value shape was classified"),
        };
        // This owner uses r3-r5 throughout the pointer walk. A volatile source
        // base needs whole-function home planning before entering this path;
        // otherwise a later implicit call-argument use would be clobbered.
        if let Some((source_base, _, _)) = source_member {
            if (!mwcc_vreg::Reg::is_virtual_field(source_base) && source_base < 14)
                || source_base == 5
            {
                return Ok(false);
            }
        }
        let root_pointer_offset = i16::try_from(store.root_pointer_offset)
            .map_err(|_| Diagnostic::error("root pointer-member offset is out of range"))?;
        let first_stride = i16::try_from(store.first_stride)
            .map_err(|_| Diagnostic::error("first nested stride is out of mulli range"))?;
        let nested_pointer_offset = i16::try_from(store.nested_pointer_offset)
            .map_err(|_| Diagnostic::error("nested pointer-member offset is out of range"))?;
        let second_stride = i16::try_from(store.second_stride)
            .map_err(|_| Diagnostic::error("second nested stride is out of mulli range"))?;
        let final_offset = i16::try_from(store.final_offset)
            .map_err(|_| Diagnostic::error("nested target offset is out of range"))?;
        let first_index_offset = i16::try_from(store.first_index_offset)
            .map_err(|_| Diagnostic::error("first global index offset is out of range"))?;
        let second_index_offset = i16::try_from(store.second_index_offset)
            .map_err(|_| Diagnostic::error("second global index offset is out of range"))?;

        let source = self.fresh_virtual_general_preferring(5);
        match (source_member, store.value) {
            (Some((source_base, source_offset, source_type)), _) => {
                self.output.instructions.push(displacement_load(
                    source_type,
                    source,
                    source_base,
                    source_offset,
                )?);
            }
            (None, Expression::IntegerLiteral(constant)) => {
                self.load_integer_constant(source, *constant);
            }
            _ => unreachable!("nested store source was classified"),
        }

        let owner = self.fresh_virtual_general_preferring(3);
        self.emit_nested_store_global_address(store.global, owner);
        let pointer = self.fresh_virtual_general_preferring(4);
        self.output.instructions.push(Instruction::LoadWord {
            d: pointer,
            a: owner,
            offset: root_pointer_offset,
        });

        let first_owner = self.fresh_virtual_general_preferring(3);
        self.emit_nested_store_global_address(store.global, first_owner);
        self.output.instructions.push(displacement_load(
            pointee_of_type(store.first_index_type)
                .expect("first nested index type was classified as scalar"),
            GENERAL_SCRATCH,
            first_owner,
            first_index_offset,
        )?);
        let first_scaled = self.fresh_virtual_general_preferring(3);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: first_scaled,
                a: GENERAL_SCRATCH,
                immediate: first_stride,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: first_scaled,
            immediate: nested_pointer_offset,
        });
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: pointer,
            a: pointer,
            b: GENERAL_SCRATCH,
        });

        let second_owner = self.fresh_virtual_general_preferring(3);
        self.emit_nested_store_global_address(store.global, second_owner);
        self.output.instructions.push(displacement_load(
            pointee_of_type(store.second_index_type)
                .expect("second nested index type was classified as scalar"),
            GENERAL_SCRATCH,
            second_owner,
            second_index_offset,
        )?);
        let second_scaled = self.fresh_virtual_general_preferring(3);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: second_scaled,
                a: GENERAL_SCRATCH,
                immediate: second_stride,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: second_scaled,
            immediate: final_offset,
        });
        self.output.instructions.push(indexed_store(
            store.element,
            source,
            pointer,
            GENERAL_SCRATCH,
        )?);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_shallow_store_without_two_pointer_levels() {
        let target = Expression::Member {
            base: Box::new(Expression::Variable("entry".into())),
            offset: 112,
            member_type: Type::Short,
            index_stride: None,
        };
        let value = Expression::Member {
            base: Box::new(Expression::Variable("data".into())),
            offset: 0,
            member_type: Type::UnsignedChar,
            index_stride: None,
        };

        assert!(classify(&target, &value).is_none());
    }
}
