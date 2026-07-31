//! Legacy O0 two-level global pointer-table stores from a register value.
//!
//! A register source changes the complete transaction relative to the
//! member/constant form: MWCC narrows it into r3 first, walks the root pointer
//! through r4/r5, retains the nested pointer in r6, and forms the second global
//! address with distinct high and low registers before the final indexed store.

use super::nested_global_member_pointer_store::classify;
use super::*;

impl Generator {
    pub(crate) fn try_emit_legacy_nested_global_member_pointer_variable_store(
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
        let Expression::Variable(_) = store.value else {
            return Ok(false);
        };
        if !matches!(store.element, Pointee::Short | Pointee::UnsignedShort)
            || !matches!(
                self.addressable_globals.get(store.global),
                Some(Type::Struct { size, .. }) if *size > 8
            )
        {
            return Ok(false);
        }

        let (value_register, _, _) = self.leaf_info(store.value)?;
        let root_pointer_offset = narrow_offset(store.root_pointer_offset, "root pointer")?;
        let first_index_offset = narrow_offset(store.first_index_offset, "first index")?;
        let first_stride = narrow_offset(store.first_stride, "first stride")?;
        let nested_pointer_offset = narrow_offset(store.nested_pointer_offset, "nested pointer")?;
        let second_index_offset = narrow_offset(store.second_index_offset, "second index")?;
        let second_stride = narrow_offset(store.second_stride, "second stride")?;
        let final_offset = i16::try_from(store.final_offset)
            .map_err(|_| Diagnostic::error("final nested offset is out of range"))?;

        let source = self.fresh_virtual_general_preferring(3);
        self.emit_widen(
            source,
            value_register,
            store.element.size() * 8,
            store.element == Pointee::Short,
        );

        let root_owner = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(root_owner, store.global);
        self.emit_address_low(root_owner, store.global);
        let root_pointer = self.fresh_virtual_general_preferring(5);
        self.output.instructions.push(Instruction::LoadWord {
            d: root_pointer,
            a: root_owner,
            offset: root_pointer_offset,
        });

        let first_owner = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(first_owner, store.global);
        self.emit_address_low(first_owner, store.global);
        self.output.instructions.push(displacement_load(
            pointee_of_type(store.first_index_type)
                .expect("first nested index was classified as scalar"),
            GENERAL_SCRATCH,
            first_owner,
            first_index_offset,
        )?);
        let first_scaled = self.fresh_virtual_general_preferring(4);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: first_scaled,
                a: GENERAL_SCRATCH,
                immediate: first_stride,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: first_scaled,
            a: first_scaled,
            immediate: nested_pointer_offset,
        });
        let nested_pointer = self.fresh_virtual_general_preferring(6);
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: nested_pointer,
            a: root_pointer,
            b: first_scaled,
        });

        let second_high = self.fresh_virtual_general_preferring(5);
        self.emit_address_high(second_high, store.global);
        let second_low = self.fresh_virtual_general_preferring(4);
        self.record_relocation(RelocationKind::Addr16Lo, store.global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: second_low,
            a: second_high,
            immediate: 0,
        });
        self.output.instructions.push(displacement_load(
            pointee_of_type(store.second_index_type)
                .expect("second nested index was classified as scalar"),
            second_high,
            second_low,
            second_index_offset,
        )?);
        let second_scaled = self.fresh_virtual_general_preferring(4);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: second_scaled,
                a: second_high,
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
            nested_pointer,
            GENERAL_SCRATCH,
        )?);
        Ok(true)
    }
}

fn narrow_offset(value: u32, label: &str) -> Compilation<i16> {
    i16::try_from(value)
        .map_err(|_| Diagnostic::error(format!("{label} is out of signed 16-bit range")))
}
