//! Constant folding, immediate forms, complement fusion, and shifts.

use crate::analysis::*;
use crate::generator::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Type, UnaryOperator};

impl Generator {
    /// If one operand is `~leaf` and the other is a leaf, emit `andc`/`orc`.
    pub(crate) fn try_emit_complement_logical(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> bool {
        // Both operands complemented — De Morgan folds to a single op: `~a & ~b` is
        // `nor(a,b)` and `~a | ~b` is `nand(a,b)`.
        if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::BitOr) {
            if let (Some(left_name), Some(right_name)) =
                (complemented_leaf_name(left), complemented_leaf_name(right))
            {
                if let (Some(left_register), Some(right_register)) = (
                    self.lookup_general(left_name),
                    self.lookup_general(right_name),
                ) {
                    self.output.instructions.push(match operator {
                        BinaryOperator::BitAnd => Instruction::Nor {
                            a: destination,
                            s: left_register,
                            b: right_register,
                        },
                        _ => Instruction::Nand {
                            a: destination,
                            s: left_register,
                            b: right_register,
                        },
                    });
                    return true;
                }
            }
        }
        let (kept_expression, complemented_name) = if let Some(name) = complemented_leaf_name(right)
        {
            (left, name)
        } else if let Some(name) = complemented_leaf_name(left) {
            (right, name)
        } else {
            return false;
        };
        let (Some(kept_name), Some(complemented_register)) = (
            leaf_name(kept_expression),
            self.lookup_general(complemented_name),
        ) else {
            return false;
        };
        let Some(kept_register) = self.lookup_general(kept_name) else {
            return false;
        };
        self.output.instructions.push(match operator {
            BinaryOperator::BitAnd => Instruction::AndComplement {
                a: destination,
                s: kept_register,
                b: complemented_register,
            },
            _ => Instruction::OrComplement {
                a: destination,
                s: kept_register,
                b: complemented_register,
            },
        });
        true
    }

