//! Diamond/ladder families (align, ilogb, early ladders).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// The WRITEBACK NORM (fire 432, e_fmod's normalize-output tail):
    ///   hx = (hx - HI_BIT) | ((iy + K) << S);
    ///   __HI(x) = hx | sx;  __LO(x) = lx;  return x;
    /// Measured: `hx - HI_BIT` folds to `addis` (high-half subtract);
    /// the stfd spill DELAYS into the int computation; the two punned
    /// stores REORDER BY READINESS (the LO store's lx was ready before
    /// the or-chain, so it emits first); the reload lfd feeds the
    /// return; frame 16 for the one punned double.
    pub(crate) fn try_writeback_norm(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_x, p_hx, p_lx, p_iy, p_sx] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_x.parameter_type != Type::Double
            || p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_iy.parameter_type != Type::Int
            || p_sx.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (x, hx, lx, iy, sx) = (
            p_x.name.as_str(),
            p_hx.name.as_str(),
            p_lx.name.as_str(),
            p_iy.name.as_str(),
            p_sx.name.as_str(),
        );
        let [Statement::Assign {
            name: assign_name,
            value: assign_value,
        }, Statement::Store {
            target: high_target,
            value: high_value,
        }, Statement::Store {
            target: low_target,
            value: low_value,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if assign_name != hx {
            return Ok(false);
        }
        // hx = (hx - HI_BIT) | ((iy + K) << S).
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: base,
            right: shifted,
        } = assign_value
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: sub_left,
            right: sub_right,
        } = base.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(sub_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(hi_bit) = sub_right.as_ref() else {
            return Ok(false);
        };
        if *hi_bit & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(addis_immediate) = i16::try_from(-(*hi_bit >> 16)) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: sum,
            right: shift_amount,
        } = shifted.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: add_left,
            right: add_right,
        } = sum.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(add_left.as_ref(), Expression::Variable(v) if v == iy) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(exponent_bias) = add_right.as_ref() else {
            return Ok(false);
        };
        let Ok(exponent_bias) = i16::try_from(*exponent_bias) else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(shift_amount) = shift_amount.as_ref() else {
            return Ok(false);
        };
        let Ok(shift_amount) = u8::try_from(*shift_amount) else {
            return Ok(false);
        };
        if !(1..=31).contains(&shift_amount) {
            return Ok(false);
        }
        // __HI(x) = hx | sx;  __LO(x) = lx;
        if crate::frame::pun_word_offset_pub(high_target, x) != Some(0)
            || crate::frame::pun_word_offset_pub(low_target, x) != Some(4)
        {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: or_left,
            right: or_right,
        } = high_value
        else {
            return Ok(false);
        };
        if !matches!(or_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(or_right.as_ref(), Expression::Variable(v) if v == sx)
            || !matches!(low_value, Expression::Variable(v) if v == lx)
            || !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == x)
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(iy_register), Some(sx_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(iy),
            self.lookup_general(sx),
        ) else {
            return Ok(false);
        };
        // -- emit --
        self.frame_size = 16;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: iy_register,
            immediate: exponent_bias,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: hx_register,
                a: hx_register,
                immediate: addis_immediate,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: shift_amount,
            });
        // The spill delays into the int computation.
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::Or {
            a: 0,
            s: hx_register,
            b: 0,
        });
        self.output.instructions.push(Instruction::Or {
            a: 0,
            s: 0,
            b: sx_register,
        });
        // Stores reorder by readiness: lx first.
        self.output.instructions.push(Instruction::StoreWord {
            s: lx_register,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ALIGN DIAMOND (fire 431, e_fmod's subnormal shift-to-normal):
    ///   if (ix >= K) hx = HI_BIT | (LOW_MASK & hx);
    ///   else { n = K - ix;  // wait: n = -1022 - ix with K = -1022
    ///          if (n <= 31) { hx = (hx<<n)|(lx>>(32-n)); lx <<= n; }
    ///          else { hx = lx << (n-32); lx = 0; } }
    ///   return hx + (int)lx;
    /// Measured: the new hx CONVERGES IN r0 from all three arms (a join
    /// register); `HI_BIT |` folds to `oris` (low half zero); n takes
    /// ix's home via `subfic r5,r5,K`; `32-n` is `subfic r0`; `n-32` is
    /// `addi r0,-32`; lx's in-place `slw` schedules INTO the srw->or
    /// latency; `lx = 0` is li r4,0 and the join adds r0+r4.
    pub(crate) fn try_align_diamond(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_ix] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_ix.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (hx, lx, ix) = (p_hx.name.as_str(), p_lx.name.as_str(), p_ix.name.as_str());
        let [Statement::If {
            condition: outer,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // Outer: ix >= K (i16).
        let Expression::Binary {
            operator: BinaryOperator::GreaterEqual,
            left: outer_left,
            right: outer_right,
        } = outer
        else {
            return Ok(false);
        };
        if !matches!(outer_left.as_ref(), Expression::Variable(v) if v == ix) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(threshold) = outer_right.as_ref() else {
            return Ok(false);
        };
        let Ok(threshold) = i16::try_from(*threshold) else {
            return Ok(false);
        };
        // Then arm: hx = HI_BIT | (LOW_MASK & hx) — oris + clrlwi form.
        let [Statement::Assign {
            name: then_name,
            value: then_value,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        if then_name != hx {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: hi_bit,
            right: masked,
        } = then_value
        else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(hi_bit) = hi_bit.as_ref() else {
            return Ok(false);
        };
        if *hi_bit & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(oris_immediate) = u16::try_from(*hi_bit >> 16) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: mask,
            right: mask_source,
        } = masked.as_ref()
        else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(mask) = mask.as_ref() else {
            return Ok(false);
        };
        let mask = *mask as u32;
        if mask == 0
            || !(mask as u64 + 1).is_power_of_two()
            || !matches!(mask_source.as_ref(), Expression::Variable(v) if v == hx)
        {
            return Ok(false);
        }
        let clear = mask.leading_zeros() as u8;
        // Else arm: [n = K - ix][the inner shift diamond].
        let [Statement::Assign {
            name: n,
            value: n_value,
        }, Statement::If {
            condition: inner,
            then_body: small_arm,
            else_body: big_arm,
        }] = else_body.as_slice()
        else {
            return Ok(false);
        };
        if n == hx || n == lx || n == ix {
            return Ok(false);
        }
        if !function.locals.iter().any(|local| {
            local.name == *n && local.declared_type == Type::Int && local.initializer.is_none()
        }) {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: n_left,
            right: n_right,
        } = n_value
        else {
            return Ok(false);
        };
        if !matches!(n_left.as_ref(), Expression::IntegerLiteral(k) if i16::try_from(*k) == Ok(threshold))
            || !matches!(n_right.as_ref(), Expression::Variable(v) if v == ix)
        {
            return Ok(false);
        }
        // Inner: n <= 31.
        let Expression::Binary {
            operator: BinaryOperator::LessEqual,
            left: inner_left,
            right: inner_right,
        } = inner
        else {
            return Ok(false);
        };
        if !matches!(inner_left.as_ref(), Expression::Variable(v) if v == n)
            || !matches!(inner_right.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        // Small arm: hx = (hx<<n)|(lx>>(32-n)); lx <<= n;
        let [Statement::Assign {
            name: sh_name,
            value: sh_value,
        }, Statement::Assign {
            name: sl_name,
            value: sl_value,
        }] = small_arm.as_slice()
        else {
            return Ok(false);
        };
        if sh_name != hx || sl_name != lx {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: shifted_high,
            right: shifted_low,
        } = sh_value
        else {
            return Ok(false);
        };
        let shift_of =
            |expression: &Expression, operator: BinaryOperator, value: &str| -> Option<()> {
                let Expression::Binary {
                    operator: found,
                    left,
                    right,
                } = expression
                else {
                    return None;
                };
                if *found != operator
                    || !matches!(left.as_ref(), Expression::Variable(v) if v == value)
                {
                    return None;
                }
                match right.as_ref() {
                    Expression::Variable(v) if v == n => Some(()),
                    _ => None,
                }
            };
        if shift_of(shifted_high.as_ref(), BinaryOperator::ShiftLeft, hx).is_none() {
            return Ok(false);
        }
        {
            let Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                left: low_source,
                right: amount,
            } = shifted_low.as_ref()
            else {
                return Ok(false);
            };
            if !matches!(low_source.as_ref(), Expression::Variable(v) if v == lx) {
                return Ok(false);
            }
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: from,
                right: taken,
            } = amount.as_ref()
            else {
                return Ok(false);
            };
            if !matches!(from.as_ref(), Expression::IntegerLiteral(32))
                || !matches!(taken.as_ref(), Expression::Variable(v) if v == n)
            {
                return Ok(false);
            }
        }
        if shift_of(sl_value, BinaryOperator::ShiftLeft, lx).is_none() {
            return Ok(false);
        }
        // Big arm: hx = lx << (n-32); lx = 0;
        let [Statement::Assign {
            name: bh_name,
            value: bh_value,
        }, Statement::Assign {
            name: bl_name,
            value: bl_value,
        }] = big_arm.as_slice()
        else {
            return Ok(false);
        };
        if bh_name != hx || bl_name != lx || !matches!(bl_value, Expression::IntegerLiteral(0)) {
            return Ok(false);
        }
        {
            let Expression::Binary {
                operator: BinaryOperator::ShiftLeft,
                left: low_source,
                right: amount,
            } = bh_value
            else {
                return Ok(false);
            };
            if !matches!(low_source.as_ref(), Expression::Variable(v) if v == lx) {
                return Ok(false);
            }
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: from,
                right: taken,
            } = amount.as_ref()
            else {
                return Ok(false);
            };
            if !matches!(from.as_ref(), Expression::Variable(v) if v == n)
                || !matches!(taken.as_ref(), Expression::IntegerLiteral(32))
            {
                return Ok(false);
            }
        }
        // Return: hx + (int)lx.
        let Some(Expression::Binary {
            operator: BinaryOperator::Add,
            left: ret_left,
            right: ret_right,
        }) = &function.return_expression
        else {
            return Ok(false);
        };
        if !matches!(ret_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::Cast {
            target_type: Type::Int,
            operand,
        } = ret_right.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(operand.as_ref(), Expression::Variable(v) if v == lx) {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(ix_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(ix),
        ) else {
            return Ok(false);
        };
        // -- emit --
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: ix_register,
                immediate: threshold,
            });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, else_label); // blt
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: hx_register,
                clear,
            });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: oris_immediate,
            });
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        // n takes ix's home: subfic r5, r5, K.
        self.bind_label(else_label);
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: ix_register,
                a: ix_register,
                immediate: threshold,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: ix_register,
                immediate: 31,
            });
        let big_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, big_label); // bgt
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: ix_register,
                immediate: 32,
            });
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: hx_register,
            s: hx_register,
            b: ix_register,
        });
        self.output.instructions.push(Instruction::ShiftRightWord {
            a: 0,
            s: lx_register,
            b: 0,
        });
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: lx_register,
            s: lx_register,
            b: ix_register,
        });
        self.output.instructions.push(Instruction::Or {
            a: 0,
            s: hx_register,
            b: 0,
        });
        self.emit_branch_to(join_label);
        self.bind_label(big_label);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: ix_register,
            immediate: -32,
        });
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: 0,
            s: lx_register,
            b: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(lx_register, 0));
        self.bind_label(join_label);
        self.output.instructions.push(Instruction::Add {
            d: 3,
            a: 0,
            b: lx_register,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The PUNNED PAIR LADDER (fire 429/430, e_fmod's |x|<=|y| purge fed
    /// from DOUBLE params): the frame/int marriage. `int f(double x,
    /// double y)` punning hx/lx/hy/ly then the fire-427 ladder.
    /// Measured FRAMED rules (contrast the frameless captures): arms
    /// JOIN at the shared epilogue (`li; b JOIN` — inline blr is a
    /// frameless-only behavior); punned loads emit in first-use order
    /// with ly DELAYED past the cmpw into its branch latency, reusing
    /// dead hx's r0; frame 32 = 8 linkage + 2x8 doubles, spilled at
    /// 8/16(r1); no stmw.
    pub(crate) fn try_punned_pair_ladder(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [p_x, p_y] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_x.parameter_type != Type::Double || p_y.parameter_type != Type::Double {
            return Ok(false);
        }
        let (x, y) = (p_x.name.as_str(), p_y.name.as_str());
        if x == y {
            return Ok(false);
        }
        // The four extracts in e_fmod's order: x-high, x-low, y-high, y-low.
        let [Statement::Assign {
            name: hx,
            value: hx_value,
        }, Statement::Assign {
            name: lx,
            value: lx_value,
        }, Statement::Assign {
            name: hy,
            value: hy_value,
        }, Statement::Assign {
            name: ly,
            value: ly_value,
        }, Statement::If {
            condition: outer,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if crate::frame::pun_word_offset_pub(hx_value, x) != Some(0)
            || crate::frame::pun_word_offset_pub(lx_value, x) != Some(4)
            || crate::frame::pun_word_offset_pub(hy_value, y) != Some(0)
            || crate::frame::pun_word_offset_pub(ly_value, y) != Some(4)
            || !else_body.is_empty()
        {
            return Ok(false);
        }
        let names_distinct = {
            let mut names = [x, y, hx.as_str(), lx.as_str(), hy.as_str(), ly.as_str()];
            names.sort_unstable();
            names.windows(2).all(|pair| pair[0] != pair[1])
        };
        if !names_distinct {
            return Ok(false);
        }
        let typed_local = |name: &str, declared: Type| {
            function.locals.iter().any(|local| {
                local.name == name && local.declared_type == declared && local.initializer.is_none()
            })
        };
        if !typed_local(hx, Type::Int)
            || !typed_local(hy, Type::Int)
            || !typed_local(lx, Type::UnsignedInt)
            || !typed_local(ly, Type::UnsignedInt)
        {
            return Ok(false);
        }
        // The ladder (fire 427's shape over the punned locals).
        let is_pair =
            |expression: &Expression, operator: BinaryOperator, a: &str, b: &str| -> bool {
                let Expression::Binary {
                    operator: found,
                    left,
                    right,
                } = expression
                else {
                    return false;
                };
                *found == operator
                    && matches!(left.as_ref(), Expression::Variable(v) if v == a)
                    && matches!(right.as_ref(), Expression::Variable(v) if v == b)
            };
        if !is_pair(outer, BinaryOperator::LessEqual, hx, hy) {
            return Ok(false);
        }
        let [Statement::If {
            condition: or_test,
            then_body: or_then,
            else_body: or_else,
        }, Statement::If {
            condition: eq_test,
            then_body: eq_then,
            else_body: eq_else,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        if !or_else.is_empty() || !eq_else.is_empty() {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: or_left,
            right: or_right,
        } = or_test
        else {
            return Ok(false);
        };
        if !is_pair(or_left.as_ref(), BinaryOperator::Less, hx, hy)
            || !is_pair(or_right.as_ref(), BinaryOperator::Less, lx, ly)
            || !is_pair(eq_test, BinaryOperator::Equal, lx, ly)
        {
            return Ok(false);
        }
        let arm_return = |statements: &[Statement]| -> Option<i16> {
            let [Statement::Return(Some(Expression::IntegerLiteral(value)))] = statements else {
                return None;
            };
            i16::try_from(*value).ok()
        };
        let (Some(k1), Some(k2)) = (arm_return(or_then), arm_return(eq_then)) else {
            return Ok(false);
        };
        let Some(Expression::IntegerLiteral(k3)) = &function.return_expression else {
            return Ok(false);
        };
        let Ok(k3) = i16::try_from(*k3) else {
            return Ok(false);
        };
        // -- emit (registers per the capture: hx r0, hy r3, lx r4, ly r0) --
        self.frame_size = 32;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 2,
                a: 1,
                offset: 16,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 0, b: 3 });
        // ly's load DELAYS into the compare->branch latency, reusing dead hx's r0.
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        let end_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, end_label); // bgt
        let first_return_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, first_return_label); // blt
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        let equality_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, equality_label); // bge
        let join_label = self.fresh_label();
        self.bind_label(first_return_label);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, k1));
        self.emit_branch_to(join_label);
        self.bind_label(equality_label);
        self.emit_branch_conditional_to(4, 2, end_label); // bne (CR0 reused from the cmplw)
        self.output
            .instructions
            .push(Instruction::load_immediate(3, k2));
        self.emit_branch_to(join_label);
        self.bind_label(end_label);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, k3));
        self.bind_label(join_label);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe — the real extab lands at @12 (mwcc
        // numbers the ladder's internal labels).
        self.output.anonymous_label_bump += 7;
        Ok(true)
    }

    /// The SIGN-INDEXED DOUBLE RETURN (fire 428, e_fmod's Zero[] exit):
    /// `return Zero[(unsigned)sx >> 31];` for a `static double Zero[]`.
    /// Measured: the index `(sx>>31)<<3` FUSES into one rotate-mask
    /// (`rlwinm r0,sx,4,28,28`); the base is a lis/addi ADDR16_HA/LO
    /// pair on the (local) array symbol — .data, NOT sdata, despite the
    /// 16-byte size; the load is `lfdx f1,lo,index`. Register slots per
    /// the capture: ha -> r4, lo -> r3 (sx's home, dead after the
    /// rlwinm), index -> r0.
    pub(crate) fn try_indexed_double_return(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
            || !function.statements.is_empty()
        {
            return Ok(false);
        }
        let [p_sx] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_sx.parameter_type != Type::Int {
            return Ok(false);
        }
        let sx = p_sx.name.as_str();
        let Some(Expression::Index { base, index }) = &function.return_expression else {
            return Ok(false);
        };
        let Expression::Variable(array) = base.as_ref() else {
            return Ok(false);
        };
        if array == sx {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: shifted,
            right: amount,
        } = index.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Cast {
            target_type: Type::UnsignedInt,
            operand,
        } = shifted.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(operand.as_ref(), Expression::Variable(v) if v == sx)
            || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        let Some(sx_register) = self.lookup_general(sx) else {
            return Ok(false);
        };
        if sx_register != 3 {
            return Ok(false);
        }
        let array = array.clone();
        // -- emit --
        self.emit_address_high(4, &array);
        // (sx >> 31) << 3 in one rotate-mask: rotate left 4, keep bit 28.
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: sx_register,
            shift: 4,
            begin: 28,
            end: 28,
        });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::LoadFloatDoubleIndexed { d: 1, a: 3, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The EARLY LADDER (fire 427, e_fmod's |x|<=|y| purge):
    ///   if (hx <= hy) { if ((hx < hy) || (lx < ly)) return K1;
    ///                   if (lx == ly) return K2; }  return K3;
    /// Measured: ONE `cmplw lx,ly` serves BOTH the `||` arm and the
    /// later `==` test — CR0 survives the branch between them (compare
    /// CSE across branches); the `||` short-circuits through `blt` into
    /// the shared return; every return is inline (li; blr — no join).
    pub(crate) fn try_early_ladder(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_hy, p_ly] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_hy.parameter_type != Type::Int
            || p_ly.parameter_type != Type::UnsignedInt
        {
            return Ok(false);
        }
        let (hx, lx, hy, ly) = (
            p_hx.name.as_str(),
            p_lx.name.as_str(),
            p_hy.name.as_str(),
            p_ly.name.as_str(),
        );
        let [Statement::If {
            condition: outer,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let is_pair =
            |expression: &Expression, operator: BinaryOperator, a: &str, b: &str| -> bool {
                let Expression::Binary {
                    operator: found,
                    left,
                    right,
                } = expression
                else {
                    return false;
                };
                *found == operator
                    && matches!(left.as_ref(), Expression::Variable(v) if v == a)
                    && matches!(right.as_ref(), Expression::Variable(v) if v == b)
            };
        if !is_pair(outer, BinaryOperator::LessEqual, hx, hy) {
            return Ok(false);
        }
        let [Statement::If {
            condition: or_test,
            then_body: or_then,
            else_body: or_else,
        }, Statement::If {
            condition: eq_test,
            then_body: eq_then,
            else_body: eq_else,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        if !or_else.is_empty() || !eq_else.is_empty() {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: or_left,
            right: or_right,
        } = or_test
        else {
            return Ok(false);
        };
        if !is_pair(or_left.as_ref(), BinaryOperator::Less, hx, hy)
            || !is_pair(or_right.as_ref(), BinaryOperator::Less, lx, ly)
            || !is_pair(eq_test, BinaryOperator::Equal, lx, ly)
        {
            return Ok(false);
        }
        let arm_return = |statements: &[Statement]| -> Option<i16> {
            let [Statement::Return(Some(Expression::IntegerLiteral(value)))] = statements else {
                return None;
            };
            i16::try_from(*value).ok()
        };
        let (Some(k1), Some(k2)) = (arm_return(or_then), arm_return(eq_then)) else {
            return Ok(false);
        };
        let Some(Expression::IntegerLiteral(k3)) = &function.return_expression else {
            return Ok(false);
        };
        let Ok(k3) = i16::try_from(*k3) else {
            return Ok(false);
        };
        let (Some(hx_register), Some(lx_register), Some(hy_register), Some(ly_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(hy),
            self.lookup_general(ly),
        ) else {
            return Ok(false);
        };
        // -- emit --
        self.output.instructions.push(Instruction::CompareWord {
            a: hx_register,
            b: hy_register,
        });
        let end_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, end_label); // bgt
        let first_return_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, first_return_label); // blt (the || short-circuit)
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord {
                a: lx_register,
                b: ly_register,
            });
        let equality_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, equality_label); // bge
        self.bind_label(first_return_label);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, k1));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // The equality test REUSES the cmplw's CR0 (no second compare).
        self.bind_label(equality_label);
        self.emit_branch_conditional_to(4, 2, end_label); // bne
        self.output
            .instructions
            .push(Instruction::load_immediate(3, k2));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(end_label);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, k3));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ILOGB DIAMOND (fire 426, e_fmod's exponent extract): rotated
    /// loops NEST INTO IF-ARMS by concatenation with per-arm register
    /// context —
    ///   if (hx < BIG) { if (hx == 0) FOR-LOOP(lx) else FOR-LOOP(hx<<A) }
    ///   else ix = (hx >> 20) - K;  return ix;
    /// Measured: ix lands DIRECTLY in r3 in every arm (hx is dead inside
    /// them, killing the standalone loop's trailing mr); each arm ends
    /// with its own inline blr (no join); r0 double-duties (the lis
    /// bound dies at the cmpw, arm 2 reuses r0 for its shift temp); the
    /// arm-2 shift init emits BEFORE the li that overwrites hx's home.
    pub(crate) fn try_ilogb_diamond(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [p_hx, p_lx] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int || p_lx.parameter_type != Type::UnsignedInt {
            return Ok(false);
        }
        let (hx, lx) = (p_hx.name.as_str(), p_lx.name.as_str());
        let [Statement::If {
            condition: outer_test,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // Outer: hx < BIG (lis-only constant).
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left: outer_left,
            right: outer_right,
        } = outer_test
        else {
            return Ok(false);
        };
        if !matches!(outer_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(bound) = outer_right.as_ref() else {
            return Ok(false);
        };
        if *bound & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(bound_high) = i16::try_from(*bound >> 16) else {
            return Ok(false);
        };
        // Inner diamond: if (hx == 0) loop-over-lx else loop-over-(hx<<A).
        let [Statement::If {
            condition: inner_test,
            then_body: zero_arm,
            else_body: shift_arm,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Equal,
            left: inner_left,
            right: inner_right,
        } = inner_test
        else {
            return Ok(false);
        };
        if !matches!(inner_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(inner_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        // An arm loop: for (ix = K, i = SRC; i > 0; i <<= 1) ix -= 1;
        enum ArmSource {
            InPlaceLow,
            ShiftOfHigh(u8),
        }
        struct ArmLoop {
            start: i16,
            source: ArmSource,
        }
        let mut result_local: Option<String> = None;
        let mut counter_local: Option<String> = None;
        let mut parse_arm = |statements: &[Statement]| -> Option<ArmLoop> {
            let [Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(init),
                condition: Some(cond),
                step: Some(step),
                body,
            }] = statements
            else {
                return None;
            };
            // The comma init: (ix = K, i = SRC).
            let Expression::Comma {
                left: first,
                right: second,
            } = init
            else {
                return None;
            };
            let Expression::Assign {
                target: ix_target,
                value: ix_value,
            } = first.as_ref()
            else {
                return None;
            };
            let Expression::Variable(ix_name) = ix_target.as_ref() else {
                return None;
            };
            let Expression::IntegerLiteral(start) = ix_value.as_ref() else {
                return None;
            };
            let start = i16::try_from(*start).ok()?;
            let Expression::Assign {
                target: i_target,
                value: i_value,
            } = second.as_ref()
            else {
                return None;
            };
            let Expression::Variable(i_name) = i_target.as_ref() else {
                return None;
            };
            let source = match i_value.as_ref() {
                Expression::Variable(v) if v == lx => ArmSource::InPlaceLow,
                Expression::Binary {
                    operator: BinaryOperator::ShiftLeft,
                    left,
                    right,
                } => {
                    if !matches!(left.as_ref(), Expression::Variable(v) if v == hx) {
                        return None;
                    }
                    let Expression::IntegerLiteral(amount) = right.as_ref() else {
                        return None;
                    };
                    ArmSource::ShiftOfHigh(
                        u8::try_from(*amount)
                            .ok()
                            .filter(|a| (1..=31).contains(a))?,
                    )
                }
                _ => return None,
            };
            // Locals consistent across arms; distinct from the params.
            if ix_name == hx || ix_name == lx || i_name == hx || i_name == lx || ix_name == i_name {
                return None;
            }
            match (&result_local, &counter_local) {
                (None, None) => {
                    result_local = Some(ix_name.clone());
                    counter_local = Some(i_name.clone());
                }
                (Some(result), Some(counter)) => {
                    if result != ix_name || counter != i_name {
                        return None;
                    }
                }
                _ => return None,
            }
            // Condition: i > 0. Step: i <<= 1. Body: ix -= 1.
            let Expression::Binary {
                operator: BinaryOperator::Greater,
                left: cond_left,
                right: cond_right,
            } = cond
            else {
                return None;
            };
            if !matches!(cond_left.as_ref(), Expression::Variable(v) if v == i_name)
                || !matches!(cond_right.as_ref(), Expression::IntegerLiteral(0))
            {
                return None;
            }
            let Expression::Assign {
                target: step_target,
                value: step_value,
            } = step
            else {
                return None;
            };
            let Expression::Binary {
                operator: BinaryOperator::ShiftLeft,
                left: step_left,
                right: step_right,
            } = step_value.as_ref()
            else {
                return None;
            };
            if !matches!(step_target.as_ref(), Expression::Variable(v) if v == i_name)
                || !matches!(step_left.as_ref(), Expression::Variable(v) if v == i_name)
                || !matches!(step_right.as_ref(), Expression::IntegerLiteral(1))
            {
                return None;
            }
            let [Statement::Assign {
                name: body_name,
                value: body_value,
            }] = body.as_slice()
            else {
                return None;
            };
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: body_left,
                right: body_right,
            } = body_value
            else {
                return None;
            };
            if body_name != ix_name
                || !matches!(body_left.as_ref(), Expression::Variable(v) if v == ix_name)
                || !matches!(body_right.as_ref(), Expression::IntegerLiteral(1))
            {
                return None;
            }
            Some(ArmLoop { start, source })
        };
        let Some(zero_loop) = parse_arm(zero_arm) else {
            return Ok(false);
        };
        let Some(shift_loop) = parse_arm(shift_arm) else {
            return Ok(false);
        };
        if !matches!(zero_loop.source, ArmSource::InPlaceLow)
            || !matches!(shift_loop.source, ArmSource::ShiftOfHigh(_))
        {
            return Ok(false);
        }
        // The else arm: ix = (hx >> S) - K.
        let [Statement::Assign {
            name: else_name,
            value: else_value,
        }] = else_body.as_slice()
        else {
            return Ok(false);
        };
        if Some(else_name.as_str()) != result_local.as_deref() {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: shifted,
            right: offset,
        } = else_value
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: shift_source,
            right: shift_amount,
        } = shifted.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(shift_source.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(else_shift) = shift_amount.as_ref() else {
            return Ok(false);
        };
        let Ok(else_shift) = u8::try_from(*else_shift) else {
            return Ok(false);
        };
        if !(1..=31).contains(&else_shift) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(else_offset) = offset.as_ref() else {
            return Ok(false);
        };
        let Ok(negated_offset) = i16::try_from(-*else_offset) else {
            return Ok(false);
        };
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if Some(v.as_str()) == result_local.as_deref())
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register)) =
            (self.lookup_general(hx), self.lookup_general(lx))
        else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // -- emit --
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: bound_high,
            });
        self.output.instructions.push(Instruction::CompareWord {
            a: hx_register,
            b: 0,
        });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: hx_register,
                immediate: 0,
            });
        let shift_arm_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, shift_arm_label); // bne
                                                                // Arm 1: the loop over lx, ix in r3, counter in lx's home.
        self.output
            .instructions
            .push(Instruction::load_immediate(3, zero_loop.start));
        let test1 = self.fresh_label();
        self.emit_branch_to(test1);
        let body1 = self.fresh_label();
        self.bind_label(body1);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: lx_register,
                s: lx_register,
                shift: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.bind_label(test1);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: lx_register,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 1, body1); // bgt
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // Arm 2: the loop over hx<<A; the shift temp rides r0 (the bound
        // is dead), and its init emits BEFORE the li overwrites r3.
        self.bind_label(shift_arm_label);
        let ArmSource::ShiftOfHigh(amount) = shift_loop.source else {
            return Ok(false);
        };
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: hx_register,
                shift: amount,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, shift_loop.start));
        let test2 = self.fresh_label();
        self.emit_branch_to(test2);
        let body2 = self.fresh_label();
        self.bind_label(body2);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 1,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: -1,
        });
        self.bind_label(test2);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, body2); // bgt
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // The else arm: srawi + addi in place.
        self.bind_label(else_label);
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate {
                a: 3,
                s: hx_register,
                shift: else_shift,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: negated_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }
}
