//! 64-bit integers (`long long`): a value is a register pair, the high word
//! in the first register (`r3:r4` returned, arguments in odd-aligned pairs).

use super::*;

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
                let low = self.temporary();
                self.load_constant(low, i64::from(*value as i32))?;
                // (Equal halves share their register.)
                if i64::from(*value as i32) == value >> 32 {
                    return Ok((low, low));
                }
                let high = self.temporary();
                self.load_constant(high, value >> 32)?;
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
                let (a_high, a_low) = self.wide(left)?;
                let (b_high, b_low) = self.wide(right)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::AddCarrying { d: d_low, a: a_low, b: b_low });
                self.emit_plain(Instruction::AddExtended { d: d_high, a: a_high, b: b_high });
                Ok((d_high, d_low))
            }
            ExprKind::Binary(BinaryOp::Subtract, left, right) => {
                let (a_high, a_low) = self.wide(left)?;
                let (b_high, b_low) = self.wide(right)?;
                let (d_high, d_low) = (self.temporary(), self.temporary());
                self.emit_plain(Instruction::SubtractFromCarrying { d: d_low, a: b_low, b: a_low });
                self.emit_plain(Instruction::SubtractFromExtended { d: d_high, a: b_high, b: a_high });
                Ok((d_high, d_low))
            }
            ExprKind::Binary(op @ (BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor), left, right) => {
                let (a_high, a_low) = self.wide(left)?;
                let (b_high, b_low) = self.wide(right)?;
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
        let (high, low) = self.wide(value)?;
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
    fn wide_difference(&mut self, left: &Expr, right: &Expr, record: bool) -> Compilation<u32> {
        let (a_high, a_low) = self.wide(left)?;
        let (b_high, b_low) = self.wide(right)?;
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
        let (mut l_high, l_low) = self.wide(left)?;
        let (mut r_high, r_low) = self.wide(right)?;
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
