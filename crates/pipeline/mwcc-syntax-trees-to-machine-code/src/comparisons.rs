//! Branchless comparison idioms.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};
use crate::analysis::*;
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
                    // into the scratch first (extsb/clrlwi), then consumed there.
                    let value = match self.leaf_info(left) {
                        Ok((register, width, signed)) if width < 32 => {
                            self.emit_widen(GENERAL_SCRATCH, register, width, signed);
                            GENERAL_SCRATCH
                        }
                        _ => self.general_register_of_leaf(left)?,
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
                    && (leaf_name(left).is_some() || self.is_word_load(left))
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
            let (first, second) = if matches!(right, Expression::FloatLiteral(_)) { (b, a) } else { (a, b) };
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

    /// Place a floating-point comparison operand: a leaf stays in its register; a
    /// float literal is loaded from the constant pool (`lfs`/`lfd`) into the float
    /// scratch, matching mwcc's `x > 0.0` form.
    fn place_float_compare_operand(&mut self, operand: &Expression, double: bool) -> Compilation<u8> {
        if let Expression::FloatLiteral(value) = operand {
            if double {
                self.load_double_constant(FLOAT_SCRATCH, value.to_bits());
            } else {
                self.load_float_constant(FLOAT_SCRATCH, *value as f32);
            }
            return Ok(FLOAT_SCRATCH);
        }
        self.float_register_of_leaf(operand)
    }

    /// Whether `value` is a full-word (32-bit) memory load — a dereference,
    /// index, or struct member — which can be evaluated into a register and used
    /// as a comparison operand without narrow extension.
    fn is_word_load(&self, value: &Expression) -> bool {
        match value {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer) == Some(32),
            Expression::Index { base, .. } => self.dereferenced_width(base) == Some(32),
            Expression::Member { member_type, .. } => member_type.width() == 32,
            _ => false,
        }
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
