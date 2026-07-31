//! Legacy O0 stores through an indexed pointer member of a global struct.
//!
//! GC 1.x/2.x materialize a function designator first, load the pointer member
//! from the global owner, multiply the saved index, fold the field displacement
//! into that scaled value, and finish with `stwx`.

#[allow(unused_imports)]
use super::*;

struct GlobalMemberPointerStore<'a> {
    global: &'a str,
    pointer_offset: u32,
    stride: u32,
    field_offset: u32,
    index: &'a Expression,
    function: &'a str,
}

fn function_designator(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::AddressOf { operand } => {
            let Expression::Variable(name) = operand.as_ref() else {
                return None;
            };
            Some(name)
        }
        _ => None,
    }
}

fn classify<'a>(
    target: &'a Expression,
    value: &'a Expression,
) -> Option<GlobalMemberPointerStore<'a>> {
    let Expression::Member {
        base: indexed,
        offset: field_offset,
        member_type: Type::Pointer(_),
        index_stride: Some(stride),
    } = target
    else {
        return None;
    };
    let Expression::Index { base, index } = indexed.as_ref() else {
        return None;
    };
    let Expression::Member {
        base: owner,
        offset: pointer_offset,
        member_type: Type::StructPointer { element_size },
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    let Expression::Variable(global) = owner.as_ref() else {
        return None;
    };
    (*element_size == *stride).then_some(GlobalMemberPointerStore {
        global,
        pointer_offset: *pointer_offset,
        stride: *stride,
        field_offset: *field_offset,
        index,
        function: function_designator(value)?,
    })
}

impl Generator {
    pub(crate) fn try_emit_legacy_global_member_pointer_indexed_store(
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
        if self.locations.contains_key(store.global)
            || !matches!(
                self.addressable_globals.get(store.global),
                Some(Type::Struct { size, .. }) if *size > 8
            )
            || self.locations.contains_key(store.function)
            || self.frame_slots.contains_key(store.function)
            || self.globals.contains_key(store.function)
            || self.known_locals.contains(store.function)
        {
            return Ok(false);
        }
        let pointer_offset = i16::try_from(store.pointer_offset).map_err(|_| {
            Diagnostic::error("global struct pointer-member offset is out of range")
        })?;
        let stride = i16::try_from(store.stride)
            .map_err(|_| Diagnostic::error("indexed struct stride is out of mulli range"))?;
        let field_offset = i16::try_from(store.field_offset)
            .map_err(|_| Diagnostic::error("indexed struct member offset is out of range"))?;
        let index = self.general_register_of_leaf(store.index)?;

        let function_high = self.fresh_virtual_general_preferring(3);
        self.emit_address_high(function_high, store.function);
        let source = self.fresh_virtual_general_preferring(5);
        self.record_relocation(RelocationKind::Addr16Lo, store.function);
        self.output.instructions.push(Instruction::AddImmediate {
            d: source,
            a: function_high,
            immediate: 0,
        });

        let owner = self.fresh_virtual_general_preferring(3);
        self.emit_address_high(owner, store.global);
        self.emit_address_low(owner, store.global);
        let pointer = self.fresh_virtual_general_preferring(4);
        self.output.instructions.push(Instruction::LoadWord {
            d: pointer,
            a: owner,
            offset: pointer_offset,
        });

        let scaled = if mwcc_vreg::Reg::is_virtual_field(index) || index != 3 {
            self.fresh_virtual_general_preferring(3)
        } else {
            // A call-result local may still be consumed as an implicit r3 call
            // argument later. Keep the scaled temporary distinct when that
            // later use is not represented by an explicit move instruction.
            self.fresh_virtual_general_avoiding(vec![index])
        };
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: scaled,
                a: index,
                immediate: stride,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: scaled,
            immediate: field_offset,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed {
                s: source,
                a: pointer,
                b: GENERAL_SCRATCH,
            });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Index {
                base: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("owner".into())),
                    offset: 16,
                    member_type: Type::StructPointer { element_size: 160 },
                    index_stride: None,
                }),
                index: Box::new(Expression::Variable("index".into())),
            }),
            offset: 48,
            member_type: Type::Pointer(Pointee::Int),
            index_stride: Some(160),
        }
    }

    #[test]
    fn recognizes_bare_and_addressed_function_designators() {
        let bare = Expression::Variable("callback".into());
        let addressed = Expression::AddressOf {
            operand: Box::new(bare.clone()),
        };

        assert!(classify(&target(), &bare).is_some());
        assert!(classify(&target(), &addressed).is_some());
        assert!(classify(&target(), &Expression::IntegerLiteral(0)).is_none());
    }
}
