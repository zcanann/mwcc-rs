//! Operand register placement and scratch selection.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Load a signed-byte operand into the scratch and sign-extend it in place (`lbz r0; extsb
    /// r0,r0`), returning the scratch — for the unary/shift idioms (`neg`, `not`, `srawi`) that
    /// read their operand from r0, where mwcc keeps it. (`addi` cannot take r0 as a source — it
    /// means literal zero — so the Add/Subtract path keeps the value in the destination via
    /// place_operand instead.) Returns None for a non-signed-byte operand or a scratch destination,
    /// so the caller falls back to its normal place_operand/place_operand_or_scratch path.
    pub(crate) fn signed_byte_scratch_source(
        &mut self,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<Option<u8>> {
        if destination != GENERAL_SCRATCH && self.is_signed_byte_load(operand)? {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            self.emit_widen(GENERAL_SCRATCH, GENERAL_SCRATCH, 8, true);
            Ok(Some(GENERAL_SCRATCH))
        } else {
            Ok(None)
        }
    }

    pub(crate) fn place_operand(
        &mut self,
        operand: &Expression,
        destination: u8,
        prefer_destination: bool,
    ) -> Compilation<Option<u8>> {
        // A same-width 32-bit integer cast (`(unsigned)x` / `(int)u`) is a bit-exact
        // reinterpretation — place its operand directly rather than copying it
        // through the scratch. The consumer takes the signedness from the cast, so
        // e.g. `(unsigned)x >> n` stays a single `srwi`.
        if let Expression::Cast {
            target_type,
            operand: inner,
        } = operand
        {
            if target_type.width() == 32 && self.plain_integer_leaf_register(inner).is_some() {
                return self.place_operand(inner, destination, prefer_destination);
            }
        }
        // A SIGNED CHAR load (member `p->x`, element `a[i]`, deref `*p`) used as an integer
        // operand needs the sign-extension its `lbz`/`lbzx` does not carry — `p->x + 1` is
        // `lbz r0; extsb r3,r0; addi`, and every non-truncating operator (`+ - * << >> | ^ /`,
        // unary, compare) miscompiles on the raw zero-extended byte (`0xFF` reads 255, not -1).
        // mwcc loads it into the scratch and sign-extends into the destination (`lbz r0;
        // extsb d,r0`); the consumer then reads the sign-extended value from the destination. A
        // TRUNCATING consumer (a fitting mask) sets narrow_truncation_context and reads the raw byte
        // — exempt; a SHORT load sign-extends (`lha`) and the direct `return p->x` uses
        // evaluate_general — both unaffected. The scratch destination (value/store context) uses a
        // different mwcc layout, so it still defers there.
        if !self.narrow_truncation_context && self.is_signed_byte_load(operand)? {
            if destination == GENERAL_SCRATCH {
                return Err(Diagnostic::error(
                    "a signed char load operand needs a sign-extension (roadmap)",
                ));
            }
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            self.emit_widen(destination, GENERAL_SCRATCH, 8, true);
            return Ok(Some(destination));
        }
        if let Expression::Variable(name) = operand {
            // A scalar whose address is taken has no register home. Reload it
            // into the consumer's preferred working register using the slot's
            // source type (which may be narrower than its allocation lane).
            if self
                .frame_slots
                .get(name)
                .is_some_and(|slot| !slot.is_array)
            {
                let target = if prefer_destination {
                    destination
                } else {
                    GENERAL_SCRATCH
                };
                self.evaluate_general(operand, target)?;
                return Ok(Some(target));
            }
            // A global is loaded into the consumer's register (the destination for
            // addi-family consumers, otherwise the scratch), like a dereference —
            // unless it was just stored and is still live in a register, which is
            // reused (no reload), reproducing mwcc.
            if !self.locations.contains_key(name) && self.globals.contains_key(name.as_str()) {
                if let Some(register) = self.live_global_register(name, prefer_destination) {
                    return Ok(Some(register));
                }
                let target = if prefer_destination {
                    destination
                } else {
                    GENERAL_SCRATCH
                };
                self.emit_global_load(name, target)?;
                return Ok(Some(target));
            }
            let location = self
                .locations
                .get(name)
                .ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
            let (register, width, signed) = (location.register, location.width, location.signed);
            if width == 32 {
                return Ok(Some(register));
            }
            // In a narrow-truncation context the result is truncated, so a narrow
            // operand of a truncation-safe op is read raw (no leading extension) — the
            // final truncation makes it redundant, matching mwcc.
            if self.narrow_truncation_context {
                return Ok(Some(register));
            }
            // A narrow operand is width-extended to 32 bits before use. The
            // extension lands in the consumer's working register: the destination
            // for addi-family consumers that keep their operand in place, otherwise
            // the scratch (mwcc routes `extsb r0,rX` ahead of an `rlwinm`/`mulli`).
            let target = if prefer_destination {
                destination
            } else {
                GENERAL_SCRATCH
            };
            self.emit_widen(target, register, width, signed);
            return Ok(Some(target));
        }
        // A call result lands in r3, its home. Let the consumer read it there rather than
        // bouncing it through the scratch with a move mwcc does not emit: place it in a
        // fresh virtual the allocator colors to r3 (the resulting `mr r3,r3` coalesces away).
        // For a tail consumer (destination already r3) the move is a self-move that vanishes;
        // for a scratch consumer it keeps the operand in r3, matching mwcc's `<op> d,r3,…`.
        if matches!(operand, Expression::Call { .. }) {
            let home = self.fresh_virtual_general();
            self.evaluate_general(operand, home)?;
            return Ok(Some(home));
        }
        if prefer_destination {
            self.evaluate_general(operand, destination)?;
            Ok(Some(destination))
        } else {
            if !fits_single_scratch(operand, true) {
                return Ok(None);
            }
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            Ok(Some(GENERAL_SCRATCH))
        }
    }

    /// Place a single consumed operand: in its own register if a leaf, otherwise
    /// computed into the scratch. A complex operand that needs temporaries beyond
    /// the scratch is no longer a deferral — the allocator supplies them (its
    /// inner sub-expressions emit virtuals), so the operand simply evaluates into
    /// the scratch like mwcc does (`mullw r0,...; neg r3,r0`). Used by the unary
    /// operators and the compare-against-zero idioms.
    pub(crate) fn place_operand_or_scratch(
        &mut self,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<u8> {
        match self.place_operand(operand, destination, false)? {
            Some(source) => Ok(source),
            None => {
                self.evaluate_general(operand, GENERAL_SCRATCH)?;
                Ok(GENERAL_SCRATCH)
            }
        }
    }
}
