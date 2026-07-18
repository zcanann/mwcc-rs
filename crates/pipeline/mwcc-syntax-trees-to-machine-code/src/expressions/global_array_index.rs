//! Build-163 address schedules for variable-indexed file-scope arrays.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn emit_legacy_global_array_variable_load(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        index: u8,
        destination: u8,
    ) -> Compilation<bool> {
        if self.behavior.global_array_index_style
            != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
        {
            return Ok(false);
        }
        let address = if matches!(pointee, Pointee::Float | Pointee::Double) {
            self.free_general_excluding(GENERAL_SCRATCH)?
        } else {
            destination
        };
        self.emit_legacy_global_array_address(name, total_size, pointee.size(), index, address)?;
        self.output
            .instructions
            .push(displacement_load(pointee, destination, address, 0)?);
        Ok(true)
    }

    pub(crate) fn emit_legacy_global_array_variable_store(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        index: u8,
        value: u8,
    ) -> Compilation<bool> {
        if self.behavior.global_array_index_style
            != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
        {
            return Ok(false);
        }
        self.emit_legacy_global_array_address(name, total_size, pointee.size(), index, index)?;
        self.output
            .instructions
            .push(displacement_store(pointee, value, index, 0)?);
        Ok(true)
    }

    pub(crate) fn emit_legacy_global_array_constant_store(
        &mut self,
        name: &str,
        pointee: Pointee,
        index: u8,
        value: i16,
        offset: i16,
    ) -> Compilation<bool> {
        if self.behavior.global_array_index_style
            != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
        {
            return Ok(false);
        }
        let high = self.fresh_virtual_general();
        self.emit_address_high(high, name);
        let shift = pointee.size().trailing_zeros() as u8;
        if offset == 0 {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: index,
                    s: index,
                    shift,
                });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: index,
                a: GENERAL_SCRATCH,
                b: index,
            });
        } else {
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: high,
                a: high,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index,
                    shift,
                });
            self.output.instructions.push(Instruction::Add {
                d: index,
                a: high,
                b: GENERAL_SCRATCH,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: 0,
            immediate: value,
        });
        self.output
            .instructions
            .push(displacement_store(pointee, GENERAL_SCRATCH, index, offset)?);
        Ok(true)
    }

    fn emit_legacy_global_array_address(
        &mut self,
        name: &str,
        total_size: u32,
        element_size: u8,
        index: u8,
        address: u8,
    ) -> Compilation<()> {
        let shift = element_size.trailing_zeros() as u8;
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: index,
                    s: index,
                    shift,
                });
            self.emit_global_array_base(name, total_size, GENERAL_SCRATCH)?;
        } else {
            let high = if address != index {
                address
            } else {
                self.fresh_virtual_general()
            };
            self.emit_address_high(high, name);
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: index,
                    s: index,
                    shift,
                });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high,
                immediate: 0,
            });
        }
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: GENERAL_SCRATCH,
            b: index,
        });
        Ok(())
    }
}
