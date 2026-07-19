//! SDA/absolute global loads and stores, global address classification.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// The register holding the value to store: a leaf stays in its own register,
    /// anything else is computed into the scratch (`li r0,0; stw r0,…`,
    /// `add r0,…; stw r0,…`) ahead of the store.
    /// Whether `expression` is `&global` — the address of a data global (not a
    /// frame-resident local). Used to defer the not-yet-scaled `&global +/- n`.
    pub(crate) fn is_global_address_of(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::AddressOf { operand }
            if matches!(operand.as_ref(), Expression::Variable(name)
                if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str())))
    }

    /// Whether `expression` is `&global +/- n` — the global-address pointer arithmetic
    /// that materializes as `li rD,0; addi rD,rD,k`.
    pub(crate) fn is_global_address_arithmetic(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
            if self.is_global_address_of(left) || self.is_global_address_of(right))
    }

    /// Load a file-scope global into `destination`. Under small-data addressing a
    /// single instruction carries the `0(r0)` placeholder an `R_PPC_EMB_SDA21`
    /// relocation fills (r13 + the small-data offset); under absolute addressing
    /// (`-sdata 0`) the address is materialized with a `lis`/`addi` pair (see
    /// [`Self::emit_global_load_absolute`]). The load is chosen by the global's type.
    pub(crate) fn emit_global_load(&mut self, name: &str, destination: u8) -> Compilation<()> {
        // A FUNCTION name in value position (a callback argument — `reg(cb, 5)`) is
        // its ADDRESS: an ADDR16 lis/addi pair against the function symbol (text
        // symbols are never small-data, so this bypasses the SDA path). Selection
        // emits the NATURAL pair; the measured interleave — the lis in the argument
        // run ahead of the prologue's LR store, the addi after it — is the
        // save-scheduler's (the lis is an `a == 0` load-immediate form it hoists).
        if !self.globals.contains_key(name) && self.call_return_types.contains_key(name) {
            self.record_relocation(RelocationKind::Addr16Ha, name);
            self.output
                .instructions
                .push(Instruction::AddImmediateShifted {
                    d: destination,
                    a: 0,
                    immediate: 0,
                });
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: destination,
                a: destination,
                immediate: 0,
            });
            return Ok(());
        }
        self.emit_global_load_value(name, destination)?;
        // A signed `char` global promotes to int with a trailing sign-extension:
        // `lbz` zero-extends the byte, so the value must be re-signed (`extsb`). In a
        // truncation context (the consumer re-narrows the result — a narrow return or a
        // narrow store of a truncation-safe op) the extsb is redundant and mwcc omits it:
        // `gc += 1` is `lbz r3; addi r0,r3,1; stb r0`, the byte store dropping the high bits.
        if self.global_char_extend(name)? && !self.narrow_truncation_context {
            self.emit_widen(destination, destination, 8, true);
        }
        Ok(())
    }

    /// Load a global's value *without* the signed-char promotion — just the
    /// addressing sequence and the load. The two-narrow-global path loads both
    /// operands before extending either, matching mwcc's batched schedule, so it
    /// drives the load and the extension separately through this and
    /// [`Self::global_char_extend`].
    pub(crate) fn emit_global_load_value(
        &mut self,
        name: &str,
        destination: u8,
    ) -> Compilation<()> {
        let global_type = *self
            .globals
            .get(name)
            .ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                self.record_relocation(RelocationKind::EmbSda21, name);
                let instruction = self.global_load_instruction(global_type, destination, 0)?;
                self.output.instructions.push(instruction);
            }
            GlobalAddressing::Absolute => {
                self.emit_global_load_absolute(name, global_type, destination)?
            }
        }
        Ok(())
    }

    /// Whether reading global `name` needs a trailing `extsb`. Plain `char` has
    /// already been resolved by the parser, so `Type::Char` means a signed byte
    /// here, including an explicit `signed char` under build 53.
    pub(crate) fn global_char_extend(&self, name: &str) -> Compilation<bool> {
        let global_type = *self
            .globals
            .get(name)
            .ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        Ok(global_type == Type::Char)
    }

    /// The type-appropriate load of a global from base register `a` (displacement
    /// zero): the small-data and absolute paths share the instruction choice and
    /// differ only in how `a`/the relocation are set up.
    pub(crate) fn global_load_instruction(
        &self,
        global_type: Type,
        d: u8,
        a: u8,
    ) -> Compilation<Instruction> {
        Ok(match global_type {
            Type::Int | Type::UnsignedInt => Instruction::LoadWord { d, a, offset: 0 },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZero { d, a, offset: 0 },
            Type::Short => Instruction::LoadHalfwordAlgebraic { d, a, offset: 0 },
            Type::UnsignedShort => Instruction::LoadHalfwordZero { d, a, offset: 0 },
            Type::Float => Instruction::LoadFloatSingle { d, a, offset: 0 },
            Type::Double => Instruction::LoadFloatDouble { d, a, offset: 0 },
            // A pointer global is a 32-bit address word.
            Type::Pointer(_) | Type::StructPointer { .. } => {
                Instruction::LoadWord { d, a, offset: 0 }
            }
            other => {
                return Err(Diagnostic::error(format!(
                    "global of type {other:?} is not supported yet"
                )))
            }
        })
    }

    /// Emit `lis base, name@ha` — the high-adjusted half of an absolute address,
    /// with its `R_PPC_ADDR16_HA` relocation. `base` must never be r0: an `addi`
    /// or load based on r0 reads literal zero, not the register (the `li` trap).
    pub(crate) fn emit_address_high(&mut self, base: u8, name: &str) {
        self.record_relocation(RelocationKind::Addr16Ha, name);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(base, 0));
    }

    /// Finish absolute address formation with `addi base,base,name@l`.
    /// O0 keeps this source-order step separate from the following access.
    pub(crate) fn emit_address_low(&mut self, base: u8, name: &str) {
        self.record_relocation(RelocationKind::Addr16Lo, name);
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: base,
            immediate: 0,
        });
    }

    /// Load a global under absolute (`-sdata 0`) addressing. mwcc's address-mode
    /// selection follows from r0 never being a usable base: when the destination
    /// is a non-r0 GPR, the address materializes into it (`lis dest; addi dest;
    /// load 0(dest)`) — base and destination coincide, so nothing folds; a float
    /// destination (an FPR) takes a separate free GPR base with `name@l` folded
    /// into the load. An integer load whose destination is the scratch r0 would
    /// need a separate base that avoids the (un-reserved) sibling operand — that
    /// liveness is the register allocator's to track, so it defers for now.
    pub(crate) fn emit_global_load_absolute(
        &mut self,
        name: &str,
        global_type: Type,
        destination: u8,
    ) -> Compilation<()> {
        if global_type == Type::Float {
            let base = self.lowest_free_general()?;
            self.emit_address_high(base, name);
            if self.behavior.absolute_access_style
                == mwcc_versions::AbsoluteAccessStyle::MaterializedAddress
            {
                self.emit_address_low(base, name);
            } else {
                self.record_relocation(RelocationKind::Addr16Lo, name);
            }
            let load = self.global_load_instruction(global_type, destination, base)?;
            self.output.instructions.push(load);
            return Ok(());
        }
        if destination != GENERAL_SCRATCH {
            self.emit_address_high(destination, name);
            self.emit_address_low(destination, name);
            let load = self.global_load_instruction(global_type, destination, destination)?;
            self.output.instructions.push(load);
            return Ok(());
        }
        // destination == r0 (a scratch operand): a separate base GPR holds the
        // address and `@l` folds into the load. The base is the lowest free GPR,
        // which avoids any sibling operand the caller has reserved — r0 itself can
        // never be the base (the literal-zero trap).
        let base = self.lowest_free_general()?;
        self.emit_address_high(base, name);
        if self.behavior.absolute_access_style
            == mwcc_versions::AbsoluteAccessStyle::MaterializedAddress
        {
            self.emit_address_low(base, name);
        } else {
            self.record_relocation(RelocationKind::Addr16Lo, name);
        }
        let load = self.global_load_instruction(global_type, destination, base)?;
        self.output.instructions.push(load);
        Ok(())
    }

    /// Store `source` to a file-scope global. Small-data uses the `0(r0)` SDA21
    /// placeholder; absolute addressing materializes the high half into a free
    /// base GPR (avoiding the value register) and folds `name@l` into the store.
    pub(crate) fn emit_global_store(
        &mut self,
        name: &str,
        pointee: Pointee,
        source: u8,
    ) -> Compilation<()> {
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                self.record_relocation(RelocationKind::EmbSda21, name);
                self.output
                    .instructions
                    .push(displacement_store(pointee, source, 0, 0)?);
            }
            GlobalAddressing::Absolute => {
                let base = self.free_general_excluding(source)?;
                self.emit_address_high(base, name);
                if self.behavior.absolute_access_style
                    == mwcc_versions::AbsoluteAccessStyle::MaterializedAddress
                {
                    self.emit_address_low(base, name);
                } else {
                    self.record_relocation(RelocationKind::Addr16Lo, name);
                }
                self.output
                    .instructions
                    .push(displacement_store(pointee, source, base, 0)?);
            }
        }
        Ok(())
    }

    /// The register a just-stored global is still live in, if reading it now would
    /// reuse it correctly: the value must not have been touched since the store (no
    /// instruction emitted), and a scratch (`r0`) value can only feed a consumer
    /// that does not use it as an `addi` base (where `r0` reads as literal zero).
    /// Build 163 deliberately declines this optimization and reloads memory.
    pub(crate) fn live_global_register(&self, name: &str, prefer_destination: bool) -> Option<u8> {
        if self.behavior.stored_global_read_style
            == mwcc_versions::StoredGlobalReadStyle::ReloadAfterStore
        {
            return None;
        }
        let &(register, at) = self.stored_globals.get(name)?;
        if at != self.output.instructions.len() {
            return None;
        }
        if register == GENERAL_SCRATCH && prefer_destination {
            return None;
        }
        Some(register)
    }
}
