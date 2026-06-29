//! Constant folding, immediate forms, complement fusion, and shifts.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    /// If one operand is `~leaf` and the other is a leaf, emit `andc`/`orc`.
    pub(crate) fn try_emit_complement_logical(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> bool {
        // Both operands complemented — De Morgan folds to a single op: `~a & ~b` is
        // `nor(a,b)` and `~a | ~b` is `nand(a,b)`.
        if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::BitOr) {
            if let (Some(left_name), Some(right_name)) = (complemented_leaf_name(left), complemented_leaf_name(right)) {
                if let (Some(left_register), Some(right_register)) = (self.lookup_general(left_name), self.lookup_general(right_name)) {
                    self.output.instructions.push(match operator {
                        BinaryOperator::BitAnd => Instruction::Nor { a: destination, s: left_register, b: right_register },
                        _ => Instruction::Nand { a: destination, s: left_register, b: right_register },
                    });
                    return true;
                }
            }
        }
        let (kept_expression, complemented_name) = if let Some(name) = complemented_leaf_name(right) {
            (left, name)
        } else if let Some(name) = complemented_leaf_name(left) {
            (right, name)
        } else {
            return false;
        };
        let (Some(kept_name), Some(complemented_register)) = (leaf_name(kept_expression), self.lookup_general(complemented_name)) else {
            return false;
        };
        let Some(kept_register) = self.lookup_general(kept_name) else {
            return false;
        };
        self.output.instructions.push(match operator {
            BinaryOperator::BitAnd => Instruction::AndComplement { a: destination, s: kept_register, b: complemented_register },
            _ => Instruction::OrComplement { a: destination, s: kept_register, b: complemented_register },
        });
        true
    }

    /// `L | R` where each operand is a contiguous bit field of a leaf variable
    /// (a constant shift or a mask) and the two fields tile the word exactly.
    /// This one shape subsumes a constant rotate `(x<<c)|(x>>(32-c))`, a
    /// sign/magnitude mask merge `(a&m)|(b&~m)`, and any mix such as
    /// `(a<<16)|(b&0xffff)`. mwcc computes the OR's **right** operand (the base)
    /// directly into the destination, then inserts the **left** operand's field
    /// with `rlwimi`, preserving the inserted value in r0 first when computing the
    /// base would otherwise clobber it. A logical right-shift operand (the base's
    /// `srwi`) requires an unsigned value.
    pub(crate) fn try_emit_field_merge(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        let (Some((insert_value, insert_kind, insert_begin, insert_end)), Some((base_value, base_kind, base_begin, base_end))) =
            (as_field(left), as_field(right))
        else {
            return Ok(false);
        };
        // The two fields must cover the whole word with no overlap.
        let insert_mask = run_mask(insert_begin, insert_end);
        let base_mask = run_mask(base_begin, base_end);
        if insert_mask & base_mask != 0 || insert_mask | base_mask != 0xFFFF_FFFF {
            return Ok(false);
        }
        // A `srwi` (the base's logical right shift) needs an unsigned operand; the
        // inserted `>>` reduces to a sign-agnostic rlwimi, but require it too to be safe.
        if matches!(insert_kind, FieldSource::ShiftRight(_)) && self.signedness_of(insert_value)? {
            return Ok(false);
        }
        if matches!(base_kind, FieldSource::ShiftRight(_)) && self.signedness_of(base_value)? {
            return Ok(false);
        }
        let (Some(insert_register), Some(base_register)) = (
            leaf_name(insert_value).and_then(|name| self.lookup_general(name)),
            leaf_name(base_value).and_then(|name| self.lookup_general(name)),
        ) else {
            return Ok(false);
        };
        // Computing the base writes the destination, except an unshifted mask whose
        // value already sits there.
        let base_writes_destination = !(matches!(base_kind, FieldSource::Mask) && base_register == destination);
        // Preserve the inserted value when the base computation would overwrite it.
        let insert_source = if base_writes_destination && insert_register == destination {
            self.output.instructions.push(Instruction::move_register(GENERAL_SCRATCH, insert_register));
            GENERAL_SCRATCH
        } else {
            insert_register
        };
        // The base, computed directly into the destination.
        match base_kind {
            FieldSource::ShiftLeft(n) => self.output.instructions.push(Instruction::ShiftLeftImmediate { a: destination, s: base_register, shift: n }),
            FieldSource::ShiftRight(n) => self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: destination, s: base_register, shift: n }),
            FieldSource::Mask => {
                if base_register != destination {
                    self.output.instructions.push(Instruction::move_register(destination, base_register));
                }
            }
        }
        // The inserted field via rlwimi: `x << n` occupies [0, 31-n] at rotate n;
        // `x >> n` occupies [n, 31] at rotate 32-n; a mask keeps its run at rotate 0.
        let rotate = match insert_kind {
            FieldSource::ShiftLeft(n) => n,
            FieldSource::ShiftRight(n) => 32 - n,
            FieldSource::Mask => 0,
        };
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: destination, s: insert_source, shift: rotate, begin: insert_begin, end: insert_end });
        Ok(true)
    }

    /// `(load_a & maskA) | (load_b & maskB)` with complementary masks where the
    /// operands are memory loads (e.g. the `__HI`/`__LO` pointer-pun merge in
    /// copysign). mwcc loads the inserted (left) operand first into a temporary,
    /// the base (right) operand into the destination, then merges with `rlwimi`.
    pub(crate) fn try_emit_field_merge_loads(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        let (Some((insert_load, insert_mask)), Some((base_load, base_mask))) = (as_masked_load(left), as_masked_load(right)) else {
            return Ok(false);
        };
        if insert_mask & base_mask != 0 || insert_mask | base_mask != 0xFFFF_FFFF {
            return Ok(false);
        }
        let Some((begin, end)) = mask_to_run(insert_mask) else {
            return Ok(false);
        };
        let insert_register = self.fresh_virtual_general_avoiding(vec![destination]);
        self.evaluate_general(insert_load, insert_register)?;
        self.evaluate_general(base_load, destination)?;
        self.output.instructions.push(Instruction::RotateAndMaskInsert { a: destination, s: insert_register, shift: 0, begin, end });
        Ok(true)
    }

    /// A shift fused with a mask collapses to one `rlwinm` (rotate-and-mask):
    ///   `(x << n) & m`, `(x >> n) & m`, `(x & m) << n`, `(x & m) >> n`.
    /// Each is `ROTL(x, r) & mask[begin,end]` for the right `r` and contiguous
    /// mask; the cases differ only in how `r` and the mask are derived. The masked
    /// region must avoid the bits the rotation wraps in (so the rotate equals the
    /// shift), and the logical-right forms require an unsigned value.
    pub(crate) fn try_emit_rotate_mask(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<bool> {
        let Some((value, rotate, begin, end, needs_unsigned)) = self.fused_rotate_mask(operator, left, right)? else {
            return Ok(false);
        };
        // The signedness guard is checked before any load is emitted so an
        // unsupported shape defers without leaving a stray instruction.
        if needs_unsigned && self.signedness_of(value)? {
            return Ok(false);
        }
        // A leaf rotates in place; otherwise place_operand resolves the value to an
        // existing register without recomputing it — a cast of a leaf (`(unsigned)x`)
        // keeps that leaf's register, a value-tracked global just stored stays live
        // in its register — and only a genuinely computed value (`a*b+c`) lands in
        // the scratch, matching mwcc's `rlwinm d,reg,…` / `<compute> r0; rlwinm d,r0`.
        let register = if let Some(register) = leaf_name(value).and_then(|name| self.lookup_general(name)) {
            register
        } else if self.is_simple_word_load(value) {
            self.evaluate_general(value, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            match self.place_operand(value, GENERAL_SCRATCH, false)? {
                Some(register) => register,
                None => return Ok(false),
            }
        };
        self.output.instructions.push(Instruction::RotateAndMask { a: destination, s: register, shift: rotate, begin, end });
        Ok(true)
    }

    /// Like `as_constant_shift`, but the shifted value may be a full-word memory
    /// load (`p[0] >> n`, `s->a << n`) as well as a leaf — `try_emit_rotate_mask`
    /// places a load into the scratch before the `rlwinm`. Kept local to the
    /// fusion so `as_constant_shift`/`as_field` stay leaf-only.
    fn constant_shift_placeable<'e>(&self, expression: &'e Expression) -> Option<(&'e Expression, bool, u8)> {
        let Expression::Binary { operator, left, right } = expression else { return None };
        // The shifted value may be a leaf, a full-word load, or a computed
        // expression — `try_emit_rotate_mask` evaluates a non-leaf into the scratch
        // before the `rlwinm`, matching mwcc's `<compute> r0; rlwinm d,r0,…`.
        match operator {
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight => {
                let amount = constant_value(right)?;
                ((1..=31).contains(&amount)).then(|| (left.as_ref(), *operator == BinaryOperator::ShiftLeft, amount as u8))
            }
            // `x / 2^n` is a logical right shift by `n` only for an UNSIGNED value
            // (a signed division rounds toward zero, not a floor) — `unsigned-rand`'s
            // `… / 65536 & 0x7fff` is the canonical `rlwinm` form.
            BinaryOperator::Divide => {
                let divisor = constant_value(right)?;
                if divisor > 1 && (divisor as u64).is_power_of_two() && self.signedness_of(left).ok() == Some(false) {
                    let shift = divisor.trailing_zeros();
                    return ((1..=31).contains(&shift)).then(|| (left.as_ref(), false, shift as u8));
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve a shift-and-mask expression to `(x, rotate, begin, end, needs_unsigned)`
    /// for the fused `rlwinm`, or `None` when the shape does not collapse cleanly.
    fn fused_rotate_mask<'e>(&self, operator: BinaryOperator, left: &'e Expression, right: &'e Expression) -> Compilation<Option<(&'e Expression, u8, u8, u8, bool)>> {
        let result = match operator {
            // `(x << n) & m` / `(x >> n) & m` — shift inside, mask outside.
            BinaryOperator::BitAnd => {
                let Some(mask) = constant_value(right) else { return Ok(None) };
                let mask = mask as u32;
                let Some((value, is_left, shift)) = self.constant_shift_placeable(left) else { return Ok(None) };
                if is_left {
                    // `x << n` zeroes the low n bits, so they cannot survive the mask.
                    let effective = mask & !((1u32 << shift) - 1);
                    let Some((begin, end)) = mask_to_run(effective) else { return Ok(None) };
                    Some((value, shift, begin, end, false))
                } else {
                    // `x >> n`: the mask must stay below the (possibly sign-extended)
                    // high n bits, so the rotate reproduces the shift for either sign.
                    if shift == 0 || mask >= (1u32 << (32 - shift)) {
                        return Ok(None);
                    }
                    let Some((begin, end)) = mask_to_run(mask) else { return Ok(None) };
                    Some((value, 32 - shift, begin, end, false))
                }
            }
            // `(x & m) << n` — mask inside, left shift outside; or a right shift
            // then a left shift `(x >> k) << n`.
            BinaryOperator::ShiftLeft => {
                let Some(shift) = constant_value(right) else { return Ok(None) };
                if !(1..=31).contains(&shift) {
                    return Ok(None);
                }
                let shift = shift as u8;
                if let Some((value, mask)) = as_masked_leaf(left).or_else(|| as_masked_load(left)) {
                    let Some((begin, end)) = mask_to_run(mask << shift) else { return Ok(None) };
                    Some((value, shift, begin, end, false))
                } else if let Some((value, false, inner)) = self.constant_shift_placeable(left) {
                    // `(x >> k) << n`: clears the low `n` bits. n == k is the
                    // round-to-multiple `clrrwi` (valid for either sign); n > k keeps
                    // the shifted value and needs an unsigned (logical) right shift.
                    if shift == inner {
                        Some((value, 0, 0, 31 - shift, false))
                    } else if shift > inner {
                        Some((value, shift - inner, 0, 31 - shift, true))
                    } else {
                        return Ok(None);
                    }
                } else {
                    return Ok(None);
                }
            }
            // `(x & m) >> n` — mask inside, right shift outside. The masked value
            // `x & m` is non-negative when the mask clears the sign bit, so the
            // shift is sign-agnostic and fuses for signed x too; only a mask that
            // reaches bit 31 needs an unsigned (logical) shift.
            BinaryOperator::ShiftRight => {
                let Some(shift) = constant_value(right) else { return Ok(None) };
                if !(1..=31).contains(&shift) {
                    return Ok(None);
                }
                // `x & m` for a leaf x, or `(p[0] & m)` / `(s->a & m)` for a load.
                let Some((value, mask)) = as_masked_leaf(left).or_else(|| as_masked_load(left)) else { return Ok(None) };
                let Some((begin, end)) = mask_to_run(mask >> shift) else { return Ok(None) };
                Some((value, (32 - shift) as u8, begin, end, mask & 0x8000_0000 != 0))
            }
            _ => None,
        };
        Ok(result)
    }

    /// Emit a right shift, choosing arithmetic (signed) or logical (unsigned)
    /// from the type of the shifted value.
    pub(crate) fn emit_shift_right(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)?;
        let d = destination;

        if let Some(amount) = constant_value(right) {
            if (1..=31).contains(&amount) {
                // An unsigned narrow value fuses extension and shift into one
                // rlwinm; a signed narrow value extends (extsb/extsh) then shifts.
                if let Ok((register, width, leaf_signed)) = self.leaf_info(left) {
                    if width < 32 && !leaf_signed {
                        if self.emit_narrow_unsigned_shift(d, register, width, false, amount as u8) {
                            return Ok(());
                        }
                        return Err(Diagnostic::error("narrow unsigned shift out of the single-rlwinm range (roadmap)"));
                    }
                }
                // The shifted value: a leaf stays put, a sub-expression goes to the
                // scratch (its temporaries are virtuals the allocator places).
                let source = self.place_operand_or_scratch(left, d)?;
                let shift = amount as u8;
                self.output.instructions.push(if signed {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: source, shift }
                } else {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: source, shift }
                });
                return Ok(());
            }
        }

        // Register form: a leaf value stays in its home register and shifts straight
        // into the destination (`srw d,a,n`, no redundant move); a sub-expression
        // evaluates into the destination. The shift amount goes in its own register.
        let source = self.place_operand_or_scratch(left, d)?;
        let amount = if is_complex(right) {
            if !fits_single_scratch(right, true) {
                return Err(Diagnostic::error("shift amount needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(right)?
        };
        self.output.instructions.push(if signed {
            Instruction::ShiftRightAlgebraicWord { a: d, s: source, b: amount }
        } else {
            Instruction::ShiftRightWord { a: d, s: source, b: amount }
        });
        Ok(())
    }

    /// Fold a constant operand into an immediate instruction. Returns whether an
    /// instruction was emitted; if the constant does not qualify (out of range,
    /// non-mask), returns false so the caller can stop honestly.
    pub(crate) fn try_emit_general_with_constant(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        // `-a +/- C` (negate of a leaf, plus/minus a constant) is `C - a` /
        // `-C - a`, a single `subfic a, ±C` (e.g. `-a - 1` -> `subfic r3,r3,-1`,
        // `-a + 1` -> `subfic r3,r3,1`), rather than a `neg` followed by `addi`.
        if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
            if let Expression::Unary { operator: UnaryOperator::Negate, operand } = left {
                if let (Some(register), Some(constant)) = (leaf_name(operand).and_then(|name| self.lookup_general(name)), constant_value(right)) {
                    let immediate = if operator == BinaryOperator::Subtract { constant.wrapping_neg() } else { constant };
                    if fits_signed_16(immediate) {
                        self.output.instructions.push(Instruction::SubtractFromImmediate { d: destination, a: register, immediate: immediate as i16 });
                        return Ok(true);
                    }
                }
            }
        }
        // variable op constant — subtraction becomes addition of the negation.
        if let Some(constant) = constant_value(right) {
            let (effective, value) = match operator {
                BinaryOperator::Subtract => (BinaryOperator::Add, -constant),
                other => (other, constant),
            };
            if self.emit_constant_form(effective, left, value, destination)? {
                return Ok(true);
            }
        }
        // constant op variable — only the commutative operators.
        if is_commutative(operator) {
            if let Some(constant) = constant_value(left) {
                if self.emit_constant_form(operator, right, constant, destination)? {
                    return Ok(true);
                }
            }
        }
        // `C - x` with a leaf `x`: `0 - x` is a `neg`, any other constant is a
        // single `subfic`.
        if operator == BinaryOperator::Subtract {
            if let (Some(constant), Some(register)) = (constant_value(left), leaf_name(right).and_then(|name| self.lookup_general(name))) {
                if constant == 0 {
                    self.output.instructions.push(Instruction::Negate { d: destination, a: register });
                    return Ok(true);
                }
                if fits_signed_16(constant) {
                    self.output.instructions.push(Instruction::SubtractFromImmediate { d: destination, a: register, immediate: constant as i16 });
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Apply `constant` to `variable` via the matching immediate instruction, if
    /// the constant qualifies. The operand is read from its own register (a leaf)
    /// or computed into `destination` (a sub-expression); the immediate then reads
    /// that source directly — `addi` must not take `r0` as its source, which would
    /// silently mean `li`.
    pub(crate) fn emit_constant_form(&mut self, operator: BinaryOperator, variable: &Expression, constant: i64, destination: u8) -> Compilation<bool> {
        // A SIGNED CHAR load (struct member `p->x`, array element `a[i]`/`a[2]`, or pointer
        // deref `*p`) promoted to int needs the sign-extension its `lbz`/`lbzx` does not carry
        // — `p->x + 1` is `lbz r0; extsb r3,r0; addi`. Nearly every operator (`+ - * << >> | ^
        // /`, a wide mask) miscompiles on the raw zero-extended byte (`0xFF` reads 255, not
        // -1); the return path adds the extsb separately, this operand path does not, and
        // mwcc's r0-load register choice is the keystone allocator's, so defer. A SHORT load
        // sign-extends on load (`lha`/`lhax`) and stays byte-exact, so only the byte case is
        // here. The sole exemption is a STRICT partial mask (`& 0xf`): the mask clears the
        // would-be sign-extended high bits, so the raw byte is already correct (`lbz; clrlwi`),
        // while `& 0xff` (mwcc drops the redundant mask) and `& 0x100` (reaches the sign bit)
        // both defer. An unsigned byte zero-extends on load and is not handled here.
        if self.is_signed_byte_load(variable)? {
            let is_fitting_mask = matches!(operator, BinaryOperator::BitAnd)
                && constant > 0
                && constant < 0xff;
            if !is_fitting_mask {
                return Err(Diagnostic::error("a signed char load promoted to int needs a sign-extension (roadmap)"));
            }
        }
        // Identity and strength-reduction folds.
        match (operator, constant) {
            (BinaryOperator::Add, 0) => {
                self.evaluate_general(variable, destination)?;
                return Ok(true);
            }
            (BinaryOperator::Multiply, 0) => {
                self.load_integer_constant(destination, 0);
                return Ok(true);
            }
            (BinaryOperator::Multiply, 1) => {
                self.evaluate_general(variable, destination)?;
                return Ok(true);
            }
            (BinaryOperator::Multiply, -1) => {
                let Some(source) = self.place_operand(variable, destination, false)? else {
                    return Ok(false);
                };
                self.output.instructions.push(Instruction::Negate { d: destination, a: source });
                return Ok(true);
            }
            _ => {}
        }

        // An add of a 32-bit constant too large for `addi` splits into `addis`
        // (high half) + `addi` (low half), the low half sign-extended so the high
        // half is carry-adjusted: `addis d,x,ha; addi d,d,lo`. Both ops read the
        // running register, which must not be the scratch — `addi/addis` with `r0`
        // as the source operand means literal zero, not r0's contents — so a
        // value/store context (destination r0) defers to the register allocator.
        if operator == BinaryOperator::Add
            && !fits_signed_16(constant)
            && destination != GENERAL_SCRATCH
            && (i32::MIN as i64..=u32::MAX as i64).contains(&constant)
        {
            let value = constant as u32;
            let low = value as u16 as i16;
            let high = ((value >> 16) as i16).wrapping_add(if value & 0x8000 != 0 { 1 } else { 0 });
            let Some(source) = self.place_operand(variable, destination, true)? else {
                return Ok(false);
            };
            self.output.instructions.push(Instruction::AddImmediateShifted { d: destination, a: source, immediate: high });
            // A zero low half needs no `addi` (mwcc emits the bare `addis`).
            if low != 0 {
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: low });
            }
            return Ok(true);
        }

        // A multiply by a constant too large for `mulli` loads the constant into a
        // register and uses `mullw`. mwcc materializes the constant in the scratch
        // via a free register: `lis free,ha; addi r0,free,lo; mullw d,x,r0`. Only a
        // leaf operand (which stays in its own register) is handled here; a loaded
        // operand (member/global) needs the register allocator.
        if operator == BinaryOperator::Multiply && !fits_signed_16(constant) {
            // A power-of-two factor is a left shift, even when it is too large for
            // `mulli` (e.g. `x * 65536` -> `slwi x, 16`). 2^31 does not fit a signed
            // int, so mwcc treats it as a wide constant (materialize + mullw) instead.
            if constant >= 2 && constant <= i32::MAX as i64 && (constant as u64).is_power_of_two() {
                let shift = constant.trailing_zeros() as u8;
                if let Ok((register, width, leaf_signed)) = self.leaf_info(variable) {
                    if width < 32 && !leaf_signed {
                        return Ok(self.emit_narrow_unsigned_shift(destination, register, width, true, shift));
                    }
                }
                let source = self.place_operand_or_scratch(variable, destination)?;
                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: destination, s: source, shift });
                return Ok(true);
            }
            let low = (constant as u32 & 0xffff) as i16;
            let high = ((constant as i32 - low as i32) >> 16) as i16;
            if let Ok(operand_register) = self.general_register_of_leaf(variable) {
                if low == 0 {
                    // No low half: build the constant straight in the scratch with a
                    // single `lis r0, high`, then multiply.
                    self.output.instructions.push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, high));
                    self.output.instructions.push(Instruction::MultiplyLow { d: destination, a: operand_register, b: GENERAL_SCRATCH });
                    return Ok(true);
                }
                // Leaf operand: it stays in its register; the constant is built in
                // the scratch via a free register.
                let free = self.free_general_excluding(operand_register)?;
                self.output.instructions.push(Instruction::load_immediate_shifted(free, high));
                self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: free, immediate: low });
                self.output.instructions.push(Instruction::MultiplyLow { d: destination, a: operand_register, b: GENERAL_SCRATCH });
                return Ok(true);
            }
            if self.is_global(variable) {
                // Global operand: mwcc builds the constant high in one register and
                // loads the global into another, then assembles the low half in the
                // scratch and multiplies: `lis t,ha; lwz g,sym; addi r0,t,lo; mullw
                // d,g,r0`. The high-temp and the load go to fresh virtuals so the
                // allocator keeps them distinct (and off the scratch) — the inline
                // version collided when the destination was the scratch.
                let name = leaf_name(variable).unwrap();
                let high_temp = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::load_immediate_shifted(high_temp, high));
                let operand = self.fresh_virtual_general();
                self.emit_global_load(name, operand)?;
                self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: high_temp, immediate: low });
                self.output.instructions.push(Instruction::MultiplyLow { d: destination, a: operand, b: GENERAL_SCRATCH });
                return Ok(true);
            }
        }

        // `(x >>(logical) n) & low-mask` fuses into one rlwinm: rotate-left by
        // (32 - n), then keep the masked low bits. mwcc emits this for the classic
        // `(value >> 16) & 0x7FFF` shape (e.g. the LCG in rand.c).
        if operator == BinaryOperator::BitAnd {
            if let Expression::Binary { operator: BinaryOperator::ShiftRight, left: inner, right: shift_amount } = variable {
                if let Expression::IntegerLiteral(amount) = shift_amount.as_ref() {
                    if (1..=31).contains(amount) && !self.signedness_of(inner)? {
                        if let Some((begin, 31)) = contiguous_mask(constant) {
                            let shift = (32 - *amount) as u8;
                            let source = self.place_operand_or_scratch(inner, destination)?;
                            self.output.instructions.push(Instruction::RotateAndMask {
                                a: destination,
                                s: source,
                                shift,
                                begin: begin.max(*amount as u8),
                                end: 31,
                            });
                            return Ok(true);
                        }
                    }
                }
            }
        }

        // `x | C` / `x ^ C` with a constant wider than 16 bits combines the high
        // and low halves: `oris`/`xoris` for the high half, then `ori`/`xori` for
        // the low. (A constant fitting 16 bits uses the single-immediate path below.)
        if matches!(operator, BinaryOperator::BitOr | BinaryOperator::BitXor)
            && !fits_unsigned_16(constant)
            && (0..=u32::MAX as i64).contains(&constant)
        {
            let value = constant as u32;
            let high = (value >> 16) as u16;
            let low = (value & 0xffff) as u16;
            let source = self.place_operand_or_scratch(variable, destination)?;
            let mut from = source;
            if high != 0 {
                self.output.instructions.push(match operator {
                    BinaryOperator::BitOr => Instruction::OrImmediateShifted { a: destination, s: from, immediate: high },
                    _ => Instruction::XorImmediateShifted { a: destination, s: from, immediate: high },
                });
                from = destination;
            }
            if low != 0 {
                self.output.instructions.push(match operator {
                    BinaryOperator::BitOr => Instruction::OrImmediate { a: destination, s: from, immediate: low },
                    _ => Instruction::XorImmediate { a: destination, s: from, immediate: low },
                });
            }
            return Ok(true);
        }

        enum Immediate {
            Add,
            ShiftLeft(u8),
            Multiply,
            Or,
            Xor,
            Mask(u8, u8),
        }
        let kind = match operator {
            BinaryOperator::Add if fits_signed_16(constant) => Immediate::Add,
            BinaryOperator::Multiply if fits_signed_16(constant) => {
                if constant >= 2 && (constant as u64).is_power_of_two() {
                    Immediate::ShiftLeft(constant.trailing_zeros() as u8)
                } else {
                    Immediate::Multiply
                }
            }
            BinaryOperator::BitOr if fits_unsigned_16(constant) => Immediate::Or,
            BinaryOperator::BitXor if fits_unsigned_16(constant) => Immediate::Xor,
            BinaryOperator::BitAnd if rlwinm_mask(constant).is_some() => {
                let (begin, end) = rlwinm_mask(constant).unwrap();
                Immediate::Mask(begin, end)
            }
            BinaryOperator::ShiftLeft if (1..=31).contains(&constant) => Immediate::ShiftLeft(constant as u8),
            _ => return Ok(false),
        };

        // A narrow value times a power of two (or `<< n`): an unsigned narrow
        // operand fuses extension and shift into one rlwinm; a signed one extends
        // (extsb/extsh) then shifts via the normal path below.
        if let &Immediate::ShiftLeft(shift) = &kind {
            if let Ok((register, width, leaf_signed)) = self.leaf_info(variable) {
                if width < 32 && !leaf_signed {
                    return Ok(self.emit_narrow_unsigned_shift(destination, register, width, true, shift));
                }
            }
        }
        // A narrow leaf masked entirely within its own bit-width needs no promotion
        // (extsb/extsh/clrlwi): the mask keeps only bits the extension would leave
        // unchanged, so mwcc masks the raw register — `char a & 0xf` is `clrlwi r3,r3,28`,
        // not `extsb r0,r3; clrlwi r3,r0,28`. The mask run must start within the narrow
        // value's low `width` bits (big-endian bit `32-width` onward); a mask reaching the
        // extension bits (`a & 0x1ff`) keeps the promotion via the normal path below.
        if let &Immediate::Mask(begin, end) = &kind {
            if let Ok((register, width, _signed)) = self.leaf_info(variable) {
                if width < 32 && (begin as u32) >= 32 - width as u32 {
                    self.output.instructions.push(Instruction::AndContiguousMask { a: destination, s: register, begin, end });
                    return Ok(true);
                }
            }
        }
        let prefer_destination = matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract);
        // `addi d, r0, imm` is `li d, imm` — it drops the source. So when an
        // add-immediate's own result lands in the scratch (it is a sub-expression),
        // its operand must still go to a non-scratch register. Place it in a fresh
        // virtual the allocator assigns, exactly as mwcc keeps such an operand in a
        // real register (g*BIG + 0x3039 -> the product in r3, then addi r0,r3,...).
        // (A call operand of any immediate op is kept in its r3 home centrally by
        // place_operand, so it needs no special case here.)
        let operand_target = if matches!(kind, Immediate::Add) && destination == GENERAL_SCRATCH {
            self.fresh_virtual_general()
        } else {
            destination
        };
        // A signed narrow (char/short) member reaching here under a Mask is a STRICT partial
        // mask (the wider masks already deferred at the top of this function), so the raw,
        // un-sign-extended byte is exactly what the `clrlwi` wants. place_operand defers a
        // signed narrow member operand by default (the promotion needs an extsb it cannot
        // emit byte-exactly yet); flag the truncation context so this masked read is exempt.
        let mask_reads_raw_member = matches!(kind, Immediate::Mask(..)) && self.is_signed_byte_load(variable)?;
        let saved_truncation_context = self.narrow_truncation_context;
        if mask_reads_raw_member {
            self.narrow_truncation_context = true;
        }
        let placed = self.place_operand(variable, operand_target, prefer_destination);
        self.narrow_truncation_context = saved_truncation_context;
        let Some(source) = placed? else {
            return Ok(false);
        };
        let d = destination;
        let instruction = match kind {
            Immediate::Add => Instruction::AddImmediate { d, a: source, immediate: constant as i16 },
            Immediate::ShiftLeft(shift) => Instruction::ShiftLeftImmediate { a: d, s: source, shift },
            Immediate::Multiply => Instruction::MultiplyImmediate { d, a: source, immediate: constant as i16 },
            Immediate::Or => Instruction::OrImmediate { a: d, s: source, immediate: constant as u16 },
            Immediate::Xor => Instruction::XorImmediate { a: d, s: source, immediate: constant as u16 },
            Immediate::Mask(begin, end) => Instruction::AndContiguousMask { a: d, s: source, begin, end },
        };
        self.output.instructions.push(instruction);
        Ok(true)
    }
}
