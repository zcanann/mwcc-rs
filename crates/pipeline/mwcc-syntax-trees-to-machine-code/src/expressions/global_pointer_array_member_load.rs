//! Member loads through elements of file-scope pointer arrays.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit `global[index]->member`. The parser retains the pointed-to struct
    /// size on the member expression, but the global array itself is an array
    /// of four-byte pointers: index by four, load the pointer, then load its
    /// member. Treating the retained struct size as the array stride skips the
    /// mandatory pointer indirection.
    pub(crate) fn try_emit_global_pointer_array_member_load(
        &mut self,
        name: &str,
        total_size: u32,
        index: &Expression,
        member_offset: u32,
        pointee: Pointee,
        destination: u8,
    ) -> Compilation<bool> {
        if !matches!(
            self.globals.get(name),
            Some(Type::Pointer(_) | Type::StructPointer { .. })
        ) {
            return Ok(false);
        }
        let member_offset = i16::try_from(member_offset).map_err(|_| {
            Diagnostic::error("global pointer-array member displacement is out of range")
        })?;

        if let Some(index) = constant_value(index) {
            let pointer_offset = index
                .checked_mul(4)
                .and_then(|offset| i16::try_from(offset).ok())
                .ok_or_else(|| Diagnostic::error("global pointer-array index is out of range"))?;
            let pointer = self.fresh_virtual_general_preferring(3);
            self.emit_global_array_base(name, total_size, pointer)?;
            self.output.instructions.push(Instruction::LoadWord {
                d: pointer,
                a: pointer,
                offset: pointer_offset,
            });
            self.output.instructions.push(displacement_load(
                pointee,
                destination,
                pointer,
                member_offset,
            )?);
            return Ok(true);
        }

        let index = if let Ok(register) = self.general_register_of_leaf(index) {
            register
        } else {
            self.evaluate_general(index, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        };
        let scaled = self.fresh_virtual_general_preferring(4);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: index,
                shift: 2,
            });

        let pointer = self.fresh_virtual_general_preferring(3);
        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        if small {
            self.emit_global_array_base(name, total_size, pointer)?;
            self.output.instructions.push(Instruction::Add {
                d: pointer,
                a: pointer,
                b: scaled,
            });
        } else {
            self.emit_address_high(pointer, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: pointer,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::Add {
                d: pointer,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: pointer,
            a: pointer,
            offset: 0,
        });
        self.output.instructions.push(displacement_load(
            pointee,
            destination,
            pointer,
            member_offset,
        )?);
        Ok(true)
    }
}
