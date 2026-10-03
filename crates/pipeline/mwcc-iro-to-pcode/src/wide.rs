//! 64-bit integers (`long long`): a value is a register pair, the high word
//! in the first register (`r3:r4` returned, arguments in odd-aligned pairs).

use super::*;

/// A wide constant (possibly a converted word constant).
pub(super) fn long_constant(expression: &Expr) -> Option<i64> {
    match &expression.kind {
        ExprKind::Convert(inner) => inner.as_int(),
        _ => expression.as_int(),
    }
}

pub(super) fn is_wide(ty: Type) -> bool {
    matches!(ty, Type::LongLong | Type::UnsignedLongLong)
}

impl Lowerer<'_, '_> {
    /// A wide variable's (high, low) registers.
    pub(super) fn wide_registers(&mut self, variable: VarId) -> (u32, u32) {
        let low = self.register(variable);
        let high = match self.registers_hi[variable] {
            Some(register) => register,
            None => {
                let register = self.fresh(Type::Int);
                self.registers_hi[variable] = Some(register);
                register
            }
        };
        (high, low)
    }

    /// Evaluate a wide expression into (high, low) registers.
    pub(super) fn wide(&mut self, expression: &Expr) -> Compilation<(u32, u32)> {
        // (A constant is its value; a word operand extends by its own type.)
        if let Some(value) = expression.as_int().filter(|_| !is_wide(expression.ty)) {
            return self.wide(&Expr { kind: ExprKind::Int(value), ty: Type::LongLong });
        }
        if !is_wide(expression.ty) {
            if is_float(expression.ty) || !is_general_word(promote(expression.ty)) {
                return Err(unsupported("a wide operand of this type"));
            }
            let widened = Expr {
                kind: ExprKind::Convert(Box::new(expression.clone())),
                ty: if is_unsigned(promote(expression.ty)) { Type::UnsignedLongLong } else { Type::LongLong },
            };
            return self.wide(&widened);
        }
        match &expression.kind {
            // (A word variable retyped by folding widens.)
            ExprKind::Var(id) if !is_wide(self.function.variables[*id].ty) => {
                let word = Expr { kind: ExprKind::Var(*id), ty: self.function.variables[*id].ty };
                self.wide(&word)
            }
            ExprKind::Var(id) if self.homes[*id].is_none() => Ok(self.wide_registers(*id)),
            ExprKind::Int(value) => {
                let low = self.wide_word(*value as i32);
                // (Equal halves share their register.)
                if i64::from(*value as i32) == value >> 32 {
                    return Ok((low, low));
                }
                let high = self.wide_word((value >> 32) as i32);
                Ok((high, low))
            }
            ExprKind::Convert(operand) if is_wide(operand.ty) => self.wide(operand),
            ExprKind::Convert(operand) if operand.as_int().is_some() => self.wide(operand),
            // A word widens: its sign (or zero) fills the high word.
            ExprKind::Convert(operand) if is_general_word(promote(operand.ty)) => {
                // (A narrow operand promotes first.)
                let word = if is_narrow(operand.ty) {
                    Expr { kind: ExprKind::Convert(operand.clone()), ty: promote(operand.ty) }
                } else {
                    (**operand).clone()
                };
                let (low, _) = self.expression(&word)?;
                let high = self.temporary();
                // (An unsigned narrow value is never negative.)
                if is_unsigned(promote(operand.ty)) || is_unsigned_narrow(unpromoted(operand).ty) {
                    self.load_constant(high, 0)?;
                } else {
                    self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: high, s: low, shift: 31 });
                }
                Ok((high, low))
            }
            ExprKind::Binary(BinaryOp::Add, left, right) => {
                // (Adding a zero-extended word carries into the high word
                // alone: `addc; addze`.)
                let zero_extended = |e: &Expr| {
                    let word = match &e.kind {
                        ExprKind::Convert(word) if !is_wide(word.ty) => word.as_ref(),
                        _ if !is_wide(e.ty) => e,
                        _ => return false,
                    };
                    word.as_int().is_none() && is_unsigned(promote(word.ty)) && is_general_word(promote(word.ty))
                };
                let (wide_operand, word) = if zero_extended(right) {
                    (left, Some(right))
                } else if zero_extended(left) {
                    (right, Some(left))
                } else {
                    (left, None)
                };
                if let Some(word) = word {
                    let word = match &word.kind {
                        ExprKind::Convert(inner) if !is_wide(inner.ty) => inner.as_ref(),
                        _ => word,
                    };
                    let (high, low) = self.wide(wide_operand)?;
                    let (value, _) = self.expression(word)?;
                    let (d_high, d_low) = (self.temporary(), self.temporary());
                    self.emit_plain(Instruction::AddCarrying { d: d_low, a: low, b: value });
                    self.emit_plain(Instruction::AddToZeroExtended { d: d_high, a: high });
                    return Ok((d_high, d_low));
                }
                let ((a_high, a_low), (b_high, b_low)) = self.wide_operands(left, right)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::AddCarrying { d: d_low, a: a_low, b: b_low });
                self.emit_plain(Instruction::AddExtended { d: d_high, a: a_high, b: b_high });
                Ok((d_high, d_low))
            }
            ExprKind::Binary(BinaryOp::Subtract, left, right) => {
                // (A constant subtrahend is added negated: `addc; adde`.)
                if let Some(value) = long_constant(right).filter(|_| left.as_int().is_none() && !super::toggle("MWCC_PCODE_WIDE_SUBTRACT_CONSTANTS")) {
                    let negated = Expr { kind: ExprKind::Int(value.wrapping_neg()), ty: expression.ty };
                    let sum = Expr { kind: ExprKind::Binary(BinaryOp::Add, left.clone(), Box::new(negated)), ty: expression.ty };
                    return self.wide(&sum);
                }
                let ((a_high, a_low), (b_high, b_low)) = self.wide_operands(left, right)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::SubtractFromCarrying { d: d_low, a: b_low, b: a_low });
                self.emit_plain(Instruction::SubtractFromExtended { d: d_high, a: b_high, b: a_high });
                Ok((d_high, d_low))
            }
            ExprKind::Binary(op @ (BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor), left, right) => {
                let ((a_high, a_low), (b_high, b_low)) = self.wide_operands(left, right)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                let make = |d, s, b| match op {
                    BinaryOp::BitAnd => Instruction::And { a: d, s, b },
                    BinaryOp::BitOr => Instruction::Or { a: d, s, b },
                    _ => Instruction::Xor { a: d, s, b },
                };
                self.emit_plain(make(d_low, a_low, b_low));
                self.emit_plain(make(d_high, a_high, b_high));
                Ok((d_high, d_low))
            }
            ExprKind::Binary(BinaryOp::Multiply, left, right) => {
                // (By one, minus one or (after the early builds) a power of
                // two: the value, its negation, a shift.)
                match right.as_int() {
                    Some(1) => return self.wide(left),
                    Some(-1) => {
                        let negation = Expr { kind: ExprKind::Unary(UnaryOp::Negate, left.clone()), ty: expression.ty };
                        return self.wide(&negation);
                    }
                    _ if self.unit.early_frame => return self.early_wide_multiply(left, right),
                    Some(k) if k > 1 && k.count_ones() == 1 => {
                        let count = Expr { kind: ExprKind::Int(i64::from(k.trailing_zeros())), ty: Type::Int };
                        let shift = Expr { kind: ExprKind::Binary(BinaryOp::ShiftLeft, left.clone(), Box::new(count)), ty: expression.ty };
                        return self.wide(&shift);
                    }
                    _ => {}
                }
                let (a_high, a_low) = self.wide(left)?;
                // `hi = mulhwu(al, bl) + ah*bl + al*bh`, `lo = al*bl`. Only
                // a constant in 0..32768 skips the `al*bh` term (`mulli`); any
                // other has its high word, zero or not, in a register.
                let short = right.as_int().and_then(|k| i16::try_from(k).ok()).filter(|&k| k >= 0);
                let (b_high, b_low) = match short {
                    Some(k) => {
                        let b_low = self.temporary();
                        self.load_constant(b_low, i64::from(k))?;
                        (None, b_low)
                    }
                    None => {
                        let (b_high, b_low) = self.wide(right)?;
                        (Some(b_high), b_low)
                    }
                };
                let carry = self.temporary();
                self.emit_plain(Instruction::MultiplyHighWordUnsigned { d: carry, a: a_low, b: b_low });
                let cross = self.temporary();
                self.emit_plain(Instruction::MultiplyLow { d: cross, a: a_high, b: b_low });
                let d_low = self.temporary();
                match short {
                    Some(immediate) => self.emit_plain(Instruction::MultiplyImmediate { d: d_low, a: a_low, immediate }),
                    None => self.emit_plain(Instruction::MultiplyLow { d: d_low, a: a_low, b: b_low }),
                }
                let partial = self.temporary();
                self.emit_plain(Instruction::Add { d: partial, a: carry, b: cross });
                if short.is_some() {
                    return Ok((partial, d_low));
                }
                let b_high = b_high.expect("a full multiplier has its high word");
                let other = self.temporary();
                self.emit_plain(Instruction::MultiplyLow { d: other, a: a_low, b: b_high });
                let d_high = self.temporary();
                self.emit_plain(Instruction::Add { d: d_high, a: partial, b: other });
                Ok((d_high, d_low))
            }
            ExprKind::Binary(op @ (BinaryOp::ShiftLeft | BinaryOp::ShiftRight), left, right) => {
                let signed = expression.ty == Type::LongLong;
                let Some(count) = right.as_int().filter(|count| (0..64).contains(count)) else {
                    // A variable count: `__shl2i`, `__shr2u`, `__shr2i`.
                    let helper = match (op, signed) {
                        (BinaryOp::ShiftLeft, _) => "__shl2i",
                        (_, false) => "__shr2u",
                        (_, true) => "__shr2i",
                    };
                    return self.wide_helper(helper, left, right);
                };
                let (high, low) = self.wide(left)?;
                let count = count as u8;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                match (op, count) {
                    (_, 0) => return Ok((high, low)),
                    // (By 32, the words move.)
                    (BinaryOp::ShiftLeft, 32) => {
                        self.emit_plain(Instruction::Or { a: d_high, s: low, b: low });
                        self.load_constant(d_low, 0)?;
                    }
                    (_, 32) => {
                        self.emit_plain(Instruction::Or { a: d_low, s: high, b: high });
                        if signed {
                            self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: d_high, s: high, shift: 31 });
                        } else {
                            self.load_constant(d_high, 0)?;
                        }
                    }
                    (BinaryOp::ShiftLeft, 1..=31) => {
                        self.emit_plain(Instruction::ShiftLeftImmediate { a: d_low, s: low, shift: count });
                        self.emit_plain(Instruction::ShiftLeftImmediate { a: d_high, s: high, shift: count });
                        self.emit_plain(Instruction::RotateAndMaskInsert { a: d_high, s: low, shift: count, begin: 32 - count, end: 31 });
                    }
                    (BinaryOp::ShiftLeft, _) => {
                        self.emit_plain(Instruction::ShiftLeftImmediate { a: d_high, s: low, shift: count - 32 });
                        self.load_constant(d_low, 0)?;
                    }
                    (_, 1..=31) => {
                        self.emit_plain(if signed {
                            Instruction::ShiftRightAlgebraicImmediate { a: d_high, s: high, shift: count }
                        } else {
                            Instruction::ShiftRightLogicalImmediate { a: d_high, s: high, shift: count }
                        });
                        self.emit_plain(Instruction::RotateAndMask { a: d_low, s: low, shift: 32 - count, begin: 0, end: 31 });
                        self.emit_plain(Instruction::RotateAndMaskInsert { a: d_low, s: high, shift: 32 - count, begin: 0, end: count - 1 });
                    }
                    _ => {
                        self.emit_plain(if signed {
                            Instruction::ShiftRightAlgebraicImmediate { a: d_low, s: high, shift: count - 32 }
                        } else {
                            Instruction::ShiftRightLogicalImmediate { a: d_low, s: high, shift: count - 32 }
                        });
                        if signed {
                            self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: d_high, s: high, shift: 31 });
                        } else {
                            self.load_constant(d_high, 0)?;
                        }
                    }
                }
                Ok((d_high, d_low))
            }
            // A signed quotient by 2^k (k < 32): an arithmetic shift whose
            // carry (negative, with bits shifted out) rounds toward zero.
            ExprKind::Binary(BinaryOp::Divide, left, right)
                if expression.ty == Type::LongLong
                    && right.as_int().is_some_and(|d| d > 1 && d.count_ones() == 1 && d.trailing_zeros() < 32) =>
            {
                let power = right.as_int().expect("checked").trailing_zeros() as u8;
                let (high, low) = self.wide(left)?;
                let marked = self.temporary();
                self.emit_plain(Instruction::Or { a: marked, s: high, b: high });
                let d_low = self.temporary();
                self.emit_plain(Instruction::RotateAndMask { a: d_low, s: low, shift: 32 - power, begin: 0, end: 31 });
                self.emit_plain(Instruction::RotateAndMaskInsert { a: marked, s: low, shift: 0, begin: 32 - power, end: 31 });
                self.emit_plain(Instruction::RotateAndMaskInsert { a: d_low, s: high, shift: 32 - power, begin: 0, end: power - 1 });
                let shifted = self.temporary();
                self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: shifted, s: marked, shift: power });
                let (r_low, r_high) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::AddToZeroExtended { d: r_low, a: d_low });
                self.emit_plain(Instruction::AddToZeroExtended { d: r_high, a: shifted });
                Ok((r_high, r_low))
            }
            // Division calls the runtime: `__div2u`, `__div2i`, `__mod2u`, `__mod2i`.
            ExprKind::Binary(op @ (BinaryOp::Divide | BinaryOp::Modulo), left, right) => {
                let signed = expression.ty == Type::LongLong;
                let helper = match (op, signed) {
                    (BinaryOp::Divide, false) => "__div2u",
                    (BinaryOp::Divide, true) => "__div2i",
                    (_, false) => "__mod2u",
                    (_, true) => "__mod2i",
                };
                let ty = expression.ty;
                let right = if is_wide(right.ty) { (**right).clone() } else { Expr { kind: ExprKind::Convert(right.clone()), ty } };
                self.wide_helper(helper, left, &right)
            }
            ExprKind::Unary(UnaryOp::Negate, operand) => {
                let (high, low) = self.wide(operand)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::SubtractFromImmediate { d: d_low, a: low, immediate: 0 });
                self.emit_plain(Instruction::SubtractFromZeroExtended { d: d_high, a: high });
                Ok((d_high, d_low))
            }
            ExprKind::Unary(UnaryOp::BitNot, operand) => {
                let (high, low) = self.wide(operand)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::Nor { a: d_low, s: low, b: low });
                self.emit_plain(Instruction::Nor { a: d_high, s: high, b: high });
                Ok((d_high, d_low))
            }
            ExprKind::Load { base, index: None, offset } => {
                let (address, _) = self.base_expression(base)?;
                let (address, displacement) = self.displacement(address, *offset)?;
                let next = displacement.checked_add(4).ok_or_else(|| unsupported("a wide load past the displacement"))?;
                let (high, _) = self.load(Type::Int, address, displacement, None, None)?;
                let (low, _) = self.load(Type::Int, address, next, None, None)?;
                Ok((high, low))
            }
            // (An indexed one adds the index to the base first.)
            ExprKind::Load { base, index: Some(index), offset } if !super::toggle("MWCC_PCODE_NO_INDEXED_WIDE_LOADS") => {
                // (The index first, but for GC/3.x and Wii.)
                let (scaled, pointer) = if self.unit.tied_halves {
                    let (pointer, _) = self.expression(base)?;
                    (self.expression(index)?.0, pointer)
                } else {
                    let (scaled, _) = self.expression(index)?;
                    (scaled, self.expression(base)?.0)
                };
                let sum = self.temporary();
                self.emit_plain(Instruction::Add { d: sum, a: pointer, b: scaled });
                let (address, displacement) = self.displacement(sum, *offset)?;
                let next = displacement.checked_add(4).ok_or_else(|| unsupported("a wide load past the displacement"))?;
                let (high, _) = self.load(Type::Int, address, displacement, None, None)?;
                let (low, _) = self.load(Type::Int, address, next, None, None)?;
                Ok((high, low))
            }
            ExprKind::Global(name) => {
                let global = self.unit.globals[name];
                if global.small_data {
                    let word = |addend| AttachedRelocation {
                        kind: RelocationKind::EmbSda21,
                        target: if addend == 0 {
                            RelocationTarget::External(name.clone())
                        } else {
                            RelocationTarget::ExternalWithAddend(name.clone(), addend)
                        },
                    };
                    let (high, _) = self.load(Type::Int, 0, 0, Some(word(0)), None)?;
                    let (low, _) = self.load(Type::Int, 0, 0, Some(word(4)), None)?;
                    return Ok((high, low));
                }
                let address = self.absolute_address(name);
                let (high, _) = self.load(Type::Int, address, 0, None, None)?;
                let (low, _) = self.load(Type::Int, address, 4, None, None)?;
                Ok((high, low))
            }
            ExprKind::Call { name, arguments } => {
                self.call(name, arguments, Type::Void, None)?;
                let (high, low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::Or { a: high, s: 3, b: 3 });
                self.emit_plain(Instruction::Or { a: low, s: 4, b: 4 });
                Ok((high, low))
            }
            other => {
                let name = format!("{other:?}");
                let end = name.find(['(', ' ', '{']).unwrap_or(name.len());
                Err(unsupported(format!("wide expression {}", &name[..end])))
            }
        }
    }

    pub(super) fn assign_wide(&mut self, variable: VarId, value: &Expr) -> Compilation<()> {
        if self.homes[variable].is_some() {
            return Err(unsupported("a wide variable in the frame"));
        }
        let (high, low) = self.wide(value)?;
        let (v_high, v_low) = self.wide_registers(variable);
        if low != v_low {
            self.emit_plain(Instruction::Or { a: v_low, s: low, b: low });
        }
        if high != v_high {
            self.emit_plain(Instruction::Or { a: v_high, s: high, b: high });
        }
        Ok(())
    }

    /// A wide store: the low word, then the high word.
    pub(super) fn store_wide(&mut self, place: &Place, value: &Expr) -> Compilation<()> {
        let (high, low) = self.wide(value)?;
        match place {
            Place::Memory { base, index: None, offset } => {
                let (address, _) = self.base_expression(base)?;
                let (address, displacement) = self.displacement(address, *offset)?;
                let next = displacement.checked_add(4).ok_or_else(|| unsupported("a wide store past the displacement"))?;
                self.emit_based(Instruction::StoreWord { s: low, a: address, offset: next }, address);
                self.emit_based(Instruction::StoreWord { s: high, a: address, offset: displacement }, address);
            }
            Place::Global(name) => {
                let global = self.unit.globals[name];
                if !global.small_data {
                    let address = self.absolute_address(name);
                    self.emit_based(Instruction::StoreWord { s: low, a: address, offset: 4 }, address);
                    self.emit_based(Instruction::StoreWord { s: high, a: address, offset: 0 }, address);
                } else {
                    for (word, addend) in [(low, 4), (high, 0)] {
                        let mut store = PInstr::new(Instruction::StoreWord { s: word, a: 0, offset: 0 });
                        store.relocation = Some(AttachedRelocation {
                            kind: RelocationKind::EmbSda21,
                            target: if addend == 0 {
                                RelocationTarget::External(name.clone())
                            } else {
                                RelocationTarget::ExternalWithAddend(name.clone(), addend)
                            },
                        });
                        self.emit(store);
                    }
                }
            }
            // (An indexed one adds the index to the base first.)
            Place::Memory { base, index: Some(index), offset } if !super::toggle("MWCC_PCODE_NO_INDEXED_WIDE_STORES") => {
                // (The index first, but for GC/3.x and Wii.)
                let (scaled, pointer) = if self.unit.tied_halves {
                    let (pointer, _) = self.expression(base)?;
                    (self.expression(index)?.0, pointer)
                } else {
                    let (scaled, _) = self.expression(index)?;
                    (scaled, self.expression(base)?.0)
                };
                let sum = self.temporary();
                self.emit_plain(Instruction::Add { d: sum, a: pointer, b: scaled });
                let (address, displacement) = self.displacement(sum, *offset)?;
                let next = displacement.checked_add(4).ok_or_else(|| unsupported("a wide store past the displacement"))?;
                self.emit_based(Instruction::StoreWord { s: low, a: address, offset: next }, address);
                self.emit_based(Instruction::StoreWord { s: high, a: address, offset: displacement }, address);
            }
            _ => return Err(unsupported("an indexed wide store")),
        }
        self.forget_loaded_globals(false);
        Ok(())
    }

    /// The returned wide value in `r3:r4`.
    pub(super) fn return_wide(&mut self, value: &Expr) -> Compilation<()> {
        // (A constant loads each half straight into its register.)
        let constant = match &value.kind {
            ExprKind::Convert(inner) => inner.as_int(),
            _ => value.as_int(),
        };
        if let Some(constant) = constant {
            self.load_constant(4, i64::from(constant as i32))?;
            self.load_constant(3, constant >> 32)?;
            return Ok(());
        }
        // (A widened word: its low word copied first, the high one made in
        // r3 from it.)
        let word = match &value.kind {
            ExprKind::Convert(inner) if !is_wide(inner.ty) => Some(inner.as_ref()),
            _ if !is_wide(value.ty) => Some(value),
            _ => None,
        };
        if let Some(inner) = word {
            if is_general_word(promote(inner.ty)) && !is_narrow(inner.ty) && !toggle("MWCC_PCODE_WIDE_RETURN_HIGH_FIRST") {
                // (A computed value lands in r4 itself.)
                let (x, _) = if matches!(inner.kind, ExprKind::Var(_)) { self.expression(inner)? } else { self.expression_with_target(inner, Some(4))? };
                // (GC/1.0-1.2.5n copy with `addi`.)
                if x == 4 {
                } else if self.unit.early_frame {
                    self.emit_based(Instruction::AddImmediate { d: 4, a: x, immediate: 0 }, x);
                } else {
                    self.emit_plain(Instruction::Or { a: 4, s: x, b: x });
                }
                if !is_unsigned(promote(inner.ty)) {
                    self.emit_plain(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: x, shift: 31 });
                } else {
                    self.load_constant(3, 0)?;
                }
                return Ok(());
            }
        }
        let (high, low) = self.wide(value)?;
        // (GC/3.x and Wii copy the high word first.)
        if self.unit.equality_subtracts_constant && !super::toggle("MWCC_PCODE_WIDE_RETURN_LOW_FIRST") {
            self.emit_plain(Instruction::Or { a: 3, s: high, b: high });
            self.emit_plain(Instruction::Or { a: 4, s: low, b: low });
            return Ok(());
        }
        self.emit_plain(Instruction::Or { a: 4, s: low, b: low });
        self.emit_plain(Instruction::Or { a: 3, s: high, b: high });
        Ok(())
    }
}

