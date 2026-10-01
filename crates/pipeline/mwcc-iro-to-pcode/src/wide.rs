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
                let (high, low) = (self.temporary(), self.temporary());
                self.load_constant(low, i64::from(*value as i32))?;
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
        let (high, low) = self.wide(value)?;
        self.emit_plain(Instruction::Or { a: 4, s: low, b: low });
        self.emit_plain(Instruction::Or { a: 3, s: high, b: high });
        Ok(())
    }
}
