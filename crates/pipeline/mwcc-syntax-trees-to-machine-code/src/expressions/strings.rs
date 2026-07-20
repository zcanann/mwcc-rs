//! String-literal emission and interning.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A string literal in expression position: intern it into the function's pooled
    /// `@N` strings (deduplicated by bytes), then load that object's address. Under
    /// small-data addressing this is `addi d,0,0` + an `R_PPC_EMB_SDA21` relocation;
    /// absolute addressing uses the ordinary `lis`/`addi` address pair. Both paths
    /// target a placeholder `@@strN` name, which the unit's string resolver rewrites
    /// to the real `@N`.
    pub(crate) fn emit_string_literal(&mut self, bytes: &[u8], destination: u8) -> Compilation<()> {
        let index = self.intern_string_literal(bytes);
        let placeholder = format!("@@str{index}");
        match self.behavior.global_addressing {
            GlobalAddressing::SmallData => {
                // A string within the small-data threshold (≤ 8 bytes incl. the NUL) lands in
                // `.sdata` and is reached with a single SDA21 `li`; a larger one lands in `.data`
                // (the writer routes by size) and is reached with ADDR16 `lis`/`addi` (`@ha`/`@l`),
                // exactly like a large global array's base.
                if bytes.len() + 1 > 8 {
                    self.emit_address_high(destination, &placeholder);
                    self.record_relocation(RelocationKind::Addr16Lo, &placeholder);
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: destination,
                        a: destination,
                        immediate: 0,
                    });
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
                self.emit_address_high(destination, &placeholder);
                self.emit_address_low(destination, &placeholder);
                Ok(())
            }
        }
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
        self.output.string_literals.len() - 1
    }
}
