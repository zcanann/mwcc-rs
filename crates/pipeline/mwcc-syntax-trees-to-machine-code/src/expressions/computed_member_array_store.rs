//! Computed floating values stored into variable-indexed scalar member arrays.
//!
//! The ordinary member-array owner keeps its scaled index in r0 while placing
//! a simple source. A float-to-integer conversion also returns through r0, so
//! this family first completes the indexed address in its own live range and
//! only then evaluates the value.

#[allow(unused_imports)]
use super::*;

struct ComputedMemberArrayStore<'a> {
    aggregate: &'a Expression,
    offset: u32,
    element: Pointee,
    index: &'a Expression,
    value: &'a Expression,
}

fn classify<'a>(
    generator: &Generator,
    target: &'a Expression,
    value: &'a Expression,
) -> Option<ComputedMemberArrayStore<'a>> {
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
    (matches!(index.as_ref(), Expression::Variable(_))
        && !matches!(element, Pointee::Float | Pointee::Double)
        && generator.is_float_value(value)
        && !crate::analysis::expression_has_side_effect(value))
    .then_some(ComputedMemberArrayStore {
        aggregate,
        offset: *offset,
        element: *element,
        index,
        value,
    })
}

impl Generator {
    pub(super) fn try_emit_computed_member_array_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Some(store) = classify(self, target, value) else {
            return Ok(false);
        };
        let base = self.member_base_register(store.aggregate)?;
        let index = self.general_register_of_leaf(store.index)?;
        let size = store.element.size();
        if !size.is_power_of_two() {
            return Ok(false);
        }
        let scaled = if size == 1 {
            index
        } else {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index,
                    shift: size.trailing_zeros() as u8,
                });
            GENERAL_SCRATCH
        };
        let address = self.fresh_virtual_general_avoiding(vec![GENERAL_SCRATCH]);
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: base,
            b: scaled,
        });
        let restore = self.reserved.insert(address);
        let source = self.place_store_value(store.value, store.element)?;
        if restore {
            self.reserved.remove(&address);
        }
        let offset = i16::try_from(store.offset).map_err(|_| {
            Diagnostic::error("computed member-array store offset is out of range")
        })?;
        self.output.instructions.push(displacement_store(
            store.element,
            source,
            address,
            offset,
        )?);
        Ok(true)
    }
}
