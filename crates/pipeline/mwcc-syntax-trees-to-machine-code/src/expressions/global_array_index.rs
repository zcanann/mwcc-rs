//! Build-163 address schedules for variable-indexed file-scope arrays.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Load an offset-zero field from a variable-indexed global struct array into
    /// r0 under the legacy explicit-address schedule. The ordinary address helper
    /// assumes its destination can also hold the element address; r0 cannot (and
    /// is already the low-half staging register). Keep the address and scaled
    /// index in separate virtual GPRs instead:
    /// `lis hi; slwi scaled,index; addi r0,hi,@l; add hi,r0,scaled; lwz r0,0(hi)`.
    pub(crate) fn emit_legacy_global_struct_array_scratch_load(
        &mut self,
        name: &str,
        total_size: u32,
        index: u8,
        stride: u32,
        member_offset: u32,
        pointee: Pointee,
    ) -> Compilation<bool> {
        if self.behavior.global_array_index_style
            != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
            || !stride.is_power_of_two()
            || member_offset != 0
        {
            return Ok(false);
        }
        // These homes stay physical because the legacy schedule deliberately
        // retains the now-dead scaled-index register as the later boolean result
        // lane. Keep both distinct from the live source index and from each other.
        let high = self.free_general_excluding(index)?;
        let scaled = self.free_general_excluding_two(index, high)?;
        self.emit_address_high(high, name);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: index,
                shift: stride.trailing_zeros() as u8,
            });
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: high,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: high,
            a: GENERAL_SCRATCH,
            b: scaled,
        });
        self.output.instructions.push(displacement_load(
            pointee,
            GENERAL_SCRATCH,
            high,
            0,
        )?);
        Ok(true)
    }

    /// Build 163 materializes the low relocated byte-table base in r0, adds the
    /// unscaled (optionally u8-normalized) index to form one element address,
    /// then loads through displacement zero instead of using `lbzx`.
    pub(crate) fn emit_legacy_global_byte_array_variable_load(
        &mut self,
        name: &str,
        total_size: u32,
        pointee: Pointee,
        index: u8,
        normalize_unsigned_byte: bool,
        destination: u8,
    ) -> Compilation<bool> {
        if self.behavior.global_array_index_style
            != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
        {
            return Ok(false);
        }
        let high = self.fresh_virtual_general();
        self.emit_address_high(high, name);
        if normalize_unsigned_byte {
            self.output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: index,
                    s: index,
                    clear: 24,
                });
        }
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: high,
            immediate: 0,
        });
        let address = if destination == GENERAL_SCRATCH {
            index
        } else {
            destination
        };
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: GENERAL_SCRATCH,
            b: index,
        });
        self.output
            .instructions
            .push(displacement_load(pointee, destination, address, 0)?);
        Ok(true)
    }

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

    /// Form the address of a variable-indexed global struct-array element using
    /// build 163's offset-sensitive schedule. A zero-offset member scales into
    /// the eventual address register between `lis` and `addi`; a nonzero member
    /// first materializes the base, then scales through r0.
    pub(crate) fn emit_legacy_global_struct_array_address(
        &mut self,
        name: &str,
        total_size: u32,
        index: u8,
        stride: u32,
        member_offset: u32,
        address: u8,
    ) -> Compilation<bool> {
        if self.behavior.global_array_index_style
            != mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
            || (self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8)
            || !stride.is_power_of_two()
        {
            return Ok(false);
        }
        let high = self.fresh_virtual_general();
        self.emit_address_high(high, name);
        let shift = stride.trailing_zeros() as u8;
        if member_offset == 0 {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: address,
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
                d: address,
                a: GENERAL_SCRATCH,
                b: address,
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
                d: address,
                a: high,
                b: GENERAL_SCRATCH,
            });
        }
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
