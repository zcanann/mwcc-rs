//! Array-to-pointer decay for file-scope arrays.
//!
//! A bare global-array name is an address value, never a load of element zero.
//! Most expression destinations can materialize that address in place. The
//! compiler scratch register is r0, however, and PowerPC `addi` treats r0 as a
//! literal-zero base. In that case mwcc forms the high half in a temporary and
//! writes the completed address to r0.

use super::*;

impl Generator {
    /// Emit `pointer_global = array_global` under absolute addressing. mwcc
    /// keeps the source and destination address transaction together. With the
    /// scheduler enabled their independent high halves overlap; `-schedule off`
    /// completes the source first and can reuse its register for the target when
    /// the source value has moved to r0.
    pub(crate) fn try_emit_global_array_decay_store(
        &mut self,
        target: &str,
        target_pointee: Pointee,
        value: &Expression,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::Absolute {
            return Ok(false);
        }
        let Expression::Variable(array) = value else {
            return Ok(false);
        };
        if !self.global_array_sizes.contains_key(array.as_str()) {
            return Ok(false);
        }

        let direct = self.behavior.global_array_decay_store_style
            == GlobalArrayDecayStoreStyle::DirectAddress;
        if !self.behavior.scheduler_enabled && !direct {
            let address = self.lowest_free_general()?;
            self.emit_address_high(address, array);
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: address,
                immediate: 0,
            });
            self.emit_address_high(address, target);
            if self.behavior.absolute_access_style
                == mwcc_versions::AbsoluteAccessStyle::MaterializedAddress
            {
                self.emit_address_low(address, target);
            } else {
                self.record_relocation(RelocationKind::Addr16Lo, target);
            }
            self.output.instructions.push(displacement_store(
                target_pointee,
                GENERAL_SCRATCH,
                address,
                0,
            )?);
            return Ok(true);
        }

        // Reserve the destination first so its longer lifetime gets the lowest
        // free physical register. The array high half uses the next register,
        // although its instruction issues first.
        let target_base = self.lowest_free_general()?;
        let restore_target = self.reserved.insert(target_base);
        let array_high = self.lowest_free_general()?;
        let restore_array = self.reserved.insert(array_high);
        self.emit_address_high(array_high, array);
        if self.behavior.scheduler_enabled {
            self.emit_address_high(target_base, target);
        }
        self.record_relocation(RelocationKind::Addr16Lo, array);
        let source = if direct {
            array_high
        } else {
            GENERAL_SCRATCH
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: source,
            a: array_high,
            immediate: 0,
        });
        if !self.behavior.scheduler_enabled {
            self.emit_address_high(target_base, target);
        }
        if self.behavior.absolute_access_style
            == mwcc_versions::AbsoluteAccessStyle::MaterializedAddress
        {
            self.emit_address_low(target_base, target);
        } else {
            self.record_relocation(RelocationKind::Addr16Lo, target);
        }
        self.output.instructions.push(displacement_store(
            target_pointee,
            source,
            target_base,
            0,
        )?);
        if restore_array {
            self.reserved.remove(&array_high);
        }
        if restore_target {
            self.reserved.remove(&target_base);
        }
        Ok(true)
    }

    pub(crate) fn emit_global_array_decay(
        &mut self,
        name: &str,
        total_size: u32,
        destination: u8,
    ) -> Compilation<()> {
        if destination != GENERAL_SCRATCH {
            return self.emit_global_array_base(name, total_size, destination);
        }

        let small = self.behavior.global_addressing == GlobalAddressing::SmallData
            && total_size <= 8;
        if small {
            self.record_relocation(RelocationKind::EmbSda21, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: 0,
                immediate: 0,
            });
            return Ok(());
        }

        let high = self.fresh_virtual_general();
        self.emit_address_high(high, name);
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: high,
            immediate: 0,
        });
        Ok(())
    }
}
