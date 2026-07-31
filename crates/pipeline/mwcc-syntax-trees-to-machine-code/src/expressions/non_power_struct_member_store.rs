//! O0 stores fed by a member of a non-power-of-two global struct array.
//!
//! The target pointer and loaded value jointly determine address placement, so
//! this transaction cannot be split safely between generic member-load and
//! member-store owners. MWCC scales first with `mulli`, materializes the global
//! base, loads the member, performs the assignment conversion, and stores it.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_emit_unoptimized_non_power_struct_member_store(
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
        let Expression::Member {
            base: target_base,
            offset: target_offset,
            member_type: target_type,
            index_stride: None,
        } = target
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: indexed,
            offset: source_offset,
            member_type: source_type,
            index_stride: Some(stride),
        } = value
        else {
            return Ok(false);
        };
        let Expression::Index { base: array, index } = indexed.as_ref() else {
            return Ok(false);
        };
        let Expression::Variable(array_name) = array.as_ref() else {
            return Ok(false);
        };
        let Some(&total_size) = self.global_array_sizes.get(array_name.as_str()) else {
            return Ok(false);
        };
        if stride.is_power_of_two()
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
        {
            return Ok(false);
        }
        let target_pointee = pointee_of_type(*target_type).ok_or_else(|| {
            Diagnostic::error("non-power struct-member store target is not scalar")
        })?;
        let source_pointee = pointee_of_type(*source_type).ok_or_else(|| {
            Diagnostic::error("non-power struct-member store source is not scalar")
        })?;
        if matches!(
            source_pointee,
            Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
        ) || matches!(
            target_pointee,
            Pointee::Float | Pointee::Double | Pointee::LongLong | Pointee::UnsignedLongLong
        ) {
            return Ok(false);
        }
        let stride = i16::try_from(*stride)
            .map_err(|_| Diagnostic::error("global struct-array stride is out of mulli range"))?;
        let source_offset = i16::try_from(*source_offset)
            .map_err(|_| Diagnostic::error("global struct-array member offset is out of range"))?;
        let target_offset = i16::try_from(*target_offset)
            .map_err(|_| Diagnostic::error("struct member store offset is out of range"))?;

        let target_address = self.member_base_register(target_base)?;
        let restore = target_address != GENERAL_SCRATCH && self.reserved.insert(target_address);
        let value_register = if target_address == Eabi::FIRST_GENERAL_ARGUMENT {
            GENERAL_SCRATCH
        } else {
            self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT)
        };
        let index_register = if let Ok(register) = self.general_register_of_leaf(index) {
            register
        } else {
            self.evaluate_general(index, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        };
        let scaled = self.fresh_virtual_general_preferring(5);
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: scaled,
                a: index_register,
                immediate: stride,
            });
        let base = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(base, array_name);
        self.record_relocation(RelocationKind::Addr16Lo, array_name);
        if value_register == GENERAL_SCRATCH {
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: base,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: base,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
            self.output.instructions.push(displacement_load(
                source_pointee,
                value_register,
                base,
                source_offset,
            )?);
        } else {
            self.output.instructions.push(Instruction::AddImmediate {
                d: base,
                a: base,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: value_register,
                a: base,
                b: scaled,
            });
            self.output.instructions.push(displacement_load(
                source_pointee,
                value_register,
                value_register,
                source_offset,
            )?);
        }
        if target_type.width() < source_type.width() {
            self.emit_widen(
                value_register,
                value_register,
                target_type.width(),
                self.signed_of(*target_type),
            );
        }
        if restore {
            self.reserved.remove(&target_address);
        }
        self.output.instructions.push(displacement_store(
            target_pointee,
            value_register,
            target_address,
            target_offset,
        )?);
        Ok(true)
    }
}
