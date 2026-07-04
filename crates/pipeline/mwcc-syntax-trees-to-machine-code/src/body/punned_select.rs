//! Pointer-punned selects, hoisted overwrites, and the modf ladder.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A whole-body `if (c) { <store run> } else { <store run> }` where each arm is two-plus stores
    /// whose values are either all REGISTER-valued (emitted sequentially) or all CONSTANT (the
    /// batched materialization): `cmpwi; beq else; <then run>; blr; else: <else run>; blr`. The
    /// no-else form is handled by try_constant_store_fill / the register-valued trailing-if path.
    /// THE PUNNED-GUARD WRITEBACK (the s_floor tail, fire 380): punned int
    /// reads of the double param spill it to the frame, a guard block
    /// mutates the punned locals in scratch registers, the block writes
    /// them back and the double reloads. Measured (one and two locals):
    /// stwu; cmpwi (HOISTED — the second local reuses the freed condition
    /// register); stfd f1,8; lwz r0[, lwz r3]; beq JOIN; li...; JOIN:
    /// stw...; lfd f1,8; addi; blr.
    /// The BRANCHLESS ZERO-SELECT: `if (j0 cmp K) p = A; else p = B;` with
    /// one arm 0 if-converts to mask algebra — no branches (measured
    /// L3/L4/S2/S3/R1/R2/R3 on 2.6). The mask is -(cond); zero-in-then
    /// selects with andc (else & ~mask), zero-in-else with and. Recipes:
    ///   <  : li rK; srwi sign(K); subfc K,g; srwi sign(g); subfe
    ///   >  : the swapped form (rK/sign registers trade places)
    ///   == : addi g-K; subfic K-g; nor; srawi 31
    ///   != : the same with or
    ///   <= : xoris 0x8000; subfic; addc; subfe rM,rM,rM
    /// Registers: the select home is r0; </> put K,sign in r3/r4 and the
    /// load in r5; ==/!=/<= compute in place on the r3 load. The L4
    /// self-mask arm (`p &= M`) keeps the load in r0 and weaves rlwinm
    /// between the guard extract and its -K addi.
    pub(crate) fn try_punned_zero_select(
        &mut self,
        locals: &[(&str, i16)],
        guard: &GuardLocal,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
    ) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        let punned = locals[0].0;
        let offset = locals[0].1;
        let Expression::Binary { operator, left, right } = condition else {
            return Ok(false);
        };
        let operator = *operator;
        if !matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::Greater
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessEqual
        ) {
            return Ok(false);
        }
        if !matches!(left.as_ref(), Expression::Variable(name) if name == guard.name) {
            return Ok(false);
        }
        let Some(bound) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let Ok(bound) = i16::try_from(bound) else {
            return Ok(false);
        };
        let ([Statement::Assign { name: then_name, value: then_value }], [Statement::Assign { name: else_name, value: else_value }]) =
            (then_body, else_body)
        else {
            return Ok(false);
        };
        if then_name != punned || else_name != punned {
            return Ok(false);
        }
        let then_zero = crate::analysis::constant_value(then_value) == Some(0);
        let else_zero = crate::analysis::constant_value(else_value) == Some(0);
        let (live_value, select_complement) = match (then_zero, else_zero) {
            (true, false) => (else_value, true),  // else & ~mask
            (false, true) => (then_value, false), // then & mask
            _ => return Ok(false),
        };
        // The live arm: a small constant, or the measured L4 self-mask
        // (`p & M`, only captured under `<` with the zero in the then).
        enum LiveArm {
            Constant(i16),
            SelfMask { begin: u8, end: u8 },
        }
        let live_arm = if let Some(constant) = crate::analysis::constant_value(live_value) {
            let Ok(small) = i16::try_from(constant) else {
                return Ok(false);
            };
            LiveArm::Constant(small)
        } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = live_value {
            if !(operator == BinaryOperator::Less && select_complement) {
                return Ok(false);
            }
            if !matches!(left.as_ref(), Expression::Variable(name) if name == punned) {
                return Ok(false);
            }
            let Some((begin, end)) =
                crate::analysis::constant_value(right).and_then(crate::analysis::rlwinm_mask)
            else {
                return Ok(false);
            };
            LiveArm::SelfMask { begin, end }
        } else {
            return Ok(false);
        };
        // The guard is read by the condition alone; the arms touch only p.
        if count_name_occurrences(condition, guard.name) != 1
            || count_name_occurrences(then_value, guard.name) != 0
            || count_name_occurrences(else_value, guard.name) != 0
        {
            return Ok(false);
        }
        let offset_negative = if guard.offset_k != 0 {
            let Ok(negative) = i16::try_from(-guard.offset_k) else {
                return Ok(false);
            };
            Some(negative)
        } else {
            None
        };
        // -- commit --
        let self_mask_arm = matches!(live_arm, LiveArm::SelfMask { .. });
        let carry_form = matches!(operator, BinaryOperator::Less | BinaryOperator::Greater);
        // Homes: the select value in r0; </> claim r3/r4 for K and its
        // sign; the load lands beyond them (r5) or shares r0 (self-mask).
        let load_register: u8 = if self_mask_arm {
            0
        } else if carry_form {
            5
        } else {
            3
        };
        let guard_register: u8 = if carry_form { 5 } else { 3 };
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        match operator {
            BinaryOperator::Less => {
                self.output.instructions.push(Instruction::load_immediate(3, bound));
                self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 3, shift: 1, begin: 31, end: 31 });
            }
            BinaryOperator::Greater => {
                self.output.instructions.push(Instruction::load_immediate(4, bound));
                self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 1, begin: 31, end: 31 });
            }
            _ => {}
        }
        if let LiveArm::Constant(constant) = live_arm {
            self.output.instructions.push(Instruction::load_immediate(0, constant));
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: load_register, a: 1, offset: 8 + offset });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: load_register,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: load_register,
                    shift: guard.shift,
                });
            }
        }
        if let LiveArm::SelfMask { begin, end } = live_arm {
            // The arm rlwinm weaves between the guard extract and its addi
            // (measured L4).
            self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin, end });
        }
        if let Some(negative) = offset_negative {
            self.output.instructions.push(Instruction::AddImmediate {
                d: guard_register,
                a: guard_register,
                immediate: negative,
            });
        }
        let g = guard_register;
        match operator {
            BinaryOperator::Less => {
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: 3, a: 3, b: g });
                self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: g, shift: 1, begin: 31, end: 31 });
                self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 4 });
            }
            BinaryOperator::Greater => {
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: 4, a: g, b: 4 });
                self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: g, shift: 1, begin: 31, end: 31 });
                self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 4 });
            }
            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                let Ok(negated) = i16::try_from(-(bound as i32)) else {
                    return Err(Diagnostic::error("select bound beyond i16 (roadmap)"));
                };
                self.output.instructions.push(Instruction::AddImmediate { d: 4, a: g, immediate: negated });
                self.output.instructions.push(Instruction::SubtractFromImmediate { d: 3, a: g, immediate: bound });
                if operator == BinaryOperator::Equal {
                    self.output.instructions.push(Instruction::Nor { a: 3, s: 4, b: 3 });
                } else {
                    self.output.instructions.push(Instruction::Or { a: 3, s: 4, b: 3 });
                }
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 3, shift: 31 });
            }
            BinaryOperator::LessEqual => {
                self.output.instructions.push(Instruction::XorImmediateShifted { a: 4, s: g, immediate: 0x8000 });
                self.output.instructions.push(Instruction::SubtractFromImmediate { d: 3, a: g, immediate: bound });
                self.output.instructions.push(Instruction::AddCarrying { d: 3, a: 3, b: 4 });
                self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 3 });
            }
            _ => unreachable!("gated above"),
        }
        if select_complement {
            self.output.instructions.push(Instruction::AndComplement { a: 0, s: 0, b: 3 });
        } else {
            self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 3 });
        }
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 + offset });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The if-converted diamond still costs its labels (measured +3:
        // real @8/@9 vs the +0 base's @5/@6 on the L3 object); the
        // compound self-mask arm adds one more (L4's @9/@10).
        self.output.anonymous_label_bump += if self_mask_arm { 4 } else { 3 };
        Ok(true)
    }

    /// The HOISTED-ELSE OVERWRITE: `if (j0 cmp K) p = C1; else p = C2;`
    /// with BOTH arms nonzero constants branches (no if-conversion) with
    /// the else value pre-loaded into the home before the compare and the
    /// then arm as a skip (measured H1–H7, all six comparison ops):
    ///   li rHome,C2; stfd; lwz r0; extract; [addi r0,-K0]; cmpwi r0;
    ///   b<inverted> skip; li rHome,C1; skip: stw rHome
    /// Homes obey the LIVENESS rule: the pre-loaded else value crosses the
    /// r0 write, so rHome = r4 when the guard holds a home (K0 fold) and
    /// r3 when the extract goes straight to r0 (K0 = 0, H7).
    pub(crate) fn try_punned_hoisted_overwrite(
        &mut self,
        locals: &[(&str, i16)],
        guard: &GuardLocal,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
    ) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        let punned = locals[0].0;
        let offset = locals[0].1;
        let Expression::Binary { operator, left, right } = condition else {
            return Ok(false);
        };
        // The inverted skip branch, (options, condition_bit) per op.
        let inverted = match operator {
            BinaryOperator::Less => (4, 0),          // bge
            BinaryOperator::Greater => (4, 1),       // ble
            BinaryOperator::Equal => (4, 2),         // bne
            BinaryOperator::NotEqual => (12, 2),     // beq
            BinaryOperator::LessEqual => (12, 1),    // bgt
            BinaryOperator::GreaterEqual => (12, 0), // blt
            _ => return Ok(false),
        };
        if !matches!(left.as_ref(), Expression::Variable(name) if name == guard.name) {
            return Ok(false);
        }
        let Some(bound) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let Ok(bound) = i16::try_from(bound) else {
            return Ok(false);
        };
        let ([Statement::Assign { name: then_name, value: then_value }], [Statement::Assign { name: else_name, value: else_value }]) =
            (then_body, else_body)
        else {
            return Ok(false);
        };
        if then_name != punned || else_name != punned {
            return Ok(false);
        }
        let (Some(then_constant), Some(else_constant)) = (
            crate::analysis::constant_value(then_value),
            crate::analysis::constant_value(else_value),
        ) else {
            return Ok(false);
        };
        let (Ok(then_constant), Ok(else_constant)) =
            (i16::try_from(then_constant), i16::try_from(else_constant))
        else {
            return Ok(false);
        };
        if then_constant == 0 || else_constant == 0 {
            // One-zero forms if-convert (the zero-select path claims them
            // first); both-zero is unmeasured.
            return Ok(false);
        }
        if count_name_occurrences(condition, guard.name) != 1 {
            return Ok(false);
        }
        let offset_negative = if guard.offset_k != 0 {
            let Ok(negative) = i16::try_from(-guard.offset_k) else {
                return Ok(false);
            };
            Some(negative)
        } else {
            None
        };
        // -- commit --
        // With the -K0 fold the guard needs a home (r3) and the else value
        // lands beyond it (r4); without it the extract computes in place
        // on r0 and the home is r3 (measured H7).
        let home: u8 = if offset_negative.is_some() { 4 } else { 3 };
        let guard_register: u8 = if offset_negative.is_some() { 3 } else { 0 };
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::load_immediate(home, else_constant));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 + offset });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: 0,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: 0,
                    shift: guard.shift,
                });
            }
        }
        if let Some(negative) = offset_negative {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: guard_register,
                immediate: negative,
            });
        }
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: bound });
        let skip = self.fresh_label();
        self.emit_branch_conditional_to(inverted.0, inverted.1, skip);
        self.output.instructions.push(Instruction::load_immediate(home, then_constant));
        self.bind_label(skip);
        self.output.instructions.push(Instruction::StoreWord { s: home, a: 1, offset: 8 + offset });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The diamond's labels (measured +3 on the H1 object: real @8/@9
        // vs the +0 base's @5/@6 — the same count as the if-converted
        // select, so the label cost predates the conversion decision).
        self.output.anonymous_label_bump += 3;
        Ok(true)
    }

    /// THE MODF LADDER (fire 405): s_modf's three-arm shape — pointer
    /// stores through the second param, the INTEGRAL block (sign-only pun
    /// store into x's spill + f1 reload), and `x - *iptr` (lfd + fsub).
    /// Registers per the capture with r3 = the live pointer param: temp
    /// r4, loads r5/r6, the scrutinee j0 r7; the integral block reuses
    /// the (path-dead) param register r3 as its scratch.
    pub(crate) fn try_punned_modf_ladder(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [x_param, pointer_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double
            || pointer_param.parameter_type != Type::Pointer(Pointee::Double)
        {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        let iptr = pointer_param.name.as_str();
        // Locals: initialized punned pair + guard; the uninitialized shift.
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        for local in &function.locals {
            if local.array_length.is_some() {
                return Ok(false);
            }
            match (&local.initializer, local.declared_type) {
                (Some(init), Type::Int) => {
                    if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                        if locals.iter().any(|&(_, seen)| seen == offset) {
                            return Ok(false);
                        }
                        locals.push((local.name.as_str(), offset));
                    } else if guard.is_none() {
                        let Some(parsed) = parse_guard_init(local.name.as_str(), init) else {
                            return Ok(false);
                        };
                        guard = Some(parsed);
                    } else {
                        return Ok(false);
                    }
                }
                (None, Type::UnsignedInt) if shift.is_none() => shift = Some(local.name.as_str()),
                _ => return Ok(false),
            }
        }
        let (Some(guard), Some(shift)) = (guard, shift) else {
            return Ok(false);
        };
        if locals.len() != 2
            || locals[0].1 != 0
            || locals[1].1 != 4
            || !locals.iter().any(|&(name, _)| name == guard.source)
            || guard.offset_k == 0
            || i16::try_from(-guard.offset_k).is_err()
        {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let i0 = local_index(guard.source).expect("checked");
        if i0 != 0 {
            return Ok(false); // the high word drives everything
        }
        // The single statement: the outer ladder (every leaf returns).
        let [Statement::If { condition: ladder1, then_body: low_arm, else_body: high_arm }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let parse_guard_compare = |condition: &Expression, operator: BinaryOperator| -> Option<i16> {
            let Expression::Binary { operator: op, left, right } = condition else { return None };
            if *op != operator || !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name) {
                return None;
            }
            crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
        };
        let Some(k1) = parse_guard_compare(ladder1, BinaryOperator::Less) else {
            return Ok(false);
        };
        let [Statement::If { condition: split, then_body: arm1, else_body: arm2 }] = low_arm.as_slice()
        else {
            return Ok(false);
        };
        if parse_guard_compare(split, BinaryOperator::Less) != Some(0) {
            return Ok(false);
        }
        let [Statement::If { condition: ladder2, then_body: mid, else_body: arm3 }] = high_arm.as_slice()
        else {
            return Ok(false);
        };
        let Some(k2) = parse_guard_compare(ladder2, BinaryOperator::Greater) else {
            return Ok(false);
        };
        // The INTEGRAL block: [*iptr = x[*one], *(int*)&x &= SIGN,
        // *(1+(int*)&x) = 0, Return(x)] — the x*one fold makes the first
        // store a plain stfd (measured: no fmul).
        let is_integral = |body: &[Statement]| -> bool {
            let [Statement::Store { target: tp, value: vp }, Statement::Store { target: t0, value: v0 }, Statement::Store { target: t1, value: v1 }, Statement::Return(Some(Expression::Variable(rx)))] =
                body
            else {
                return false;
            };
            let pointer_store_ok =
                matches!(tp, Expression::Dereference { pointer }
                    if matches!(pointer.as_ref(), Expression::Variable(v) if v == iptr));
            let value_is_x = matches!(vp, Expression::Variable(v) if v == x)
                || matches!(vp, Expression::Binary { operator: BinaryOperator::Multiply, left, right }
                    if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                        && matches!(right.as_ref(), Expression::FloatLiteral(one) if *one == 1.0));
            rx == x
                && pointer_store_ok
                && value_is_x
                && crate::frame::pun_word_offset_pub(t0, x) == Some(0)
                && crate::frame::pun_word_offset_pub(t1, x) == Some(4)
                && crate::analysis::constant_value(v1) == Some(0)
                && matches!(v0, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                    if crate::analysis::constant_value(right).map(|c| c as u32) == Some(0x8000_0000)
                        && (crate::frame::pun_word_offset_pub(left, x) == Some(0)
                            || matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(0))))
        };
        // A pointer-store pair + fraction return:
        //   [*(int*)iptr = HIGH, *(1+(int*)iptr) = LOW, Return(x - *iptr)]
        // HIGH: i0 & SIGN (arm1) / i0 & ~i (arm2) / i0 (arm3);
        // LOW: 0 (arm1/arm2) / i1 & ~i (arm3); arm1 returns plain x.
        enum HighForm {
            SignOnly,
            AndcShift,
            Plain,
        }
        enum LowForm {
            Zero,
            AndcShift,
        }
        let parse_pointer_arm = |body: &[Statement], fraction: bool| -> Option<(HighForm, LowForm)> {
            let [Statement::Store { target: t0, value: v0 }, Statement::Store { target: t1, value: v1 }, Statement::Return(Some(ret))] =
                body
            else {
                return None;
            };
            if pointer_word_offset(t0, iptr) != Some(0) || pointer_word_offset(t1, iptr) != Some(4) {
                return None;
            }
            if fraction {
                let ok = matches!(ret, Expression::Binary { operator: BinaryOperator::Subtract, left, right }
                    if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                        && matches!(right.as_ref(), Expression::Dereference { pointer }
                            if matches!(pointer.as_ref(), Expression::Variable(v) if v == iptr)));
                if !ok {
                    return None;
                }
            } else if !matches!(ret, Expression::Variable(v) if v == x) {
                return None;
            }
            let high = if matches!(v0, Expression::Variable(v) if local_index(v) == Some(0)) {
                HighForm::Plain
            } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = v0 {
                if !matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(0)) {
                    return None;
                }
                match right.as_ref() {
                    Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift) =>
                    {
                        HighForm::AndcShift
                    }
                    other if crate::analysis::constant_value(other).map(|c| c as u32)
                        == Some(0x8000_0000) =>
                    {
                        HighForm::SignOnly
                    }
                    _ => return None,
                }
            } else {
                return None;
            };
            let low = if crate::analysis::constant_value(v1) == Some(0) {
                LowForm::Zero
            } else if matches!(v1, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(1))
                    && matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift)))
            {
                LowForm::AndcShift
            } else {
                return None;
            };
            Some((high, low))
        };
        // arm1: the sign-only pointer pair, plain return.
        if !matches!(parse_pointer_arm(arm1, false), Some((HighForm::SignOnly, LowForm::Zero))) {
            return Ok(false);
        }
        // arm2: [i = C >> j0, If{((i0&i)|i1)==0, integral, pointer-frac}].
        let [Statement::Assign { name: a2_shift, value: a2_value }, Statement::If { condition: a2_test, then_body: a2_int, else_body: a2_frac }] =
            arm2.as_slice()
        else {
            return Ok(false);
        };
        if a2_shift != shift {
            return Ok(false);
        }
        let Some((a2_mask, a2_logical, a2_off)) = parse_shift_init(a2_value, guard.name) else {
            return Ok(false);
        };
        if a2_logical || a2_off != 0 || i16::try_from(a2_mask).is_ok() {
            return Ok(false);
        }
        let a2_test_ok = matches!(a2_test, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if crate::analysis::constant_value(right) == Some(0)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::BitOr, left: ol, right: or }
                    if matches!(or.as_ref(), Expression::Variable(v) if local_index(v) == Some(1))
                        && matches!(ol.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                            if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(0))
                                && matches!(ar.as_ref(), Expression::Variable(v) if v == shift))));
        if !a2_test_ok
            || !is_integral(a2_int)
            || !matches!(parse_pointer_arm(a2_frac, true), Some((HighForm::AndcShift, LowForm::Zero)))
        {
            return Ok(false);
        }
        // mid: the integral block.
        if !is_integral(mid) {
            return Ok(false);
        }
        // arm3: [i = (unsigned)C >> (j0-K), If{(i1&i)==0, integral, pointer-frac}].
        let [Statement::Assign { name: a3_shift, value: a3_value }, Statement::If { condition: a3_test, then_body: a3_int, else_body: a3_frac }] =
            arm3.as_slice()
        else {
            return Ok(false);
        };
        if a3_shift != shift {
            return Ok(false);
        }
        let Some((a3_mask, a3_logical, a3_off)) = parse_shift_init(a3_value, guard.name) else {
            return Ok(false);
        };
        let a3_mask = a3_mask as u32 as i32 as i64;
        let (Ok(a3_mask_small), Ok(a3_off_neg)) = (i16::try_from(a3_mask), i16::try_from(-a3_off))
        else {
            return Ok(false);
        };
        if !a3_logical || a3_off == 0 {
            return Ok(false);
        }
        let a3_test_ok = matches!(a3_test, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if crate::analysis::constant_value(right) == Some(0)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                    if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(1))
                        && matches!(ar.as_ref(), Expression::Variable(v) if v == shift)));
        if !a3_test_ok
            || !is_integral(a3_int)
            || !matches!(parse_pointer_arm(a3_frac, true), Some((HighForm::Plain, LowForm::AndcShift)))
        {
            return Ok(false);
        }
        // -- emit (registers per the capture; r3 = the live pointer param) --
        let (i0_reg, i1_reg, j0_reg, temp) = (5u8, 6u8, 7u8, 4u8);
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: i0_reg, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: i1_reg, a: 1, offset: 12 });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: temp,
                    s: i0_reg,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: temp,
                    s: i0_reg,
                    shift: guard.shift,
                });
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: j0_reg,
            a: temp,
            immediate: i16::try_from(-guard.offset_k).expect("validated"),
        });
        let epilogue = self.fresh_label();
        let ladder2_at = self.fresh_label();
        let arm2_at = self.fresh_label();
        let arm3_at = self.fresh_label();
        // The integral block: `*iptr = x` + the sign-only pun store + the
        // f1 reload — the stfd through the pointer schedules AFTER the pun
        // stores (measured), and the scratch is the temp r4 (the pointer
        // stays live here).
        let integral = |generator: &mut Self| {
            generator.output.instructions.push(Instruction::RotateAndMask {
                a: temp,
                s: i0_reg,
                shift: 0,
                begin: 0,
                end: 0,
            });
            generator.output.instructions.push(Instruction::load_immediate(0, 0));
            generator.output.instructions.push(Instruction::StoreWord { s: temp, a: 1, offset: 8 });
            generator.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
            generator.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 3, offset: 0 });
            generator.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        };
        let fraction = |generator: &mut Self| {
            generator.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
            generator.output.instructions.push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 0 });
        };
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k1 });
        self.emit_branch_conditional_to(4, 0, ladder2_at); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, arm2_at); // bge
        // arm1: the sign pair through the pointer.
        self.output.instructions.push(Instruction::RotateAndMask { a: temp, s: i0_reg, shift: 0, begin: 0, end: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: temp, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        self.emit_branch_to(epilogue);
        // arm2.
        self.bind_label(arm2_at);
        let a2_lis = ((a2_mask + 0x8000) >> 16) << 16;
        self.output.instructions.push(Instruction::load_immediate_shifted(temp, (a2_lis >> 16) as i16));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: temp, immediate: a2_mask as i16 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: temp, s: 0, b: j0_reg });
        self.output.instructions.push(Instruction::And { a: 0, s: i0_reg, b: temp });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: i1_reg, b: 0 });
        let a2_frac_at = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a2_frac_at); // bne
        integral(self);
        self.emit_branch_to(epilogue);
        self.bind_label(a2_frac_at);
        self.output.instructions.push(Instruction::AndComplement { a: temp, s: i0_reg, b: temp });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: temp, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        fraction(self);
        self.emit_branch_to(epilogue);
        // ladder 2 + mid.
        self.bind_label(ladder2_at);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k2 });
        self.emit_branch_conditional_to(4, 1, arm3_at); // ble
        integral(self);
        self.emit_branch_to(epilogue);
        // arm3.
        self.bind_label(arm3_at);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: j0_reg, immediate: a3_off_neg });
        self.output.instructions.push(Instruction::load_immediate(temp, a3_mask_small));
        self.output.instructions.push(Instruction::ShiftRightWord { a: temp, s: temp, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: i1_reg, b: temp });
        let a3_frac_at = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a3_frac_at); // bne
        integral(self);
        self.emit_branch_to(epilogue);
        self.bind_label(a3_frac_at);
        self.output.instructions.push(Instruction::StoreWord { s: i0_reg, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::AndComplement { a: 0, s: i1_reg, b: temp });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        fraction(self);
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @24 on the s_modf object vs the +0
        // base's @5).
        self.output.anonymous_label_bump += 19;
        Ok(true)
    }

}
