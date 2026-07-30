//! String-literal emission and interning.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Address a full-data string through an already-retained translation-unit
    /// `.data` base. The string's final symbol and section displacement are
    /// assigned after unit-wide pooling, so this emits one D-form instruction
    /// with a late symbolic fixup.
    pub(crate) fn emit_data_anchor_string_literal(
        &mut self,
        bytes: &[u8],
        destination: u8,
    ) -> bool {
        let Some(base) = self
            .data_section_anchor
            .as_ref()
            .and_then(|anchor| anchor.register)
        else {
            return false;
        };
        let placeholder = self.string_literal_placeholder(bytes);
        self.record_data_section_symbol_displacement(&placeholder);
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: base,
            immediate: 0,
        });
        true
    }

    pub(crate) fn loop_assertion_string_high_home(&self, bytes: &[u8]) -> Option<u8> {
        self.loop_assertion_string_highs
            .iter()
            .find_map(|(candidate, home)| (candidate.as_slice() == bytes).then_some(*home))
    }

    pub(crate) fn emit_loop_assertion_string_highs(&mut self) {
        if self.loop_assertion_string_highs_emitted {
            return;
        }
        self.loop_assertion_string_highs_emitted = true;
        for (bytes, home) in self.loop_assertion_string_highs.clone() {
            let placeholder = self.string_literal_placeholder(&bytes);
            self.emit_address_high(home, &placeholder);
        }
    }

    /// A string literal in expression position: intern it into the function's pooled
    /// `@N` strings (deduplicated by bytes), then load that object's address. Under
    /// small-data addressing this is `addi d,0,0` + an `R_PPC_EMB_SDA21` relocation;
    /// absolute addressing uses the ordinary `lis`/`addi` address pair. Both paths
    /// target a placeholder `@@strN` name, which the unit's string resolver rewrites
    /// to the real `@N`.
    pub(crate) fn emit_string_literal(&mut self, bytes: &[u8], destination: u8) -> Compilation<()> {
        let placeholder = self.string_literal_placeholder(bytes);
        if self.behavior.string_literals_packed {
            self.output.packed_string_literals = true;
            self.emit_string_address(&placeholder, destination);
            return Ok(());
        }
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                // A string within the small-data threshold (≤ 8 bytes incl. the NUL) lands in
                // `.sdata` and is reached with a single SDA21 `li`; a larger one lands in `.data`
                // (the writer routes by size) and is reached with ADDR16 `lis`/`addi` (`@ha`/`@l`),
                // exactly like a large global array's base.
                if bytes.len() + 1 > 8 {
                    self.emit_string_address(&placeholder, destination);
                } else {
                    self.record_relocation(RelocationKind::EmbSda21, &placeholder);
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: destination,
                        a: 0,
                        immediate: 0,
                    });
                }
                // The `@@str{index}` placeholder is resolved to the function's per-function `@N`
                // string symbol by the unit's string resolver (apps/mwcc), which places each
                // function's strings at the FRONT of its anonymous-`@N` block (before its constants
                // and unwind entries) and defers the not-yet-modeled cases (file-scope strings, or a
                // function that also has a jump table).
                Ok(())
            }
            GlobalAddressing::Absolute => {
                self.emit_string_address(&placeholder, destination);
                Ok(())
            }
        }
    }

    /// Form an absolute string address in `destination`. r0 can hold the final
    /// value but cannot serve as the base of `addi`, so scratch-valued stores
    /// keep the high half in a short-lived allocatable GPR.
    fn emit_string_address(&mut self, placeholder: &str, destination: u8) {
        let high = if destination == GENERAL_SCRATCH {
            self.fresh_virtual_general()
        } else {
            destination
        };
        self.emit_address_high(high, placeholder);
        self.emit_string_address_low(placeholder, high, destination);
    }

    /// Return the resolver placeholder for an interned string. Call-argument
    /// schedulers use this when MWCC separates an address's high and low halves
    /// with independent argument setup.
    pub(crate) fn string_literal_placeholder(&mut self, bytes: &[u8]) -> String {
        let index = self.intern_string_literal(bytes);
        format!("@@str{index}")
    }

    /// Finish an absolute string address in a destination which may differ from
    /// the register holding its high half.
    pub(crate) fn emit_string_address_low(&mut self, placeholder: &str, base: u8, destination: u8) {
        self.record_relocation(RelocationKind::Addr16Lo, placeholder);
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: base,
            immediate: 0,
        });
    }

    /// Intern a string literal into the function's pooled list (by bytes), returning
    /// its index. The unit-wide resolver assigns the `@N` names after lowering.
    pub(crate) fn intern_string_literal(&mut self, bytes: &[u8]) -> usize {
        if let Some(index) = self
            .output
            .string_literals
            .iter()
            .position(|existing| existing.as_slice() == bytes)
        {
            return index;
        }
        self.output.string_literals.push(bytes.to_vec());
        let index = self.output.string_literals.len() - 1;
        if let Some(symbol) = self.inline_string_symbols.get(bytes) {
            self.output
                .string_literal_symbols
                .insert(index, symbol.clone());
        }
        index
    }
}