    /// `a*b + a*c` / `a*b - a*c` — two products sharing a common factor distribute to `a*(b±c)`, as
    /// mwcc does: one `add`/`subf` of the non-factor operands into the scratch, then a single `mullw`.
    /// The factor keeps its side from the FIRST product (`a*b`→`mullw d,a,r0`; `b*a`→`mullw d,r0,a`);
    /// the sum is source order (`add r0,o1,o2`), the difference is `o1-o2` (`subf r0,o2,o1`). All three
    /// operands must be DISTINCT register leaves — a constant multiplier (`a*2+a*3`) folds elsewhere,
    /// and `a*a+a*b` (the factor reused as a non-factor operand) is left alone.
    pub(crate) fn try_emit_distributed_product(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
            || destination == GENERAL_SCRATCH
        {
            return Ok(false);
        }
        let (
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: x1,
                right: y1,
            },
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: x2,
                right: y2,
            },
        ) = (left, right)
        else {
            return Ok(false);
        };
        // The COMMUTED case (`a*b + b*a`) is ambiguous: both operands are shared but mwcc factors the
        // OTHER operand than our left-first search would, so leave it alone. EQUAL products (`a*b +
        // a*b`) and single-shared-operand cases (incl. `a*a + a*b` -> `a*(a+b)`) fold cleanly.
        if structurally_equal(x1, y2) && structurally_equal(y1, x2) {
            return Ok(false);
        }
        // Find the shared factor and the two remaining operands, noting the factor's side in the
        // FIRST product (false = left, true = right).
        let (factor, o1, o2, factor_on_right) = if structurally_equal(x1, x2) {
            (x1, y1, y2, false)
        } else if structurally_equal(x1, y2) {
            (x1, y1, x2, false)
        } else if structurally_equal(y1, x2) {
            (y1, x1, y2, true)
        } else if structurally_equal(y1, y2) {
            (y1, x1, x2, true)
        } else {
            return Ok(false);
        };
        // All three operands must be register leaves (a constant multiplier or a load is not
        // distributed here).
        let (Some(factor_register), Some(o1_register), Some(o2_register)) = (
            leaf_name(factor).and_then(|name| self.lookup_general(name)),
            leaf_name(o1).and_then(|name| self.lookup_general(name)),
            leaf_name(o2).and_then(|name| self.lookup_general(name)),
        ) else {
            return Ok(false);
        };
        // The sum / difference of the non-factor operands into the scratch.
        match operator {
            BinaryOperator::Subtract => {
                // `subf d,a,b` = b - a, so `subf r0, o2, o1` = o1 - o2.
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: GENERAL_SCRATCH,
                    a: o2_register,
                    b: o1_register,
                });
            }
            _ => self.output.instructions.push(Instruction::Add {
                d: GENERAL_SCRATCH,
                a: o1_register,
                b: o2_register,
            }),
        }
        // One multiply, the factor on its original side.
        let (multiply_a, multiply_b) = if factor_on_right {
            (GENERAL_SCRATCH, factor_register)
        } else {
            (factor_register, GENERAL_SCRATCH)
        };
        self.output.instructions.push(Instruction::MultiplyLow {
            d: destination,
            a: multiply_a,
            b: multiply_b,
        });
        Ok(true)
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
    pub(crate) fn try_emit_field_merge(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let (
            Some((insert_value, insert_kind, insert_begin, insert_end)),
            Some((base_value, base_kind, base_begin, base_end)),
        ) = (as_field(left), as_field(right))
        else {
            return Ok(false);
        };
        // The two fields must be disjoint. If they also tile the whole word (full
        // coverage) the base is moved raw and `rlwimi` overwrites the other field; with
        // PARTIAL coverage the bits outside both fields must be zeroed, so a MASK base is
        // masked to its field first (a shift base already zeros outside its field), then
        // `rlwimi` inserts the other — `(a&0xff00)|(b&0xff)` -> `clrlwi b; rlwimi a`.
        let insert_mask = run_mask(insert_begin, insert_end);
        let base_mask = run_mask(base_begin, base_end);
        if insert_mask & base_mask != 0 {
            return Ok(false);
        }
        let full_coverage = insert_mask | base_mask == 0xFFFF_FFFF;
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
        // Computing the base writes the destination, except a FULL-COVERAGE unshifted
        // mask whose value already sits there (a partial-coverage mask must be masked in
        // place, which writes it).
        let base_writes_destination = !(matches!(base_kind, FieldSource::Mask)
            && base_register == destination
            && full_coverage);
        // Preserve the inserted value when the base computation would overwrite it.
        let insert_source = if base_writes_destination && insert_register == destination {
            self.output
                .instructions
                .push(Instruction::move_register(GENERAL_SCRATCH, insert_register));
            GENERAL_SCRATCH
        } else {
            insert_register
        };
        // The base, computed directly into the destination.
        match base_kind {
            FieldSource::ShiftLeft(n) => {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: destination,
                        s: base_register,
                        shift: n,
                    })
            }
            FieldSource::ShiftRight(n) => {
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: destination,
                        s: base_register,
                        shift: n,
                    })
            }
            FieldSource::Mask => {
                if full_coverage {
                    if base_register != destination {
                        self.output
                            .instructions
                            .push(Instruction::move_register(destination, base_register));
                    }
                } else {
                    // Partial coverage: clear the bits outside the base's field (rlwimi
                    // will not touch them) — mwcc's `clrlwi`/`rlwinm` base.
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: destination,
                        s: base_register,
                        shift: 0,
                        begin: base_begin,
                        end: base_end,
                    });
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
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: destination,
                s: insert_source,
                shift: rotate,
                begin: insert_begin,
                end: insert_end,
            });
        Ok(true)
    }

    /// `(a << P) | (a >> Q)` for an UNSIGNED leaf `a` whose VARIABLE shift amounts are complementary
    /// (`P + Q == 32`) — a rotate LEFT by the left-shift amount P. mwcc emits a single `rotlw d,a,P`
    /// (`rlwnm d,a,P,0,31`); when P is `32 - m` (i.e. a rotate RIGHT by m) it first computes the
    /// amount with `subfic r0,m,32`. The right shift must be logical, so `a` must be unsigned — a
    /// signed `a >> Q` is arithmetic and not a rotate. (Constant-amount rotates go through the rlwimi
    /// field-merge path instead.)
    pub(crate) fn try_emit_variable_rotate(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        // Split into (rotated value, left-shift amount P, right-shift amount Q); the OR's operands may
        // be in either order.
        fn split<'e>(
            x: &'e Expression,
            y: &'e Expression,
        ) -> Option<(&'e Expression, &'e Expression, &'e Expression)> {
            if let (
                Expression::Binary {
                    operator: BinaryOperator::ShiftLeft,
                    left: value_left,
                    right: p,
                },
                Expression::Binary {
                    operator: BinaryOperator::ShiftRight,
                    left: value_right,
                    right: q,
                },
            ) = (x, y)
            {
                if structurally_equal(value_left, value_right) {
                    return Some((value_left, p, q));
                }
            }
            None
        }
        let Some((value, p, q)) = split(left, right).or_else(|| split(right, left)) else {
            return Ok(false);
        };
        // The amounts must be complementary: one is `32 - <the other>`.
        let is_32_minus = |whole: &Expression, part: &Expression| {
            matches!(whole, Expression::Binary { operator: BinaryOperator::Subtract, left: c, right: m }
                if constant_value(c) == Some(32) && structurally_equal(m, part))
        };
        if !(is_32_minus(q, p) || is_32_minus(p, q)) {
            return Ok(false);
        }
        // A SIGNED value's `>>` is arithmetic (`sraw`), so this rotate-shaped OR is NOT a true rotate:
        // mwcc emits the literal shift-or with a distinct schedule (`subfic` first, the amount register
        // reused) that we do not reproduce. Defer rather than emit our (differently scheduled) shift-or.
        if self.signedness_of(value)? {
            return Err(Diagnostic::error("a signed complementary variable shift-or is not a rotate; its literal-shift schedule is unmodeled (roadmap)"));
        }
        let Some(value_register) = leaf_name(value).and_then(|name| self.lookup_general(name))
        else {
            return Ok(false);
        };
        // The rotate-LEFT amount is the left-shift amount P: a register directly (rotate left by n),
        // or `32 - m` computed with `subfic r0,m,32` (rotate right by m).
        let amount_register = if let Some(register) =
            leaf_name(p).and_then(|name| self.lookup_general(name))
        {
            register
        } else if let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: c,
            right: m,
        } = p
        {
            if constant_value(c) != Some(32) {
                return Ok(false);
            }
            let Some(m_register) = leaf_name(m).and_then(|name| self.lookup_general(name)) else {
                return Ok(false);
            };
            self.output
                .instructions
                .push(Instruction::SubtractFromImmediate {
                    d: GENERAL_SCRATCH,
                    a: m_register,
                    immediate: 32,
                });
            GENERAL_SCRATCH
        } else {
            return Ok(false);
        };
        self.output
            .instructions
            .push(Instruction::RotateAndMaskVariable {
                a: destination,
                s: value_register,
                b: amount_register,
                begin: 0,
                end: 31,
            });
        Ok(true)
    }

    /// `(load_a & maskA) | (load_b & maskB)` with complementary masks where the
    /// operands are memory loads (e.g. the `__HI`/`__LO` pointer-pun merge in
    /// copysign). mwcc loads the inserted (left) operand first into a temporary,
    /// the base (right) operand into the destination, then merges with `rlwimi`.
    pub(crate) fn try_emit_field_merge_loads(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let (Some((insert_load, insert_mask)), Some((base_load, base_mask))) =
            (as_masked_load(left), as_masked_load(right))
        else {
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
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: destination,
                s: insert_register,
                shift: 0,
                begin,
                end,
            });
        Ok(true)
    }

    /// A shift fused with a mask collapses to one `rlwinm` (rotate-and-mask):
    ///   `(x << n) & m`, `(x >> n) & m`, `(x & m) << n`, `(x & m) >> n`.
    /// Each is `ROTL(x, r) & mask[begin,end]` for the right `r` and contiguous
    /// mask; the cases differ only in how `r` and the mask are derived. The masked
    /// region must avoid the bits the rotation wraps in (so the rotate equals the
    /// shift), and the logical-right forms require an unsigned value.
    pub(crate) fn try_emit_rotate_mask(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Some((value, rotate, begin, end, needs_unsigned)) =
            self.fused_rotate_mask(operator, left, right)?
        else {
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
        let register =
            if let Some(register) = leaf_name(value).and_then(|name| self.lookup_general(name)) {
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
        self.output.instructions.push(Instruction::RotateAndMask {
            a: destination,
            s: register,
            shift: rotate,
            begin,
            end,
        });
        Ok(true)
    }

    /// Like `as_constant_shift`, but the shifted value may be a full-word memory
    /// load (`p[0] >> n`, `s->a << n`) as well as a leaf — `try_emit_rotate_mask`
    /// places a load into the scratch before the `rlwinm`. Kept local to the
    /// fusion so `as_constant_shift`/`as_field` stay leaf-only.
    fn constant_shift_placeable<'e>(
        &self,
        expression: &'e Expression,
    ) -> Option<(&'e Expression, bool, u8)> {
        let Expression::Binary {
            operator,
            left,
            right,
        } = expression
        else {
            return None;
        };
        // The shifted value may be a leaf, a full-word load, or a computed
        // expression — `try_emit_rotate_mask` evaluates a non-leaf into the scratch
        // before the `rlwinm`, matching mwcc's `<compute> r0; rlwinm d,r0,…`.
        match operator {
            BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight => {
                let amount = constant_value(right)?;
                ((1..=31).contains(&amount)).then(|| {
                    (
                        left.as_ref(),
                        *operator == BinaryOperator::ShiftLeft,
                        amount as u8,
                    )
                })
            }
            // `x / 2^n` is a logical right shift by `n` only for an UNSIGNED value
            // (a signed division rounds toward zero, not a floor) — `unsigned-rand`'s
            // `… / 65536 & 0x7fff` is the canonical `rlwinm` form.
            BinaryOperator::Divide => {
                let divisor = constant_value(right)?;
                if divisor > 1
                    && (divisor as u64).is_power_of_two()
                    && self.signedness_of(left).ok() == Some(false)
                {
                    let shift = divisor.trailing_zeros();
                    return ((1..=31).contains(&shift))
                        .then(|| (left.as_ref(), false, shift as u8));
                }
                None
            }
            _ => None,
        }
    }

    /// Resolve a shift-and-mask expression to `(x, rotate, begin, end, needs_unsigned)`
    /// for the fused `rlwinm`, or `None` when the shape does not collapse cleanly.
    fn fused_rotate_mask<'e>(
        &self,
        operator: BinaryOperator,
        left: &'e Expression,
        right: &'e Expression,
    ) -> Compilation<Option<(&'e Expression, u8, u8, u8, bool)>> {
        let result = match operator {
            // `(x << n) & m` / `(x >> n) & m` — shift inside, mask outside.
            BinaryOperator::BitAnd => {
                let Some(mask) = constant_value(right) else {
                    return Ok(None);
                };
                let mask = mask as u32;
                let Some((value, is_left, shift)) = self.constant_shift_placeable(left) else {
                    return Ok(None);
                };
                if is_left {
                    // `x << n` zeroes the low n bits, so they cannot survive the mask.
                    let effective = mask & !((1u32 << shift) - 1);
                    let Some((begin, end)) = mask_to_run(effective) else {
                        return Ok(None);
                    };
                    Some((value, shift, begin, end, false))
                } else {
                    // `x >> n`: the mask must stay below the (possibly sign-extended)
                    // high n bits, so the rotate reproduces the shift for either sign.
                    if shift == 0 || mask >= (1u32 << (32 - shift)) {
                        return Ok(None);
                    }
                    let Some((begin, end)) = mask_to_run(mask) else {
                        return Ok(None);
                    };
                    Some((value, 32 - shift, begin, end, false))
                }
            }
            // `(x & m) << n` — mask inside, left shift outside; or a right shift
            // then a left shift `(x >> k) << n`.
            BinaryOperator::ShiftLeft => {
                let Some(shift) = constant_value(right) else {
                    return Ok(None);
                };
                if !(1..=31).contains(&shift) {
                    return Ok(None);
                }
                let shift = shift as u8;
                if let Some((value, mask)) = as_masked_leaf(left).or_else(|| as_masked_load(left)) {
                    let Some((begin, end)) = mask_to_run(mask << shift) else {
                        return Ok(None);
                    };
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
                let Some(shift) = constant_value(right) else {
                    return Ok(None);
                };
                if !(1..=31).contains(&shift) {
                    return Ok(None);
                }
                // `(x << k) >> n` (LOGICAL) collapses two shifts to one rotate-and-mask:
                //   rotate = (32 + k - n) mod 32, surviving field [n, 31] narrowed from the
                //   top by (k - n) when k >= n. `k==n` is the zero-extend `clrlwi x,n`.
                // `needs_unsigned=true` restricts it to an unsigned outer shift; the SIGNED
                // form sign-extends (`slwi;srawi` / `extsb`) and is left to the shift path.
                if let Some((value, true, k)) = self.constant_shift_placeable(left) {
                    let n = shift as u8;
                    let rotate = ((32 + k as i32 - n as i32) % 32) as u8;
                    let end = if k >= n { 31 - (k - n) } else { 31 };
                    return Ok(Some((value, rotate, n, end, true)));
                }
                // `x & m` for a leaf x, or `(p[0] & m)` / `(s->a & m)` for a load.
                let Some((value, mask)) = as_masked_leaf(left).or_else(|| as_masked_load(left))
                else {
                    return Ok(None);
                };
                let Some((begin, end)) = mask_to_run(mask >> shift) else {
                    return Ok(None);
                };
                Some((
                    value,
                    (32 - shift) as u8,
                    begin,
                    end,
                    mask & 0x8000_0000 != 0,
                ))
            }
            _ => None,
        };
        Ok(result)
    }

    /// Emit a right shift, choosing arithmetic (signed) or logical (unsigned)
    /// from the type of the shifted value.
    pub(crate) fn emit_shift_right(
        &mut self,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        let signed = self.signedness_of(left)?;
        // A narrow (char/short) value promotes to a SIGNED int before a `>>`, so a narrow
        // UNSIGNED LOAD — a `*p`/`p[i]`/`s->m` deref, or a file-scope global read (lbz/lhz) —
        // shifts with the arithmetic `srawi` (the value is non-negative, same result). A
        // register-resident narrow LOCAL fuses the extension and shift into one `rlwinm` below.
        let promoted_signed = signed
            || self.is_narrow_unsigned_load(left)?
            || matches!(left, Expression::Variable(name)
                if !self.locations.contains_key(name.as_str())
                    && matches!(self.globals.get(name.as_str()), Some(Type::UnsignedChar) | Some(Type::UnsignedShort)));
        let d = destination;

        if let Some(amount) = constant_value(right) {
            if (1..=31).contains(&amount) {
                // An unsigned narrow value fuses extension and shift into one
                // rlwinm; a signed narrow value extends (extsb/extsh) then shifts.
                if let Ok((register, width, leaf_signed)) = self.leaf_info(left) {
                    if width < 32 && !leaf_signed {
                        if self.emit_narrow_unsigned_shift(d, register, width, false, amount as u8)
                        {
                            return Ok(());
                        }
                        return Err(Diagnostic::error(
                            "narrow unsigned shift out of the single-rlwinm range (roadmap)",
                        ));
                    }
                }
                // The shifted value: a leaf stays put, a sub-expression goes to the
                // scratch (its temporaries are virtuals the allocator places).
                let source = match self.signed_byte_scratch_source(left, d)? {
                    Some(scratch) => scratch,
                    None => self.place_operand_or_scratch(left, d)?,
                };
                let shift = amount as u8;
                // A narrow unsigned LOAD promotes to a signed int before the shift, so mwcc emits
                // the arithmetic `srawi` (the value is non-negative, so the result is the same).
                self.output.instructions.push(if promoted_signed {
                    Instruction::ShiftRightAlgebraicImmediate {
                        a: d,
                        s: source,
                        shift,
                    }
                } else {
                    Instruction::ShiftRightLogicalImmediate {
                        a: d,
                        s: source,
                        shift,
                    }
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
                return Err(Diagnostic::error(
                    "shift amount needs the full register allocator (roadmap M1)",
                ));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(right)?
        };
        self.output.instructions.push(if promoted_signed {
            Instruction::ShiftRightAlgebraicWord {
                a: d,
                s: source,
                b: amount,
            }
        } else {
            Instruction::ShiftRightWord {
                a: d,
                s: source,
                b: amount,
            }
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
            if let Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } = left
            {
                if let (Some(register), Some(constant)) = (
                    leaf_name(operand).and_then(|name| self.lookup_general(name)),
                    constant_value(right),
                ) {
                    let immediate = if operator == BinaryOperator::Subtract {
                        constant.wrapping_neg()
                    } else {
                        constant
                    };
                    if fits_signed_16(immediate) {
                        self.output
                            .instructions
                            .push(Instruction::SubtractFromImmediate {
                                d: destination,
                                a: register,
                                immediate: immediate as i16,
                            });
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
            if let (Some(constant), Some(register)) = (
                constant_value(left),
                leaf_name(right).and_then(|name| self.lookup_general(name)),
            ) {
                if constant == 0 {
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: register,
                    });
                    return Ok(true);
                }
                if fits_signed_16(constant) {
                    self.output
                        .instructions
                        .push(Instruction::SubtractFromImmediate {
                            d: destination,
                            a: register,
                            immediate: constant as i16,
                        });
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
    pub(crate) fn emit_constant_form(
        &mut self,
        operator: BinaryOperator,
        variable: &Expression,
        constant: i64,
        destination: u8,
    ) -> Compilation<bool> {
        // A SIGNED CHAR load with a fitting constant for `|`, `^`, or `<<`: mwcc loads the byte into
        // the scratch and sign-extends it in place (`lbz r0; extsb r0,r0`), then the immediate op
        // reads r0 into the destination (`ori|xori|slwi r3,r0,c`). Unlike `addi`, these ops can
        // source r0, so the value stays in the scratch. Handled before the defer below (which gates
        // the operators — multiply, divide, wide ops — that need a different layout).
        if destination != GENERAL_SCRATCH && self.is_signed_byte_load(variable)? {
            let immediate = match operator {
                BinaryOperator::BitOr if fits_unsigned_16(constant) => {
                    Some(Instruction::OrImmediate {
                        a: destination,
                        s: GENERAL_SCRATCH,
                        immediate: constant as u16,
                    })
                }
                BinaryOperator::BitXor if fits_unsigned_16(constant) => {
                    Some(Instruction::XorImmediate {
                        a: destination,
                        s: GENERAL_SCRATCH,
                        immediate: constant as u16,
                    })
                }
                BinaryOperator::ShiftLeft if (1..=31).contains(&constant) => {
                    Some(Instruction::ShiftLeftImmediate {
                        a: destination,
                        s: GENERAL_SCRATCH,
                        shift: constant as u8,
                    })
                }
                _ => None,
            };
            if let Some(instruction) = immediate {
                self.signed_byte_scratch_source(variable, destination)?;
                self.output.instructions.push(instruction);
                return Ok(true);
            }
        }
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
            let is_fitting_mask =
                matches!(operator, BinaryOperator::BitAnd) && constant > 0 && constant < 0xff;
            // Add/Subtract read the sign-extended operand through place_operand (`lbz r0; extsb
            // d,r0` then the immediate), so they are byte-exact for a real-register destination. The
            // other operators (multiply/shift/or/xor) take a different operand path that does not
            // sign-extend yet, and the scratch (value/store) destination uses a different mwcc
            // layout — both still defer.
            let handled = is_fitting_mask
                || (destination != GENERAL_SCRATCH
                    && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract));
            if !handled {
                return Err(Diagnostic::error(
                    "a signed char load promoted to int needs a sign-extension (roadmap)",
                ));
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
                self.output.instructions.push(Instruction::Negate {
                    d: destination,
                    a: source,
                });
                return Ok(true);
            }
            // Bitwise identities: `x | 0`, `x ^ 0`, and `x & ~0` (all bits) are `x` — mwcc
            // emits no instruction (the value stays in its register), where ours would have
            // emitted a dead `ori`/`xori`/`rlwinm`. (`x & 0` is the constant 0; `x | ~0` is ~0
            // — those are different folds, not handled here.)
            (BinaryOperator::BitOr, 0)
            | (BinaryOperator::BitXor, 0)
            | (BinaryOperator::BitAnd, -1)
            | (BinaryOperator::BitAnd, 0xffff_ffff) => {
                self.evaluate_general(variable, destination)?;
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
            self.output
                .instructions
                .push(Instruction::AddImmediateShifted {
                    d: destination,
                    a: source,
                    immediate: high,
                });
            // A zero low half needs no `addi` (mwcc emits the bare `addis`).
            if low != 0 {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: destination,
                    immediate: low,
                });
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
                        return Ok(self.emit_narrow_unsigned_shift(
                            destination,
                            register,
                            width,
                            true,
                            shift,
                        ));
                    }
                }
                let source = self.place_operand_or_scratch(variable, destination)?;
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: destination,
                        s: source,
                        shift,
                    });
                return Ok(true);
            }
            let low = (constant as u32 & 0xffff) as i16;
            let high = ((constant as i32 - low as i32) >> 16) as i16;
            if let Ok(operand_register) = self.general_register_of_leaf(variable) {
                if low == 0 {
                    // No low half: build the constant straight in the scratch with a
                    // single `lis r0, high`, then multiply.
                    self.output
                        .instructions
                        .push(Instruction::load_immediate_shifted(GENERAL_SCRATCH, high));
                    self.output.instructions.push(Instruction::MultiplyLow {
                        d: destination,
                        a: operand_register,
                        b: GENERAL_SCRATCH,
                    });
                    return Ok(true);
                }
                // Leaf operand: it stays in its register; the constant is built in
                // the scratch via a free register.
                let free = self.free_general_excluding(operand_register)?;
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(free, high));
                self.output.instructions.push(Instruction::AddImmediate {
                    d: GENERAL_SCRATCH,
                    a: free,
                    immediate: low,
                });
                self.output.instructions.push(Instruction::MultiplyLow {
                    d: destination,
                    a: operand_register,
                    b: GENERAL_SCRATCH,
                });
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
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(high_temp, high));
                let operand = self.fresh_virtual_general();
                self.emit_global_load(name, operand)?;
                self.output.instructions.push(Instruction::AddImmediate {
                    d: GENERAL_SCRATCH,
                    a: high_temp,
                    immediate: low,
                });
                self.output.instructions.push(Instruction::MultiplyLow {
                    d: destination,
                    a: operand,
                    b: GENERAL_SCRATCH,
                });
                return Ok(true);
            }
        }

        // `(x >>(logical) n) & low-mask` fuses into one rlwinm: rotate-left by
        // (32 - n), then keep the masked low bits. mwcc emits this for the classic
        // `(value >> 16) & 0x7FFF` shape (e.g. the LCG in rand.c).
        if operator == BinaryOperator::BitAnd {
            if let Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                left: inner,
                right: shift_amount,
            } = variable
            {
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
                    BinaryOperator::BitOr => Instruction::OrImmediateShifted {
                        a: destination,
                        s: from,
                        immediate: high,
                    },
                    _ => Instruction::XorImmediateShifted {
                        a: destination,
                        s: from,
                        immediate: high,
                    },
                });
                from = destination;
            }
            if low != 0 {
                self.output.instructions.push(match operator {
                    BinaryOperator::BitOr => Instruction::OrImmediate {
                        a: destination,
                        s: from,
                        immediate: low,
                    },
                    _ => Instruction::XorImmediate {
                        a: destination,
                        s: from,
                        immediate: low,
                    },
                });
            }
            return Ok(true);
        }

        // `a * -C` for a power-of-two C is `-(a << log2 C)`: shift into the scratch then negate into
        // the destination (`slwi r0,a,n; neg d,r0`), as mwcc does — not a `mulli` by the negative.
        if operator == BinaryOperator::Multiply && constant <= -2 {
            let magnitude = constant.unsigned_abs();
            if magnitude.is_power_of_two() {
                let shift = magnitude.trailing_zeros();
                if (1..=31).contains(&shift) {
                    let source = self.place_operand_or_scratch(variable, destination)?;
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: GENERAL_SCRATCH,
                            s: source,
                            shift: shift as u8,
                        });
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: GENERAL_SCRATCH,
                    });
                    return Ok(true);
                }
            }
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
            BinaryOperator::ShiftLeft if (1..=31).contains(&constant) => {
                Immediate::ShiftLeft(constant as u8)
            }
            _ => return Ok(false),
        };

        // A narrow value times a power of two (or `<< n`): an unsigned narrow
        // operand fuses extension and shift into one rlwinm; a signed one extends
        // (extsb/extsh) then shifts via the normal path below.
        if let &Immediate::ShiftLeft(shift) = &kind {
            if let Ok((register, width, leaf_signed)) = self.leaf_info(variable) {
                if width < 32 && !leaf_signed {
                    return Ok(self.emit_narrow_unsigned_shift(
                        destination,
                        register,
                        width,
                        true,
                        shift,
                    ));
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
                    self.output
                        .instructions
                        .push(Instruction::AndContiguousMask {
                            a: destination,
                            s: register,
                            begin,
                            end,
                        });
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
        let mask_reads_raw_member =
            matches!(kind, Immediate::Mask(..)) && self.is_signed_byte_load(variable)?;
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
            Immediate::Add => Instruction::AddImmediate {
                d,
                a: source,
                immediate: constant as i16,
            },
            Immediate::ShiftLeft(shift) => Instruction::ShiftLeftImmediate {
                a: d,
                s: source,
                shift,
            },
            Immediate::Multiply => Instruction::MultiplyImmediate {
                d,
                a: source,
                immediate: constant as i16,
            },
            Immediate::Or => Instruction::OrImmediate {
                a: d,
                s: source,
                immediate: constant as u16,
            },
            Immediate::Xor => Instruction::XorImmediate {
                a: d,
                s: source,
                immediate: constant as u16,
            },
            Immediate::Mask(begin, end) => Instruction::AndContiguousMask {
                a: d,
                s: source,
                begin,
                end,
            },
        };
        self.output.instructions.push(instruction);
        Ok(true)
    }
}
