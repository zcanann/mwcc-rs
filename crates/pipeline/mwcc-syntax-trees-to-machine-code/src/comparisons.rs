//! Branchless comparison idioms.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use crate::analysis::*;
use crate::expressions::load_base_name;
use crate::generator::*;

impl Generator {

    /// Emit a comparison as mwcc's branchless idiom. Currently handles `==` (and
    /// `== 0`) and signed `< 0`; the richer signed less/greater idioms are not
    /// implemented yet.
    pub(crate) fn emit_comparison(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        // A comparison whose operands are floating-point materializes a boolean
        // from cr0 rather than using the integer branchless idioms below.
        if self.is_float_leaf(left) || self.is_float_leaf(right) {
            return self.emit_float_comparison(operator, left, right, destination);
        }
        // An INTEGER comparison of a value against itself (`x == x`, `a[0] < a[0]`)
        // is a compile-time constant — mwcc folds it to `li 0`/`li 1` without
        // evaluating the operand. (Floats are excluded above: `NaN == NaN` is
        // false, so that fold would be wrong.)
        if same_operand(left, right) {
            let value = i64::from(matches!(operator,
                BinaryOperator::Equal | BinaryOperator::LessEqual | BinaryOperator::GreaterEqual));
            self.load_integer_constant(destination, value);
            return Ok(());
        }
        let d = destination;
        let signed_left = self.signedness_of(left)?;
        match operator {
            BinaryOperator::Equal => {
                if is_zero_literal(right) || is_zero_literal(left) {
                    let value = if is_zero_literal(right) { left } else { right };
                    // `(x & (1<<k)) == 0`: extract bit k to the low bit (one rlwinm),
                    // then flip it.
                    if let Some((variable, mask)) = as_masked_leaf(value) {
                        if mask.is_power_of_two() {
                            let register = self.general_register_of_leaf(variable)?;
                            let shift = ((32 - mask.trailing_zeros()) % 32) as u8;
                            self.output.instructions.push(Instruction::RotateAndMask { a: GENERAL_SCRATCH, s: register, shift, begin: 31, end: 31 });
                            self.output.instructions.push(Instruction::XorImmediate { a: d, s: GENERAL_SCRATCH, immediate: 1 });
                            return Ok(());
                        }
                    }
                    let source = self.place_operand_or_scratch(value, d)?;
                    // A signed byte load is `lbz` (zero-extended); mwcc re-extends it
                    // with `extsb` before the leading-zero test. Signed halfword loads
                    // use `lha` (already sign-extended) and unsigned loads need nothing.
                    if self.is_signed_byte_load(value)? {
                        self.emit_widen(source, source, 8, true);
                    }
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                } else if let Some(constant) = as_small_integer(right) {
                    // a == c : (c - a) leading zeros. A narrow operand is extended
                    // into the scratch first (extsb/clrlwi); a full-word load is
                    // evaluated into the scratch; a wide leaf stays in its register.
                    let value = if self.is_word_load(left) {
                        self.evaluate_general(left, GENERAL_SCRATCH)?;
                        GENERAL_SCRATCH
                    } else {
                        match self.leaf_info(left) {
                            Ok((register, width, signed)) if width < 32 => {
                                self.emit_widen(GENERAL_SCRATCH, register, width, signed);
                                GENERAL_SCRATCH
                            }
                            _ => self.general_register_of_leaf(left)?,
                        }
                    };
                    self.output.instructions.push(Instruction::SubtractFromImmediate { d: GENERAL_SCRATCH, a: value, immediate: constant });
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                } else {
                    // a == b : leading zeros of (a - b). Narrow operands are
                    // extended first — the left in place, the right into the
                    // scratch (mwcc's placement for the equality idiom).
                    let (left_register, right_register) = self.place_compare_leaves(left, right)?;
                    self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: left_register, b: right_register });
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                }
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
                Ok(())
            }
            // x != 0 : sign bit of (-x | x)
            BinaryOperator::NotEqual if is_zero_literal(right) => {
                // `(x & (1<<k)) != 0`: extract bit k to the low bit with one rlwinm.
                if let Some((variable, mask)) = as_masked_leaf(left) {
                    if mask.is_power_of_two() {
                        let register = self.general_register_of_leaf(variable)?;
                        let shift = ((32 - mask.trailing_zeros()) % 32) as u8;
                        self.output.instructions.push(Instruction::RotateAndMask { a: d, s: register, shift, begin: 31, end: 31 });
                        return Ok(());
                    }
                }
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // `x != C` (nonzero constant): sign bit of ((C - x) | (x - C)), both
            // halves built with immediates (`subfic` and `addi`).
            BinaryOperator::NotEqual
                if leaf_name(left).is_some() && !self.is_narrow_leaf(left)
                    && constant_value(right).is_some_and(|constant| constant != 0 && i16::try_from(constant).is_ok() && i16::try_from(-constant).is_ok()) =>
            {
                let constant = constant_value(right).unwrap() as i16;
                let x = self.general_register_of_leaf(left)?;
                let Some(temp) = (3u8..=12).find(|register| *register != x && !self.reserved.contains(register)) else {
                    return Err(Diagnostic::error("out of registers for the != idiom"));
                };
                self.output.instructions.push(Instruction::SubtractFromImmediate { d: temp, a: x, immediate: constant });
                self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: x, immediate: -constant });
                self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: temp, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed x < 0 : the sign bit.
            BinaryOperator::Less if is_zero_literal(right) && signed_left => {
                let source = self.place_operand_or_scratch(left, d)?;
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: source, shift: 31 });
                Ok(())
            }
            // signed x > 0 : sign bit of (-x & ~x)
            BinaryOperator::Greater if is_zero_literal(right) && signed_left => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed x >= 0 : !(x < 0)
            BinaryOperator::GreaterEqual if is_zero_literal(right) && signed_left => {
                let source = self.place_operand_or_scratch(left, d)?;
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: source, shift: 31 });
                self.output.instructions.push(Instruction::XorImmediate { a: d, s: GENERAL_SCRATCH, immediate: 1 });
                Ok(())
            }
            // signed x <= 0 : `cntlzw(x)` is 0 (x<0) or 32 (x==0) but 1..31 (x>0), so
            // rotating a `1` left by that count lands in the low bit only for x <= 0.
            // When `x` already occupies the destination (a leaf), the `cntlzw` must
            // read it before `li d,1` overwrites it; otherwise mwcc schedules `li d,1`
            // first, ahead of the `cntlzw` of the scratch-resident operand.
            BinaryOperator::LessEqual if is_zero_literal(right) && signed_left => {
                let source = self.place_operand_or_scratch(left, d)?;
                if source == d {
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                    self.load_integer_constant(d, 1);
                } else {
                    self.load_integer_constant(d, 1);
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                }
                self.output.instructions.push(Instruction::RotateAndMaskVariable { a: d, s: d, b: GENERAL_SCRATCH, begin: 31, end: 31 });
                Ok(())
            }
            // general signed branchless comparisons. Both leaves (any operator), or
            // a one-non-leaf shape whose idiom uses that operand twice, so mwcc keeps
            // it in a register: the `>` idiom keeps its LEFT operand, the `<` idiom
            // keeps its RIGHT. Computing that operand into a virtual that *avoids the
            // destination* leaves the destination free for the idiom's result-path
            // temporary — reproducing mwcc (p->a > x, (a+b) > c; x < p->a, x < (a+b)).
            // The other idioms use an operand once (mwcc keeps it in the scratch);
            // those non-leaf shapes still defer rather than mismatch.
            // signed `x > C` : materialize C in r0 (the `>` idiom uses the right
            // operand once), then the same sign-bit idiom with a fresh temp. `x` is
            // a leaf or a full-word load (e.g. `*p > C`, `s->a > C`).
            BinaryOperator::Greater
                if signed_left && !self.is_narrow_leaf(left)
                    && (leaf_name(left).is_some() || self.is_simple_word_load(left))
                    && constant_value(right).is_some_and(|constant| i16::try_from(constant).is_ok()) =>
            {
                let x = if leaf_name(left).is_some() {
                    self.general_register_of_leaf(left)?
                } else {
                    // The load avoids the destination so the later temp (the
                    // intermediate `(x^C)>>1`) can coalesce onto it, as mwcc does.
                    let register = self.fresh_virtual_general_avoiding(vec![d]);
                    self.evaluate_general(left, register)?;
                    register
                };
                let constant = constant_value(right).unwrap();
                self.load_integer_constant(GENERAL_SCRATCH, constant);
                let scratch = GENERAL_SCRATCH;
                let temp = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::Xor { a: scratch, s: x, b: scratch });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: temp, s: scratch, shift: 1 });
                self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: x });
                self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: temp });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                Ok(())
            }
            // signed `(load) < C` : the load is the low operand (read once) → r0;
            // the constant is the high operand (read twice) → a fresh register.
            BinaryOperator::Less
                if signed_left && self.is_simple_word_load(left) && !self.is_narrow_leaf(right)
                    && constant_value(right).is_some_and(|constant| i16::try_from(constant).is_ok()) =>
            {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                let load = GENERAL_SCRATCH;
                let constant_register = self.fresh_virtual_general();
                self.load_integer_constant(constant_register, constant_value(right).unwrap());
                let scratch = GENERAL_SCRATCH;
                self.output.instructions.push(Instruction::Xor { a: scratch, s: constant_register, b: load });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: scratch, shift: 1 });
                self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: constant_register });
                self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                Ok(())
            }
            // `a[i] != a[j]` / `*p != *q` (two full-word loads): the left operand
            // loads into a fresh virtual, the right into the scratch, in source
            // order, then the leaf != idiom — sign bit of ((b-a)|(a-b)). Equality
            // is sign-agnostic so this also covers unsigned loads. The left value
            // is live across both subtracts, and mwcc keeps it off BOTH base
            // registers (a same base stays live for the second load; a different
            // base is dead but mwcc still does not reuse it) — so the left virtual
            // avoids both bases: it colors r4 for a shared base, r5 for distinct
            // bases, matching `lwz r4/r5; lwz r0; subf …`.
            BinaryOperator::NotEqual
                if self.is_simple_word_load(left) && self.is_simple_word_load(right)
                    && load_base_name(left).is_some()
                    && load_base_name(right).is_some() =>
            {
                let avoid: Vec<u8> = [&*left, &*right].iter()
                    .filter_map(|operand| load_base_name(operand).and_then(|name| self.lookup_general(name)))
                    .collect();
                let left_register = self.fresh_virtual_general_avoiding(avoid);
                self.evaluate_general(left, left_register)?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                let right_register = GENERAL_SCRATCH;
                let scratch = GENERAL_SCRATCH;
                let temp = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::SubtractFrom { d: temp, a: left_register, b: right_register });
                self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: right_register, b: left_register });
                self.output.instructions.push(Instruction::Or { a: scratch, s: temp, b: scratch });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                Ok(())
            }
            // `a[i] < a[j]` / `a[i] > a[j]` (two same-base full-word loads): the
            // operand the signed idiom reads twice goes to a fresh virtual (the
            // allocator colors it r4, the base reg being live), the once-read
            // operand to the scratch; loads in source order. `<` reads its RIGHT
            // operand twice (`xor`+`and`), `>` reads its LEFT twice. Different
            // bases defer (the same allocator-coloring gap as the != case).
            //
            // The `>` form loads its twice-read operand FIRST, while the shared
            // base is still needed for the second load, so that operand is forced
            // off the (soon-dead) base register exactly as mwcc places it — it
            // matches in every destination context. The `<` form loads its
            // twice-read operand SECOND, after the base dies; there it relies on
            // the fixed `srawi → d` write to keep that operand off the destination
            // register, which only reproduces mwcc when `d` is a real register
            // (return/arg). In a value/store context (`d` == scratch) that write
            // would collide with the scratch and mwcc declines to reuse the dead
            // base anyway, so `<` defers there.
            BinaryOperator::Less | BinaryOperator::Greater
                if signed_left && self.is_simple_word_load(left) && self.is_simple_word_load(right)
                    && load_base_name(left).is_some()
                    && load_base_name(right).is_some()
                    && (matches!(operator, BinaryOperator::Greater) || d != GENERAL_SCRATCH) =>
            {
                let scratch = GENERAL_SCRATCH;
                let left_base = load_base_name(left).and_then(|name| self.lookup_general(name));
                let right_base = load_base_name(right).and_then(|name| self.lookup_general(name));
                if matches!(operator, BinaryOperator::Less) {
                    // a < b : sign bit of (((a^b)>>1) - ((a^b)&b)). b is read twice
                    // and loaded second, so it reuses its own (now-dead) base — it
                    // only avoids the LEFT operand's base (r4 same-base, or the
                    // right base it coalesces onto for distinct bases).
                    self.evaluate_general(left, scratch)?;
                    let right_register = self.fresh_virtual_general_avoiding(left_base.into_iter().collect());
                    self.evaluate_general(right, right_register)?;
                    self.output.instructions.push(Instruction::Xor { a: scratch, s: right_register, b: scratch });
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: scratch, shift: 1 });
                    self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: right_register });
                    self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: d });
                } else {
                    // a > b : sign bit of (((a^b)>>1) - ((a^b)&a)). a is read twice
                    // and loaded first, so it must stay off BOTH bases (its own is
                    // live during the load, the other until the second load) — r4
                    // same-base, r5 distinct.
                    let left_register = self.fresh_virtual_general_avoiding([left_base, right_base].into_iter().flatten().collect());
                    self.evaluate_general(left, left_register)?;
                    self.evaluate_general(right, scratch)?;
                    let temp = self.fresh_virtual_general();
                    self.output.instructions.push(Instruction::Xor { a: scratch, s: left_register, b: scratch });
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: temp, s: scratch, shift: 1 });
                    self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: left_register });
                    self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: temp });
                }
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                Ok(())
            }
            BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::NotEqual
                if signed_left && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right)
                    && (
                        (leaf_name(left).is_some() && leaf_name(right).is_some())
                        || (matches!(operator, BinaryOperator::Greater)
                            && leaf_name(left).is_none() && leaf_name(right).is_some())
                        || (matches!(operator, BinaryOperator::Less)
                            && leaf_name(right).is_none() && leaf_name(left).is_some())
                        || (matches!(operator, BinaryOperator::NotEqual)
                            && leaf_name(left).is_none() != leaf_name(right).is_none())
                    ) =>
            {
                let (left_register, right_register) = self.place_compare_operands(operator, left, right, d)?;
                let scratch = GENERAL_SCRATCH;
                match operator {
                    // a < b : sign bit of (((a^b)>>1) - ((a^b)&b))
                    BinaryOperator::Less => {
                        self.output.instructions.push(Instruction::Xor { a: scratch, s: right_register, b: left_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: scratch, shift: 1 });
                        self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: right_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: d });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                    // a > b : sign bit of (((a^b)>>1) - ((a^b)&a)). The intermediate
                    // `(a^b)>>1` goes to a fresh virtual the allocator places at the
                    // lowest free register — for leaves that coalesces onto rB (free
                    // after the xor), reproducing mwcc, and it stays correct when an
                    // operand is a load and rB is not free.
                    BinaryOperator::Greater => {
                        let temp = self.fresh_virtual_general();
                        self.output.instructions.push(Instruction::Xor { a: scratch, s: left_register, b: right_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: temp, s: scratch, shift: 1 });
                        self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: left_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: temp });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                    // a != b : sign bit of ((b - a) | (a - b)), with a second temp.
                    _ => {
                        let temp = self.fresh_virtual_general();
                        self.output.instructions.push(Instruction::SubtractFrom { d: temp, a: left_register, b: right_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: right_register, b: left_register });
                        self.output.instructions.push(Instruction::Or { a: scratch, s: temp, b: scratch });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                }
                Ok(())
            }
            // unsigned a < b / a > b : xor/cntlzw/slw/srwi. A constant operand `x > C`
            // is the low side (read once) → r0; `x < C` is the high side (read twice)
            // → a fresh register the allocator places at the lowest free GPR.
            BinaryOperator::Less | BinaryOperator::Greater
                if !signed_left && leaf_name(left).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right)
                    && (leaf_name(right).is_some()
                        || constant_value(right).is_some_and(|constant| i16::try_from(constant).is_ok())) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = match constant_value(right) {
                    Some(constant) if matches!(operator, BinaryOperator::Less) => {
                        let register = self.fresh_virtual_general();
                        self.load_integer_constant(register, constant);
                        register
                    }
                    _ => self.compare_right_operand(right)?,
                };
                // a < b uses b as the high side; a > b is b < a.
                let high = if matches!(operator, BinaryOperator::Less) { right_register } else { left_register };
                let low = if matches!(operator, BinaryOperator::Less) { left_register } else { right_register };
                self.output.instructions.push(Instruction::Xor { a: GENERAL_SCRATCH, s: high, b: low });
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftLeftWord { a: GENERAL_SCRATCH, s: high, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // unsigned a <= b / a >= b : orc-based, dest + scratch.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if !signed_left && leaf_name(left).is_some() && leaf_name(right).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                // a<=b uses (low,high)=(a,b); a>=b is b<=a.
                let (low, high) = match operator {
                    BinaryOperator::LessEqual => (left_register, right_register),
                    _ => (right_register, left_register),
                };
                self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: low, b: high });
                self.output.instructions.push(Instruction::OrComplement { a: d, s: high, b: low });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 1 });
                self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // `a[i] <= a[j]` / `s->x >= s->y` (two same-base full-word loads): the
            // carry idiom over loaded operands. The operands load high-first — one
            // into the scratch, the other into a free register; sign(high) goes to
            // another free register and sign(low) to the destination:
            // `lwz r0; lwz r5; srawi r4,high,31; srwi d,low,31; subfc r0,low,high;
            // adde d,r4,d`. Different bases / value context defer.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if signed_left && self.is_simple_word_load(left) && self.is_simple_word_load(right)
                    && load_base_name(left).is_some()
                    && load_base_name(left) == load_base_name(right)
                    && d != GENERAL_SCRATCH =>
            {
                let base = load_base_name(left).and_then(|name| self.lookup_general(name));
                let mut free = (3u8..=12).filter(|r| *r != GENERAL_SCRATCH && *r != d && Some(*r) != base && !self.reserved.contains(r));
                let (Some(sign_high_reg), Some(operand_reg)) = (free.next(), free.next()) else {
                    return Err(Diagnostic::error("out of registers for the two-load <=/>= idiom"));
                };
                let scratch = GENERAL_SCRATCH;
                let (high, low) = if matches!(operator, BinaryOperator::LessEqual) { (right, left) } else { (left, right) };
                // The high operand loads first; for `<=` it lands in the scratch,
                // for `>=` in the free register (and the low operand vice versa).
                let (high_reg, low_reg) = if matches!(operator, BinaryOperator::LessEqual) {
                    self.evaluate_general(high, scratch)?;
                    self.evaluate_general(low, operand_reg)?;
                    (scratch, operand_reg)
                } else {
                    self.evaluate_general(high, operand_reg)?;
                    self.evaluate_general(low, scratch)?;
                    (operand_reg, scratch)
                };
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: sign_high_reg, s: high_reg, shift: 31 });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: low_reg, shift: 31 });
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: scratch, a: low_reg, b: high_reg });
                self.output.instructions.push(Instruction::AddExtended { d, a: sign_high_reg, b: d });
                Ok(())
            }
            // signed a <= b / a >= b : carry-based, with two temporaries. A
            // constant right operand materializes into r0 (read twice before being
            // overwritten by the subfc).
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if signed_left && leaf_name(left).is_some()
                    && !self.is_narrow_leaf(left) && !self.is_narrow_leaf(right)
                    && (leaf_name(right).is_some() || constant_value(right).is_some_and(|constant| i16::try_from(constant).is_ok())) =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.compare_right_operand(right)?;
                let mut free = (3u8..=12).filter(|r| ![left_register, right_register, GENERAL_SCRATCH].contains(r));
                let (Some(lower), Some(higher)) = (free.next(), free.next()) else {
                    return Err(Diagnostic::error("out of registers for comparison"));
                };
                // For a<=b: high = sign(b), low = sign(a), carry from (b - a).
                // For a>=b the operands swap.
                let (sign_high, sign_low, subtrahend, minuend) = match operator {
                    BinaryOperator::LessEqual => (right_register, left_register, left_register, right_register),
                    _ => (left_register, right_register, right_register, left_register),
                };
                let sign_of_high = Instruction::ShiftRightAlgebraicImmediate { a: higher, s: sign_high, shift: 31 };
                let sign_of_low = Instruction::ShiftRightLogicalImmediate { a: lower, s: sign_low, shift: 31 };
                // With a materialized constant, mwcc shifts the ready variable
                // operand (the left leaf) before the constant's.
                if constant_value(right).is_some() && sign_low == left_register {
                    self.output.instructions.push(sign_of_low);
                    self.output.instructions.push(sign_of_high);
                } else {
                    self.output.instructions.push(sign_of_high);
                    self.output.instructions.push(sign_of_low);
                }
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: GENERAL_SCRATCH, a: subtrahend, b: minuend });
                self.output.instructions.push(Instruction::AddExtended { d, a: higher, b: lower });
                Ok(())
            }
            _ => Err(Diagnostic::error("this comparison needs the branchless compare idioms (roadmap)")),
        }
    }

    /// A comparison whose operands are floating-point. mwcc compares into cr0
    /// (`fcmpu` for `==`/`!=`, `fcmpo` for the ordered relations), then moves cr0
    /// into a GPR with `mfcr` and rotates the relevant bit (lt=0, gt=1, eq=2) down
    /// to the low bit. `<=`/`>=` first fold equality into the eq bit with `cror`;
    /// `!=` extracts eq and flips it with `xori`.
    pub(crate) fn emit_float_comparison(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        const LT: u8 = 0;
        const GT: u8 = 1;
        const EQ: u8 = 2;
        // The comparison's precision comes from the typed (non-literal) operand; a
        // float literal (e.g. `x > 0.0`) is loaded from the constant pool.
        let double = self.is_double_value(left) || self.is_double_value(right);
        let a = self.place_float_compare_operand(left, double)?;
        let b = self.place_float_compare_operand(right, double)?;
        let scratch = GENERAL_SCRATCH;
        if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            // `==`/`!=` are commutative; mwcc canonicalizes a literal operand to
            // the front (it loaded the constant first), so `x == 0.0` is `fcmpu 0,x`.
            let (first, second) = if matches!(right, Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)) { (b, a) } else { (a, b) };
            self.output.instructions.push(Instruction::FloatCompareUnordered { a: first, b: second });
        } else {
            self.output.instructions.push(Instruction::FloatCompareOrdered { a, b });
        }
        // `<=`/`>=` fold equality into the eq bit so one extract covers both relations.
        match operator {
            BinaryOperator::LessEqual => self.output.instructions.push(Instruction::ConditionRegisterOr { d: EQ, a: LT, b: EQ }),
            BinaryOperator::GreaterEqual => self.output.instructions.push(Instruction::ConditionRegisterOr { d: EQ, a: GT, b: EQ }),
            _ => {}
        }
        self.output.instructions.push(Instruction::MoveFromConditionRegister { d: scratch });
        let bit = match operator {
            BinaryOperator::Less => LT,
            BinaryOperator::Greater => GT,
            BinaryOperator::Equal | BinaryOperator::NotEqual | BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => EQ,
            _ => return Err(Diagnostic::error("unsupported floating-point comparison")),
        };
        // Rotate the bit (at position `bit` from the MSB) into bit 31 and mask it.
        let shift = bit + 1;
        if matches!(operator, BinaryOperator::NotEqual) {
            self.output.instructions.push(Instruction::RotateAndMask { a: scratch, s: scratch, shift, begin: 31, end: 31 });
            self.output.instructions.push(Instruction::XorImmediate { a: destination, s: scratch, immediate: 1 });
        } else {
            self.output.instructions.push(Instruction::RotateAndMask { a: destination, s: scratch, shift, begin: 31, end: 31 });
        }
        Ok(())
    }

    /// A floating-point comparison used as a *condition* (in an `if`): emit the
    /// `fcmpo`/`fcmpu` (and the `cror` that folds equality into the eq bit for
    /// `<=`/`>=`) and return the branch `(options, bit)` that skips the guarded body
    /// when the relation is false — the same bit mapping the integer compare uses.
    pub(crate) fn emit_float_condition(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<(u8, u8)> {
        const LT: u8 = 0;
        const GT: u8 = 1;
        const EQ: u8 = 2;
        const FLOAT_FIRST: u8 = 1; // f1
        let double = self.is_double_value(left) || self.is_double_value(right);
        let eq = matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual);
        let left_literal = matches!(left, Expression::FloatLiteral(_) | Expression::IntegerLiteral(_));
        let right_literal = matches!(right, Expression::FloatLiteral(_) | Expression::IntegerLiteral(_));
        let left_load = self.is_float_operand(left) && !self.is_float_leaf(left) && !left_literal;
        let right_load = self.is_float_operand(right) && !self.is_float_leaf(right) && !right_literal;
        let (a, b) = if eq && (left_load || right_load) {
            // `==`/`!=` against a loaded value (member/global) uses a *swapped* register
            // assignment versus the ordered form: the constant in f1 (loaded first), the
            // value in f0 — `lfs f1,k; lfs f0,(v); fcmpu f1,f0`. The == canonicalization
            // below (constant first) then emits the right operand order. f1 must be free
            // of a float argument; member==member and the like aren't modeled yet.
            if self.f1_holds_float_argument() {
                return Err(Diagnostic::error("a float == comparison with a float argument in f1 needs the FP register allocator (roadmap)"));
            }
            if right_literal && left_load {
                self.load_float_literal_into(FLOAT_FIRST, right, double)?;
                self.evaluate_float(left, FLOAT_SCRATCH)?;
                (FLOAT_SCRATCH, FLOAT_FIRST)
            } else if left_literal && right_load {
                self.load_float_literal_into(FLOAT_FIRST, left, double)?;
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                (FLOAT_FIRST, FLOAT_SCRATCH)
            } else {
                return Err(Diagnostic::error("this floating-point == comparison needs the value register allocator (roadmap)"));
            }
        } else if right_literal && !left_literal {
            // One operand is a pool literal and the other a value that must be loaded (a
            // float member or global): mwcc loads the constant into f0 first, then the
            // value into f1 — `lfs f0,k; lfs f1,(v); fcmpo f1,f0`. Place the literal side
            // first so the loads emit in that order; the fcmpo keeps source order.
            let b = self.place_float_compare_operand(right, double)?;
            let a = self.place_float_compare_value(left)?;
            (a, b)
        } else if left_literal && !right_literal {
            let a = self.place_float_compare_operand(left, double)?;
            let b = self.place_float_compare_value(right)?;
            (a, b)
        } else {
            let a = self.place_float_compare_operand(left, double)?;
            let b = self.place_float_compare_operand(right, double)?;
            (a, b)
        };
        if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            let (first, second) = if matches!(right, Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)) { (b, a) } else { (a, b) };
            self.output.instructions.push(Instruction::FloatCompareUnordered { a: first, b: second });
        } else {
            self.output.instructions.push(Instruction::FloatCompareOrdered { a, b });
        }
        match operator {
            BinaryOperator::LessEqual => self.output.instructions.push(Instruction::ConditionRegisterOr { d: EQ, a: LT, b: EQ }),
            BinaryOperator::GreaterEqual => self.output.instructions.push(Instruction::ConditionRegisterOr { d: EQ, a: GT, b: EQ }),
            _ => {}
        }
        Ok(match operator {
            BinaryOperator::Less => (4, LT),
            BinaryOperator::Greater => (4, GT),
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual | BinaryOperator::Equal => (4, EQ),
            BinaryOperator::NotEqual => (12, EQ),
            _ => return Err(Diagnostic::error("unsupported floating-point condition")),
        })
    }

    /// Load a float or (promoted) integer literal into `dest` at the comparison's
    /// precision — `lfs`/`lfd` from the pool, the same promotion mwcc applies to a
    /// written `a > 0`.
    fn load_float_literal_into(&mut self, dest: u8, operand: &Expression, double: bool) -> Compilation<()> {
        match operand {
            Expression::FloatLiteral(value) => {
                if double {
                    self.load_double_constant(dest, value.to_bits());
                } else {
                    self.load_float_constant(dest, *value as f32);
                }
                Ok(())
            }
            Expression::IntegerLiteral(value) => {
                if double {
                    self.load_double_constant(dest, (*value as f64).to_bits());
                } else {
                    self.load_float_constant(dest, *value as f32);
                }
                Ok(())
            }
            _ => Err(Diagnostic::error("expected a float literal operand")),
        }
    }

    /// Whether f1 currently holds a float argument (a float parameter lives there),
    /// so it can't double as the compare scratch without the FP register allocator.
    fn f1_holds_float_argument(&self) -> bool {
        self.locations.values().any(|location| location.class == ValueClass::Float && location.register == 1)
    }

    /// Place a floating-point comparison operand: a leaf stays in its register; a
    /// float literal is loaded from the constant pool (`lfs`/`lfd`) into the float
    /// scratch, matching mwcc's `x > 0.0` form.
    fn place_float_compare_operand(&mut self, operand: &Expression, double: bool) -> Compilation<u8> {
        if matches!(operand, Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)) {
            self.load_float_literal_into(FLOAT_SCRATCH, operand, double)?;
            return Ok(FLOAT_SCRATCH);
        }
        self.float_register_of_leaf(operand)
    }

    /// Place a float comparison operand that is a *value* (not a pool literal): a
    /// register leaf stays put; a float member/global/dereference is loaded into `f1`
    /// via evaluate_float (`lfs f1,(addr)`), the register mwcc uses for the compared
    /// value. Deferred when `f1` already holds a float argument — mwcc would pick a
    /// higher FPR there, which needs the register allocator.
    fn place_float_compare_value(&mut self, operand: &Expression) -> Compilation<u8> {
        const FLOAT_FIRST: u8 = 1;
        if self.is_float_leaf(operand) {
            return self.float_register_of_leaf(operand);
        }
        if self.f1_holds_float_argument() {
            return Err(Diagnostic::error("a float member/global compare with a float argument in f1 needs the FP register allocator (roadmap)"));
        }
        self.evaluate_float(operand, FLOAT_FIRST)?;
        Ok(FLOAT_FIRST)
    }

    /// Whether `value` is a full-word (32-bit) memory load — a dereference,
    /// index, or struct member — which can be evaluated into a register and used
    /// as a comparison operand without narrow extension.
    pub(crate) fn is_word_load(&self, value: &Expression) -> bool {
        match value {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer) == Some(32),
            Expression::Index { base, .. } => self.dereferenced_width(base) == Some(32),
            Expression::Member { member_type, .. } => member_type.width() == 32,
            _ => false,
        }
    }

    /// A full-word load that is a SINGLE machine instruction — a dereference,
    /// struct member, or constant-index subscript (`lwz off(base)`). A
    /// variable-index subscript is excluded: it scales to `slwi; lwzx`, and the
    /// branchless comparison idioms then mis-schedule the constant/second-load
    /// against that two-instruction sequence (mwcc fills the `lwzx` latency gap
    /// with the independent `li`/load, which the scheduler does not reproduce),
    /// so those defer.
    pub(crate) fn is_simple_word_load(&self, value: &Expression) -> bool {
        self.is_word_load(value)
            && !matches!(value, Expression::Index { index, .. } if constant_value(index).is_none())
    }

    /// Whether `value` is a load of a signed 8-bit value (a `char`/`signed char`
    /// dereference, index, or struct member) — emitted as `lbz`, which zero-extends
    /// and so needs a following `extsb` in the sign-sensitive idioms.
    pub(crate) fn is_signed_byte_load(&self, value: &Expression) -> Compilation<bool> {
        let width = match value {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer),
            Expression::Index { base, .. } => self.dereferenced_width(base),
            Expression::Member { member_type, .. } => Some(member_type.width()),
            _ => return Ok(false),
        };
        Ok(width == Some(8) && self.signedness_of(value)?)
    }

    /// Place two leaf operands for the equality idiom, extending narrow operands
    /// the way mwcc does: when both are narrow the left is extended in its home
    /// register and the right into the scratch; when only one is narrow it goes to
    /// the scratch and the wide operand stays in its home register. Build-aware via
    /// each leaf's signedness; transparent (home registers) for the all-int case.
    pub(crate) fn place_compare_leaves(&mut self, left: &Expression, right: &Expression) -> Compilation<(u8, u8)> {
        // Two single-instruction full-word loads: mwcc loads the left operand into
        // a fresh register (the allocator colors it at the lowest free GPR) and the
        // right into the scratch, in source order — `lwz r4,…; lwz r0,…`. The
        // equality idiom that follows (`subf r0,r4,r0; cntlzw; srwi 5`) then
        // matches. Variable-index subscripts (scaled `slwi; lwzx`) are excluded —
        // two of them mis-schedule, so they defer.
        if self.is_simple_word_load(left) && self.is_simple_word_load(right) {
            let left_register = self.fresh_virtual_general();
            self.evaluate_general(left, left_register)?;
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            return Ok((left_register, GENERAL_SCRATCH));
        }
        let (left_register, left_width, left_signed) = self.leaf_info(left)?;
        let (right_register, right_width, right_signed) = self.leaf_info(right)?;
        let left_narrow = left_width < 32;
        let right_narrow = right_width < 32;

        let (left_placed, right_placed) = if left_narrow && right_narrow {
            self.emit_widen(left_register, left_register, left_width, left_signed);
            self.emit_widen(GENERAL_SCRATCH, right_register, right_width, right_signed);
            (left_register, GENERAL_SCRATCH)
        } else if left_narrow {
            self.emit_widen(GENERAL_SCRATCH, left_register, left_width, left_signed);
            (GENERAL_SCRATCH, right_register)
        } else if right_narrow {
            self.emit_widen(GENERAL_SCRATCH, right_register, right_width, right_signed);
            (left_register, GENERAL_SCRATCH)
        } else {
            (left_register, right_register)
        };
        Ok((left_placed, right_placed))
    }

    /// Place the two operands of a general signed comparison into registers. A
    /// leaf stays in its home register; a single non-leaf operand is computed
    /// somewhere the idiom can keep it live across its two uses — and *where*
    /// depends on the idiom:
    ///
    ///  - `<`/`>` interleave the scratch (holding `a^b`) with the result-path
    ///    temp, so the non-leaf operand must survive in a *preserved* register
    ///    that AVOIDS the destination, leaving the destination free for that temp
    ///    (mwcc's coalescing — p->a > x, x < p->a).
    ///  - `!=` uses the operand in two ADJACENT subtractions with nothing
    ///    competing for the scratch between them, so mwcc evaluates it straight
    ///    into the *scratch* (r0) and lets the second subtraction overwrite it
    ///    (x != p->a, x != a*b+c). Deeper operands borrow the destination as an
    ///    internal temp but still settle into the scratch.
    ///
    /// Two non-leaf operands are not handled here.
    /// The register holding a comparison's right operand: a leaf in its home
    /// register, or a constant materialized into the scratch (`li r0, C`). The
    /// caller's idiom must read the right operand before overwriting r0.
    fn compare_right_operand(&mut self, right: &Expression) -> Compilation<u8> {
        if let Some(constant) = constant_value(right) {
            self.load_integer_constant(GENERAL_SCRATCH, constant);
            Ok(GENERAL_SCRATCH)
        } else {
            self.general_register_of_leaf(right)
        }
    }

    fn place_compare_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<(u8, u8)> {
        let left_leaf = leaf_name(left).is_some();
        let right_leaf = leaf_name(right).is_some();
        if left_leaf && right_leaf {
            return Ok((self.general_register_of_leaf(left)?, self.general_register_of_leaf(right)?));
        }
        if matches!(operator, BinaryOperator::NotEqual) {
            // The non-leaf operand goes into the scratch; the leaf keeps its home.
            return if left_leaf {
                let left_register = self.general_register_of_leaf(left)?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Ok((left_register, GENERAL_SCRATCH))
            } else {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                Ok((GENERAL_SCRATCH, self.general_register_of_leaf(right)?))
            };
        }
        match (left_leaf, right_leaf) {
            // Non-leaf LEFT (the `>` idiom keeps its left): evaluate it off the dest.
            (false, true) => {
                let right_register = self.general_register_of_leaf(right)?;
                let left_register = self.fresh_virtual_general_avoiding(vec![destination]);
                self.evaluate_general(left, left_register)?;
                Ok((left_register, right_register))
            }
            // Non-leaf RIGHT (the `<` idiom keeps its right): evaluate it off the dest.
            (true, false) => {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.fresh_virtual_general_avoiding(vec![destination]);
                self.evaluate_general(right, right_register)?;
                Ok((left_register, right_register))
            }
            // Two non-leaf operands are not handled yet.
            _ => Err(Diagnostic::error("this comparison operand shape needs the full register allocator (roadmap)")),
        }
    }
}
