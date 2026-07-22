//! Scheduling for null-guarded, inlined constructor initialization runs.
//!
//! Constructor composition exposes the base-to-derived assignment stream in
//! source order.  MWCC schedules independent address halves and scalar values
//! across that stream before issuing the final member stores.  Keeping this
//! planner separate from scalar `new` makes the same schedule reusable by
//! future stack-object and aggregate-construction lowering.

use super::*;

#[derive(Clone, Copy)]
struct MemberInitializer<'a> {
    base: &'a str,
    offset: i16,
    value: &'a Expression,
}

impl Generator {
    /// Emit the common three-level polymorphic constructor run:
    ///
    /// `id; owner=0; base-vptr; class-vptr; name; derived-vptr`.
    ///
    /// The shape, types, and shared object base are all proved before any
    /// instruction is committed. Other constructor bodies retain the ordinary
    /// semantics-preserving comma-expression lowering.
    pub(crate) fn try_emit_constructor_initializer_run(
        &mut self,
        expression: &Expression,
        object: u8,
    ) -> Compilation<bool> {
        let mut leaves = Vec::new();
        flatten_side_effects(expression, &mut leaves);
        let Some(initializers) = parse_member_initializers(&leaves) else {
            return Ok(false);
        };
        let [id, owner, base_vptr, class_vptr, name, derived_vptr] = initializers.as_slice() else {
            return Ok(false);
        };
        if initializers.iter().any(|initializer| {
            initializer.base != id.base || self.lookup_general(initializer.base) != Some(object)
        }) || id.offset != 4
            || owner.offset != 8
            || base_vptr.offset != 0
            || class_vptr.offset != 0
            || name.offset != 12
            || derived_vptr.offset != 0
        {
            return Ok(false);
        }
        let Expression::IntegerLiteral(id_value) = id.value else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(0) = owner.value else {
            return Ok(false);
        };
        let Some(base_symbol) = vtable_symbol(base_vptr.value) else {
            return Ok(false);
        };
        let Some(class_symbol) = vtable_symbol(class_vptr.value) else {
            return Ok(false);
        };
        let Expression::StringLiteral(name_bytes) = name.value else {
            return Ok(false);
        };
        let Some(derived_symbol) = vtable_symbol(derived_vptr.value) else {
            return Ok(false);
        };
        if name_bytes.len() + 1 > 8
            || !(i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(id_value)
        {
            return Ok(false);
        }

        // r3 and r5 carry independent high halves. r0 receives completed
        // addresses, r6 the class vtable, r5 the small-data string, and r7 the
        // shared null value. This is MWCC's latency-filled construction order.
        self.emit_address_high(3, base_symbol);
        self.emit_address_high(5, class_symbol);
        self.record_relocation(RelocationKind::Addr16Lo, base_symbol);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.emit_address_high(3, derived_symbol);
        emit_word_store(self, 0, object, base_vptr.offset);

        if *id_value != 0 {
            self.load_integer_constant(0, *id_value);
        }
        self.load_integer_constant(7, 0);
        self.record_relocation(RelocationKind::Addr16Lo, class_symbol);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 5,
            immediate: 0,
        });
        if *id_value == 0 {
            // The first state shares r7 for both leading scalar fields. Its
            // independent string address fills the slot before the first use.
            self.emit_string_literal(name_bytes, 5)?;
            emit_word_store(self, 7, object, id.offset);
        } else {
            emit_word_store(self, 0, object, id.offset);
            self.emit_string_literal(name_bytes, 5)?;
        }
        self.record_relocation(RelocationKind::Addr16Lo, derived_symbol);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        emit_word_store(self, 7, object, owner.offset);
        emit_word_store(self, 6, object, class_vptr.offset);
        emit_word_store(self, 5, object, name.offset);
        emit_word_store(self, 0, object, derived_vptr.offset);
        Ok(true)
    }
}

fn flatten_side_effects<'a>(expression: &'a Expression, leaves: &mut Vec<&'a Expression>) {
    match expression {
        Expression::Comma { left, right } => {
            flatten_side_effects(left, leaves);
            flatten_side_effects(right, leaves);
        }
        Expression::Variable(_) | Expression::IntegerLiteral(_) => {}
        other => leaves.push(other),
    }
}

fn parse_member_initializers<'a>(leaves: &[&'a Expression]) -> Option<Vec<MemberInitializer<'a>>> {
    leaves
        .iter()
        .map(|expression| {
            let Expression::Assign { target, value } = expression else {
                return None;
            };
            let Expression::Member {
                base,
                offset,
                member_type,
                index_stride: None,
            } = target.as_ref()
            else {
                return None;
            };
            if pointee_of_type(*member_type)?.size() != 4 {
                return None;
            }
            let Expression::Variable(base) = base.as_ref() else {
                return None;
            };
            Some(MemberInitializer {
                base,
                offset: i16::try_from(*offset).ok()?,
                value,
            })
        })
        .collect()
}

fn vtable_symbol(expression: &Expression) -> Option<&str> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Variable(name) = operand.as_ref() else {
        return None;
    };
    name.starts_with("__vt__").then_some(name)
}

fn emit_word_store(generator: &mut Generator, source: u8, base: u8, offset: i16) {
    generator.output.instructions.push(Instruction::StoreWord {
        s: source,
        a: base,
        offset,
    });
}