impl Lowerer<'_, '_> {
    /// Compare wide operands for a branch: the cr0 bit to test and whether
    /// the relation holds when it is set.
    pub(super) fn wide_compare(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Compilation<(u8, bool)> {
        match op {
            BinaryOp::Equal | BinaryOp::NotEqual => {
                self.wide_difference(left, right, true)?;
                Ok((2, op == BinaryOp::Equal))
            }
            _ => {
                let borrow = self.wide_borrow(op, left, right)?;
                let d = self.temporary();
                self.emit_plain(Instruction::NegateRecord { d, a: borrow });
                Ok((2, matches!(op, BinaryOp::LessEqual | BinaryOp::GreaterEqual)))
            }
        }
    }

    /// A wide comparison as a 0/1 value.
    pub(super) fn wide_compare_value(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Compilation<u32> {
        match op {
            BinaryOp::Equal => {
                let difference = self.wide_difference(left, right, false)?;
                let zeros = self.temporary();
                self.emit_plain(Instruction::CountLeadingZeros { a: zeros, s: difference });
                let d = self.temporary();
                self.emit_plain(Instruction::RotateAndMask { a: d, s: zeros, shift: 27, begin: 5, end: 31 });
                Ok(d)
            }
            BinaryOp::NotEqual => {
                let difference = self.wide_difference(left, right, false)?;
                let less = self.temporary();
                self.emit_plain(Instruction::AddImmediateCarrying { d: less, a: difference, immediate: -1 });
                let d = self.temporary();
                self.emit_plain(Instruction::SubtractFromExtended { d, a: less, b: difference });
                Ok(d)
            }
            _ => {
                let borrow = self.wide_borrow(op, left, right)?;
                let d = self.temporary();
                self.emit_plain(Instruction::Negate { d, a: borrow });
                if matches!(op, BinaryOp::LessEqual | BinaryOp::GreaterEqual) {
                    let inverted = self.temporary();
                    self.emit_plain(Instruction::SubtractFromImmediate { d: inverted, a: d, immediate: 1 });
                    return Ok(inverted);
                }
                Ok(d)
            }
        }
    }

    /// `(a_lo ^ b_lo) | (a_hi ^ b_hi)` (recorded for a branch).
    /// Both operands of a wide operation: a call's first (its result
    /// returns in registers the other operand's loads would have to
    /// survive).
    fn wide_operands(&mut self, left: &Expr, right: &Expr) -> Compilation<((u32, u32), (u32, u32))> {
        if super::contains_call(right) && !super::contains_call(left) && !super::toggle("MWCC_PCODE_WIDE_CALL_OPERAND_IN_ORDER") {
            let r = self.wide(right)?;
            let l = self.wide(left)?;
            return Ok((l, r));
        }
        let l = self.wide(left)?;
        let r = self.wide(right)?;
        Ok((l, r))
    }

    fn wide_difference(&mut self, left: &Expr, right: &Expr, record: bool) -> Compilation<u32> {
        let ((a_high, a_low), (b_high, b_low)) = self.wide_operands(left, right)?;
        let (low, high) = (self.temporary(), self.temporary());
        self.emit_plain(Instruction::Xor { a: low, s: a_low, b: b_low });
        self.emit_plain(Instruction::Xor { a: high, s: a_high, b: b_high });
        let d = self.temporary();
        self.emit_plain(if record { Instruction::OrRecord { a: d, s: low, b: high } } else { Instruction::Or { a: d, s: low, b: high } });
        Ok(d)
    }

    /// `-1` when the comparison's minuend is below its subtrahend, else 0
    /// (`subfc; subfe; subfe x,x`): `a < b` and `a >= b` subtract `b` from
    /// `a`, `a > b` and `a <= b` `a` from `b`. Signed operands flip their
    /// high words' signs first.
    fn wide_borrow(&mut self, op: BinaryOp, left: &Expr, right: &Expr) -> Compilation<u32> {
        let ((mut l_high, l_low), (mut r_high, r_low)) = self.wide_operands(left, right)?;
        let signed = left.ty != Type::UnsignedLongLong && right.ty != Type::UnsignedLongLong;
        if signed {
            let flipped = self.temporary();
            self.emit_plain(Instruction::XorImmediateShifted { a: flipped, s: l_high, immediate: 0x8000 });
            l_high = flipped;
            let flipped = self.temporary();
            self.emit_plain(Instruction::XorImmediateShifted { a: flipped, s: r_high, immediate: 0x8000 });
            r_high = flipped;
        }
        let ((m_high, m_low), (s_high, s_low)) = match op {
            BinaryOp::Less | BinaryOp::GreaterEqual => ((l_high, l_low), (r_high, r_low)),
            _ => ((r_high, r_low), (l_high, l_low)),
        };
        let low = self.temporary();
        self.emit_plain(Instruction::SubtractFromCarrying { d: low, a: s_low, b: m_low });
        let high = self.temporary();
        self.emit_plain(Instruction::SubtractFromExtended { d: high, a: s_high, b: m_high });
        // (Of the left operand's high word, whose value cancels.)
        let filler = if self.unit.signed_promoted_truth { l_low } else { l_high };
        let borrow = self.temporary();
        self.emit_plain(Instruction::SubtractFromExtended { d: borrow, a: filler, b: filler });
        Ok(borrow)
    }
}

impl Lowerer<'_, '_> {
    /// A runtime helper's wide result (`r3:r4`).
    fn wide_helper(&mut self, name: &str, left: &Expr, right: &Expr) -> Compilation<(u32, u32)> {
        let left = if is_wide(left.ty) {
            left.clone()
        } else {
            Expr { kind: ExprKind::Convert(Box::new(left.clone())), ty: Type::LongLong }
        };
        self.call(name, &[left, right.clone()], Type::Void, None)?;
        let (high, low) = (self.temporary(), self.temporary());
        self.emit_plain(Instruction::Or { a: high, s: 3, b: 3 });
        self.emit_plain(Instruction::Or { a: low, s: 4, b: 4 });
        Ok((high, low))
    }

    /// One word of a wide constant; `lis` and `addi` write separate registers.
    fn wide_word(&mut self, value: i32) -> u32 {
        let low = value as i16;
        if i16::try_from(value).is_ok() || low == 0 {
            let d = self.temporary();
            self.load_constant(d, i64::from(value)).expect("a word constant");
            return d;
        }
        let upper = self.temporary();
        let high = ((value - i32::from(low)) >> 16) as i16;
        self.emit_plain(Instruction::AddImmediateShifted { d: upper, a: 0, immediate: high });
        let d = self.temporary();
        let mut addi = PInstr::new(Instruction::AddImmediate { d, a: upper, immediate: low });
        addi.not_r0.push(upper);
        self.emit(addi);
        d
    }

    /// Early builds' `a * b`: every term, `ah*bl` first.
    fn early_wide_multiply(&mut self, left: &Expr, right: &Expr) -> Compilation<(u32, u32)> {
        let (a_high, a_low) = self.wide(left)?;
        let constant = right.as_int();
        let (variable_high, b_low) = match constant {
            Some(k) => (None, self.wide_word(k as i32)),
            None => {
                let (high, low) = self.wide(right)?;
                (Some(high), low)
            }
        };
        let cross = self.temporary();
        self.emit_plain(Instruction::MultiplyLow { d: cross, a: a_high, b: b_low });
        let carry = self.temporary();
        self.emit_plain(Instruction::MultiplyHighWordUnsigned { d: carry, a: a_low, b: b_low });
        let b_high = match (variable_high, constant) {
            (Some(high), _) => high,
            (None, k) => self.wide_word((k.unwrap_or(0) >> 32) as i32),
        };
        let partial = self.temporary();
        self.emit_plain(Instruction::Add { d: partial, a: cross, b: carry });
        let other = self.temporary();
        self.emit_plain(Instruction::MultiplyLow { d: other, a: a_low, b: b_high });
        let d_low = self.temporary();
        self.emit_plain(Instruction::MultiplyLow { d: d_low, a: a_low, b: b_low });
        let d_high = self.temporary();
        self.emit_plain(Instruction::Add { d: d_high, a: partial, b: other });
        Ok((d_high, d_low))
    }
}
