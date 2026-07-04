//! Pointer-punned writeback ladders (shift/guard variants).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// THE COMPOSER (fire 403): the full three-arm s_floor ladder —
    /// `if (j0<K1) { if (j0<0) ARM1 else ARM2 } else if (j0>K2) MID else
    /// ARM3` + writebacks. Arms are the standalone byte-exact templates
    /// with in-arm constants; registers come from int_alloc v3 with j0 as
    /// the SCRUTINEE (assigned last — r7 in the capture) and the arm
    /// shifts as ARM-DEFINED (they join the death-asc pool at r4). One
    /// JOIN (the stores) and one EPI serve every arm.
    pub(crate) fn try_punned_ladder_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Locals: initialized punned pair + guard; uninitialized unsigned
        // shift + carry (the normalizer folds only the leading assigns).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        let mut carry: Option<&str> = None;
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
                (None, Type::UnsignedInt) if carry.is_none() => carry = Some(local.name.as_str()),
                _ => return Ok(false),
            }
        }
        let (Some(guard), Some(shift), Some(carry)) = (guard, shift, carry) else {
            return Ok(false);
        };
        if locals.len() != 2
            || !locals.iter().any(|&(name, _)| name == guard.source)
            || guard.offset_k == 0
            || i16::try_from(-guard.offset_k).is_err()
        {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let i0 = local_index(guard.source).expect("checked");
        let i1 = 1 - i0;
        // The outer ladder + stores.
        let [Statement::If { condition: ladder1, then_body: low_arm, else_body: high_arm }, store_statements @ ..] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if store_statements.len() != 2 {
            return Ok(false);
        }
        for (statement, &(name, offset)) in store_statements.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset)
                || !matches!(value, Expression::Variable(read) if read == name)
            {
                return Ok(false);
            }
        }
        // ladder1: j0 < K1.
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
        // low_arm = [If{j0<0, arm1, arm2}].
        let [Statement::If { condition: split, then_body: arm1, else_body: arm2 }] = low_arm.as_slice()
        else {
            return Ok(false);
        };
        if parse_guard_compare(split, BinaryOperator::Less) != Some(0) {
            return Ok(false);
        }
        // high_arm = [If{j0>K2, mid, arm3}].
        let [Statement::If { condition: ladder2, then_body: mid, else_body: arm3 }] = high_arm.as_slice()
        else {
            return Ok(false);
        };
        let Some(k2) = parse_guard_compare(ladder2, BinaryOperator::Greater) else {
            return Ok(false);
        };
        // mid = [If{j0==K3, [Return x+x], [Return x]}].
        let [Statement::If { condition: mid_cond, then_body: mid_then, else_body: mid_else }] =
            mid.as_slice()
        else {
            return Ok(false);
        };
        let Some(k3) = parse_guard_compare(mid_cond, BinaryOperator::Equal) else {
            return Ok(false);
        };
        let mid_ok = matches!(mid_then.as_slice(),
                [Statement::Return(Some(Expression::Binary { operator: BinaryOperator::Add, left, right }))]
                    if matches!((left.as_ref(), right.as_ref()),
                        (Expression::Variable(a), Expression::Variable(b)) if a == x && b == x))
            && matches!(mid_else.as_slice(),
                [Statement::Return(Some(Expression::Variable(v)))] if v == x);
        if !mid_ok {
            return Ok(false);
        }
        // ARM1 (G3): If{huge+x>0, [If{i0>=0, [i0=i1=0], [If{((i0&M)|i1)!=0, [i0=HIGH, i1=0]}]}]}.
        let [Statement::If { condition: guard1_cond, then_body: guard1_body, else_body: guard1_else }] =
            arm1.as_slice()
        else {
            return Ok(false);
        };
        let Some((huge_bits, zero_bits)) = float_guard_condition(guard1_cond) else {
            return Ok(false);
        };
        if !guard1_else.is_empty() {
            return Ok(false);
        }
        let [Statement::If { condition: sign1, then_body: sign1_then, else_body: sign1_else }] =
            guard1_body.as_slice()
        else {
            return Ok(false);
        };
        // The sign comparison: `i0 >= 0` (s_floor) or `i0 < 0` (s_ceil) —
        // the emitted branch is the inverted sense to the else arm either
        // way.
        let Expression::Binary { operator: sign1_op, left: sign1_l, right: sign1_r } = sign1 else {
            return Ok(false);
        };
        let sign1_branch = match sign1_op {
            BinaryOperator::GreaterEqual => (12u8, 0u8), // blt
            BinaryOperator::Less => (4u8, 0u8),          // bge
            _ => return Ok(false),
        };
        if !matches!(sign1_l.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
            || crate::analysis::constant_value(sign1_r) != Some(0)
        {
            return Ok(false);
        }
        // A constant pair `[i0 = C, i1 = C']` (each li or lis form), or the
        // chained `i0 = i1 = 0` (emitted inner-first).
        enum ConstPair {
            Chained0,
            Pair { first: i64, second: i64 },
        }
        let parse_pair = |body: &[Statement]| -> Option<ConstPair> {
            match body {
                [Statement::Assign { name, value: Expression::Assign { target, value } }]
                    if local_index(name) == Some(i0)
                        && matches!(target.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                        && crate::analysis::constant_value(value) == Some(0) =>
                {
                    Some(ConstPair::Chained0)
                }
                [Statement::Assign { name: a, value: av }, Statement::Assign { name: b, value: bv }]
                    if local_index(a) == Some(i0) && local_index(b) == Some(i1) =>
                {
                    let first = crate::analysis::constant_value(av)? as u32 as i32 as i64;
                    let second = crate::analysis::constant_value(bv)? as u32 as i32 as i64;
                    let representable = |constant: i64| {
                        i16::try_from(constant).is_ok() || constant & 0xffff == 0
                    };
                    (representable(first) && representable(second))
                        .then_some(ConstPair::Pair { first, second })
                }
                _ => None,
            }
        };
        let Some(sign1_pair) = parse_pair(sign1_then) else {
            return Ok(false);
        };
        // else: If{((i0 [& M]) | i1) != 0, [pair]} — the mask is optional
        // (s_ceil's plain `(i0 | i1) != 0`).
        let [Statement::If { condition: mag_cond, then_body: mag_then, else_body: mag_else }] =
            sign1_else.as_slice()
        else {
            return Ok(false);
        };
        if !mag_else.is_empty() {
            return Ok(false);
        }
        let Some(mag_mask) = (|| {
            let Expression::Binary { operator: BinaryOperator::NotEqual, left, right } = mag_cond
            else {
                return None;
            };
            if crate::analysis::constant_value(right) != Some(0) {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::BitOr, left: or_l, right: or_r } =
                left.as_ref()
            else {
                return None;
            };
            if !matches!(or_r.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)) {
                return None;
            }
            match or_l.as_ref() {
                Expression::Variable(v) if local_index(v) == Some(i0) => Some(None),
                Expression::Binary { operator: BinaryOperator::BitAnd, left: and_l, right: and_r }
                    if matches!(and_l.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0)) =>
                {
                    let mask = crate::analysis::constant_value(and_r)?;
                    let (begin, end) = crate::analysis::rlwinm_mask(mask)?;
                    Some(Some((begin, end)))
                }
                _ => None,
            }
        })() else {
            return Ok(false);
        };
        let Some(ConstPair::Pair { first: mag_first, second: mag_second }) = parse_pair(mag_then)
        else {
            return Ok(false);
        };
        // ARM2 (fire 399): [i = C >> j0, If{test, [Ret x]}, If{huge, [If{i0<0, [i0 += C2>>j0]}, i0 &= ~i, i1 = 0]}].
        let [Statement::Assign { name: a2_shift_name, value: a2_shift_value }, Statement::If { condition: a2_test, then_body: a2_ret, else_body: a2_test_else }, Statement::If { condition: a2_guard, then_body: a2_guard_body, else_body: a2_guard_else }] =
            arm2.as_slice()
        else {
            return Ok(false);
        };
        if a2_shift_name != shift || !a2_test_else.is_empty() || !a2_guard_else.is_empty() {
            return Ok(false);
        }
        let Some((a2_mask, a2_logical, a2_offset)) = parse_shift_init(a2_shift_value, guard.name)
        else {
            return Ok(false);
        };
        if a2_logical || a2_offset != 0 || i16::try_from(a2_mask).is_ok() {
            return Ok(false);
        }
        let a2_lis = ((a2_mask + 0x8000) >> 16) << 16;
        if !matches!(a2_ret.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x) {
            return Ok(false);
        }
        // test: ((i0 & i) | i1) == 0
        let a2_test_ok = (|| {
            let Expression::Binary { operator: BinaryOperator::Equal, left, right } = a2_test else {
                return false;
            };
            if crate::analysis::constant_value(right) != Some(0) {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::BitOr, left: or_l, right: or_r } =
                left.as_ref()
            else {
                return false;
            };
            matches!(or_r.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                && matches!(or_l.as_ref(),
                    Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                        if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                            && matches!(ar.as_ref(), Expression::Variable(v) if v == shift))
        })();
        if !a2_test_ok || float_guard_condition(a2_guard) != Some((huge_bits, zero_bits)) {
            return Ok(false);
        }
        let [Statement::If { condition: a2_sign, then_body: a2_add, else_body: a2_sign_else }, Statement::Assign { name: a2_andc_name, value: a2_andc_value }, Statement::Assign { name: a2_rw_name, value: a2_rw_value }] =
            a2_guard_body.as_slice()
        else {
            return Ok(false);
        };
        let parse_sign = |condition: &Expression| -> Option<(u8, u8)> {
            let Expression::Binary { operator, left, right } = condition else { return None };
            if !matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                || crate::analysis::constant_value(right) != Some(0)
            {
                return None;
            }
            match operator {
                BinaryOperator::Less => Some((4, 0)),    // bge — skip when >= 0
                BinaryOperator::Greater => Some((4, 1)), // ble — skip when <= 0
                _ => None,
            }
        };
        let Some(a2_sign_branch) = parse_sign(a2_sign) else {
            return Ok(false);
        };
        let a2_ok = a2_sign_else.is_empty()
            && matches!(a2_add.as_slice(), [Statement::Assign { name, value }]
                if local_index(name) == Some(i0)
                    && matches!(value, Expression::Binary { operator: BinaryOperator::Add, left, right }
                        if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                            && matches!(right.as_ref(),
                                Expression::Binary { operator: BinaryOperator::ShiftRight, left: c2, right: by }
                                    if crate::analysis::constant_value(c2) == Some(a2_lis)
                                        && matches!(by.as_ref(), Expression::Variable(v) if v == guard.name))))
            && local_index(a2_andc_name) == Some(i0)
            && matches!(a2_andc_value, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                    && matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift)))
            && local_index(a2_rw_name) == Some(i1)
            && crate::analysis::constant_value(a2_rw_value) == Some(0);
        if !a2_ok {
            return Ok(false);
        }
        // ARM3 (fire 400): [i = (unsigned)C >> (j0-K4), If{(i1&i)==0, [Ret x]},
        //   If{huge, [If{i0<0, [If{j0==K5, [i0+=1], [carry]}]}, i1 &= ~i]}].
        let [Statement::Assign { name: a3_shift_name, value: a3_shift_value }, Statement::If { condition: a3_test, then_body: a3_ret, else_body: a3_test_else }, Statement::If { condition: a3_guard, then_body: a3_guard_body, else_body: a3_guard_else }] =
            arm3.as_slice()
        else {
            return Ok(false);
        };
        if a3_shift_name != shift || !a3_test_else.is_empty() || !a3_guard_else.is_empty() {
            return Ok(false);
        }
        let Some((a3_mask, a3_logical, a3_offset)) = parse_shift_init(a3_shift_value, guard.name)
        else {
            return Ok(false);
        };
        let a3_mask = a3_mask as u32 as i32 as i64;
        let (Ok(a3_mask_small), Ok(a3_offset_neg)) = (i16::try_from(a3_mask), i16::try_from(-a3_offset))
        else {
            return Ok(false);
        };
        if !a3_logical || a3_offset == 0 {
            return Ok(false);
        }
        if !matches!(a3_ret.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x) {
            return Ok(false);
        }
        let a3_test_ok = matches!(a3_test, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if crate::analysis::constant_value(right) == Some(0)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                    if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                        && matches!(ar.as_ref(), Expression::Variable(v) if v == shift)));
        if !a3_test_ok || float_guard_condition(a3_guard) != Some((huge_bits, zero_bits)) {
            return Ok(false);
        }
        let [Statement::If { condition: a3_sign, then_body: a3_diamond, else_body: a3_sign_else }, Statement::Assign { name: a3_andc_name, value: a3_andc_value }] =
            a3_guard_body.as_slice()
        else {
            return Ok(false);
        };
        let Some(a3_sign_branch) = parse_sign(a3_sign) else {
            return Ok(false);
        };
        let a3_frame_ok = a3_sign_else.is_empty()
            && local_index(a3_andc_name) == Some(i1)
            && matches!(a3_andc_value, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                    && matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift)));
        if !a3_frame_ok {
            return Ok(false);
        }
        let [Statement::If { condition: eq_cond, then_body: eq_then, else_body: eq_else }] =
            a3_diamond.as_slice()
        else {
            return Ok(false);
        };
        let Some(k5) = parse_guard_compare(eq_cond, BinaryOperator::Equal) else {
            return Ok(false);
        };
        let inc_ok = |body: &[Statement]| {
            matches!(body, [Statement::Assign { name, value }]
                if local_index(name) == Some(i0)
                    && matches!(value, Expression::Binary { operator: BinaryOperator::Add, left, right }
                        if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                            && crate::analysis::constant_value(right) == Some(1)))
        };
        if !inc_ok(eq_then) {
            return Ok(false);
        }
        let [Statement::Assign { name: j_name, value: j_value }, Statement::If { condition: carry_cond, then_body: carry_then, else_body: carry_else }, Statement::Assign { name: copy_name, value: copy_value }] =
            eq_else.as_slice()
        else {
            return Ok(false);
        };
        let Some(k6) = (|| {
            if j_name != carry {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left: base, right: one_shift } =
                j_value
            else {
                return None;
            };
            if !matches!(base.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)) {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: one, right: amount } =
                one_shift.as_ref()
            else {
                return None;
            };
            if crate::analysis::constant_value(one) != Some(1) {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::Subtract, left: k6, right: by } =
                amount.as_ref()
            else {
                return None;
            };
            if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                return None;
            }
            crate::analysis::constant_value(k6).and_then(|k| i16::try_from(k).ok())
        })() else {
            return Ok(false);
        };
        let carry_ok = carry_else.is_empty()
            && matches!(carry_cond, Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if v == carry)
                    && matches!(right.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)))
            && inc_ok(carry_then)
            && local_index(copy_name) == Some(i1)
            && matches!(copy_value, Expression::Variable(v) if v == carry);
        if !carry_ok {
            return Ok(false);
        }
        // -- the model (positions computed from the emission template) --
        use mwcc_vreg::int_alloc::{allocate, Class, Value};
        // arm1's sign diamond: [cmpwi, branch, then(1 or 2), b] + else
        // ([clrlwi]?, or., beq, lis, li, b).
        let sign1_then_len: u32 = match &sign1_pair {
            ConstPair::Chained0 => 2,
            ConstPair::Pair { .. } => 2,
        };
        let mag_len: u32 = if mag_mask.is_some() { 6 } else { 5 };
        let arm1_diamond = 2 + sign1_then_len + 1 + mag_len;
        let arm2_base = 15 + arm1_diamond; // preamble 0..9 + float(4)+ble @10..14
        let ladder2 = arm2_base + 19;
        let arm3_base = ladder2 + 6;
        let join_at = arm3_base + 26;
        let values = [
            Value { class: Class::Temp, def: 4, last: 5 },
            Value { class: Class::Temp, def: arm2_base, last: arm2_base + 14 }, // lis..sraw2 (CSE)
            Value { class: Class::Mask, def: arm3_base + 1, last: arm3_base + 2 },
            Value { class: Class::Mask, def: arm3_base + 18, last: arm3_base + 19 }, // the ONE
            Value { class: Class::Scrutinee, def: 5, last: arm3_base + 17 },   // ..subfic
            Value { class: Class::LoadSurviving, def: 2, last: join_at },
            Value { class: Class::LoadSurviving, def: 3, last: join_at + 1 },
            Value { class: Class::ArmShift, def: arm2_base + 2, last: arm2_base + 16 },
            Value { class: Class::ArmShift, def: arm3_base + 2, last: arm3_base + 25 },
        ];
        let registers = allocate(&values);
        let extract_temp = registers[0];
        let a2_temp = registers[1];
        let a3_mask_reg = registers[2];
        let one_reg = registers[3];
        let j0_reg = registers[4];
        let i0_reg = if i0 == 0 { registers[5] } else { registers[6] };
        let i1_reg = if i0 == 0 { registers[6] } else { registers[5] };
        let a2_i = registers[7];
        let a3_i = registers[8];
        // NB: loads emit in frame-offset order = locals order; registers[5]
        // belongs to locals[0].
        let load0 = registers[5];
        let load1 = registers[6];
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: load0, a: 1, offset: 8 + locals[0].1 });
        self.output.instructions.push(Instruction::LoadWord { d: load1, a: 1, offset: 8 + locals[1].1 });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: extract_temp,
                    s: i0_reg,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: extract_temp,
                    s: i0_reg,
                    shift: guard.shift,
                });
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: j0_reg,
            a: extract_temp,
            immediate: i16::try_from(-guard.offset_k).expect("validated"),
        });
        let join = self.fresh_label();
        let epilogue = self.fresh_label();
        let ladder2_at = self.fresh_label();
        let arm2_at = self.fresh_label();
        let arm3_at = self.fresh_label();
        // The ladder.
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k1 });
        self.emit_branch_conditional_to(4, 0, ladder2_at); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, arm2_at); // bge
        // ARM1.
        self.load_double_constant(2, huge_bits);
        self.load_double_constant(0, zero_bits);
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let arm1_else = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: i0_reg, immediate: 0 });
        self.emit_branch_conditional_to(sign1_branch.0, sign1_branch.1, arm1_else);
        let emit_constant = |generator: &mut Self, register: u8, constant: i64| {
            if let Ok(small) = i16::try_from(constant) {
                generator.output.instructions.push(Instruction::load_immediate(register, small));
            } else {
                generator
                    .output
                    .instructions
                    .push(Instruction::load_immediate_shifted(register, (constant >> 16) as i16));
            }
        };
        match &sign1_pair {
            ConstPair::Chained0 => {
                // The chained `i0 = i1 = 0` assigns inner-first.
                self.output.instructions.push(Instruction::load_immediate(i1_reg, 0));
                self.output.instructions.push(Instruction::load_immediate(i0_reg, 0));
            }
            ConstPair::Pair { first, second } => {
                emit_constant(self, i0_reg, *first);
                emit_constant(self, i1_reg, *second);
            }
        }
        self.emit_branch_to(join);
        self.bind_label(arm1_else);
        if let Some((begin, end)) = mag_mask {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 0,
                s: i0_reg,
                shift: 0,
                begin,
                end,
            });
            self.output.instructions.push(Instruction::OrRecord { a: 0, s: 0, b: i1_reg });
        } else {
            self.output.instructions.push(Instruction::OrRecord { a: 0, s: i0_reg, b: i1_reg });
        }
        self.emit_branch_conditional_to(12, 2, join); // beq
        emit_constant(self, i0_reg, mag_first);
        emit_constant(self, i1_reg, mag_second);
        self.emit_branch_to(join);
        // ARM2.
        self.bind_label(arm2_at);
        self.output.instructions.push(Instruction::load_immediate_shifted(a2_temp, (a2_lis >> 16) as i16));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: a2_temp,
            immediate: a2_mask as i16,
        });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: a2_i, s: 0, b: j0_reg });
        self.output.instructions.push(Instruction::And { a: 0, s: i0_reg, b: a2_i });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: i1_reg, b: 0 });
        let a2_cont = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a2_cont); // bne
        self.emit_branch_to(epilogue);
        self.bind_label(a2_cont);
        self.load_double_constant(2, huge_bits);
        self.load_double_constant(0, zero_bits);
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let a2_skip = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: i0_reg, immediate: 0 });
        self.emit_branch_conditional_to(a2_sign_branch.0, a2_sign_branch.1, a2_skip);
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: 0, s: a2_temp, b: j0_reg });
        self.output.instructions.push(Instruction::Add { d: i0_reg, a: i0_reg, b: 0 });
        self.bind_label(a2_skip);
        self.output.instructions.push(Instruction::AndComplement { a: i0_reg, s: i0_reg, b: a2_i });
        self.output.instructions.push(Instruction::load_immediate(i1_reg, 0));
        self.emit_branch_to(join);
        // LADDER 2 + MID.
        self.bind_label(ladder2_at);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k2 });
        self.emit_branch_conditional_to(4, 1, arm3_at); // ble
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k3 });
        self.emit_branch_conditional_to(4, 2, epilogue); // bne — return x
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 1 });
        self.emit_branch_to(epilogue);
        // ARM3.
        self.bind_label(arm3_at);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: j0_reg, immediate: a3_offset_neg });
        self.output.instructions.push(Instruction::load_immediate(a3_mask_reg, a3_mask_small));
        self.output.instructions.push(Instruction::ShiftRightWord { a: a3_i, s: a3_mask_reg, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: i1_reg, b: a3_i });
        let a3_cont = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a3_cont); // bne
        self.emit_branch_to(epilogue);
        self.bind_label(a3_cont);
        self.load_double_constant(2, huge_bits);
        self.load_double_constant(0, zero_bits);
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let a3_andc = self.fresh_label();
        let a3_carry = self.fresh_label();
        let a3_no_carry = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: i0_reg, immediate: 0 });
        self.emit_branch_conditional_to(a3_sign_branch.0, a3_sign_branch.1, a3_andc);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k5 });
        self.emit_branch_conditional_to(4, 2, a3_carry); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: i0_reg, a: i0_reg, immediate: 1 });
        self.emit_branch_to(a3_andc);
        self.bind_label(a3_carry);
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: j0_reg, immediate: k6 });
        self.output.instructions.push(Instruction::load_immediate(one_reg, 1));
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: one_reg, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 0, a: i1_reg, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: i1_reg });
        self.emit_branch_conditional_to(4, 0, a3_no_carry); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: i0_reg, a: i0_reg, immediate: 1 });
        self.bind_label(a3_no_carry);
        self.output.instructions.push(Instruction::move_register(i1_reg, 0));
        self.bind_label(a3_andc);
        self.output.instructions.push(Instruction::AndComplement { a: i1_reg, s: i1_reg, b: a3_i });
        // JOIN + EPI.
        self.bind_label(join);
        self.output.instructions.push(Instruction::StoreWord { s: load0, a: 1, offset: 8 + locals[0].1 });
        self.output.instructions.push(Instruction::StoreWord { s: load1, a: 1, offset: 8 + locals[1].1 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured on the full s_floor object: real @45
        // vs the +0 base's @5).
        self.output.anonymous_label_bump += 40;
        Ok(true)
    }

    /// The SHIFT-WRITEBACK family (s_floor arm2's core): statements =
    /// `[i = C >> j0]  [if (test) return x]  [mutations...]  [stores...]`
    /// with a multi-use shifted mask. Registers come from the fitted
    /// int_alloc v2 model (13/13 captures — docs/int-allocator-frontier.md):
    /// a synthetic position pass numbers the template, values classify as
    /// Temp/Mask/Computed/Load{Discarded,Surviving}/Shift, and the model
    /// orders lowest-free assignment. Measured forms:
    ///   test: `((a & i) | b) == 0` (and + or., b FIRST) or `(a & i) == 0`
    ///     (and. record); skip = bne CONT; b EPI; CONT:.
    ///   mutations: `l &= ~i` (fused andc; TWO of them share one not r0),
    ///     `l &= K` (clrlwi r0, store from r0 — the home is read only),
    ///     `l = K` (li r0, store from r0 — the home is DISCARDED when it
    ///     was read in the test, and never loaded when read nowhere).
    pub(crate) fn try_punned_shift_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Roles come either from local INITIALIZERS (the normalizer folds
        // the leading assigns when nothing reassigns at top level — the
        // guarded-mutation forms) or from the LEADING assigns themselves
        // (top-level mutations make the normalizer refuse).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        let mut mask_constant: Option<(i64, bool, i64)> = None; // (C, logical, amount offset)
        let mut cursor = 0usize;
        // The carry local (arm3's `j`) is assigned only inside the guard,
        // so the normalizer leaves it uninitialized while folding the rest.
        let mut carry_local: Option<&str> = None;
        let normalized = !function.locals.is_empty()
            && function.locals.iter().any(|local| local.initializer.is_some());
        if normalized {
            for local in &function.locals {
                if local.array_length.is_some() {
                    return Ok(false);
                }
                let Some(init) = local.initializer.as_ref() else {
                    if local.declared_type == Type::UnsignedInt && carry_local.is_none() {
                        carry_local = Some(local.name.as_str());
                        continue;
                    }
                    return Ok(false);
                };
                if local.declared_type == Type::UnsignedInt {
                    if shift.is_some() {
                        return Ok(false);
                    }
                    let Some(parsed) = &guard else { return Ok(false) };
                    let Some((constant, logical, offset)) = parse_shift_init(init, parsed.name)
                    else {
                        return Ok(false);
                    };
                    mask_constant = Some((constant, logical, offset));
                    shift = Some(local.name.as_str());
                    continue;
                }
                if local.declared_type != Type::Int {
                    return Ok(false);
                }
                if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                    if locals.iter().any(|&(_, seen)| seen == offset) {
                        return Ok(false);
                    }
                    locals.push((local.name.as_str(), offset));
                    continue;
                }
                if guard.is_some() {
                    return Ok(false);
                }
                let Some(parsed) = parse_guard_init(local.name.as_str(), init) else {
                    return Ok(false);
                };
                if !locals.iter().any(|&(name, _)| name == parsed.source) {
                    return Ok(false);
                }
                guard = Some(parsed);
            }
        } else {
            while let Some(Statement::Assign { name, value }) = function.statements.get(cursor) {
                let Some(declaration) = function.locals.iter().find(|local| &local.name == name) else {
                    return Ok(false);
                };
                if declaration.initializer.is_some() || declaration.array_length.is_some() {
                    return Ok(false);
                }
                if let Some(offset) = crate::frame::pun_word_offset_pub(value, x) {
                    if declaration.declared_type != Type::Int
                        || locals.iter().any(|&(_, seen)| seen == offset)
                    {
                        return Ok(false);
                    }
                    locals.push((name.as_str(), offset));
                    cursor += 1;
                    continue;
                }
                if guard.is_none() && declaration.declared_type == Type::Int {
                    if let Some(parsed) = parse_guard_init(name.as_str(), value) {
                        if locals.iter().any(|&(local, _)| local == parsed.source) {
                            guard = Some(parsed);
                            cursor += 1;
                            continue;
                        }
                    }
                    return Ok(false);
                }
                if shift.is_none() && declaration.declared_type == Type::UnsignedInt {
                    if let Some(parsed) = &guard {
                        if let Some((constant, logical, offset)) = parse_shift_init(value, parsed.name) {
                            mask_constant = Some((constant, logical, offset));
                            shift = Some(name.as_str());
                            cursor += 1;
                            continue;
                        }
                    }
                    return Ok(false);
                }
                return Ok(false);
            }
        }
        let (Some(guard), Some(shift), Some((mask_constant, logical_shift, amount_offset))) =
            (guard, shift, mask_constant)
        else {
            return Ok(false);
        };
        if i16::try_from(-amount_offset).is_err() {
            return Ok(false);
        }
        if locals.is_empty() || locals.len() > 2 {
            return Ok(false);
        }
        if guard.offset_k == 0 || i16::try_from(-guard.offset_k).is_err() {
            return Ok(false);
        }
        // j0 is consumed by the shift alone; the shift local is written once.
        let tail = &function.statements[cursor..];
        fn reads_in(statement: &Statement, name: &str) -> usize {
            match statement {
                Statement::Assign { value, .. } => count_name_occurrences(value, name),
                Statement::Store { target, value } => {
                    count_name_occurrences(target, name) + count_name_occurrences(value, name)
                }
                Statement::If { condition, then_body, else_body } => {
                    count_name_occurrences(condition, name)
                        + then_body.iter().map(|inner| reads_in(inner, name)).sum::<usize>()
                        + else_body.iter().map(|inner| reads_in(inner, name)).sum::<usize>()
                }
                Statement::Return(Some(value)) => count_name_occurrences(value, name),
                _ => 1,
            }
        }
        let guard_tail_reads: usize =
            tail.iter().map(|statement| reads_in(statement, guard.name)).sum();
        if guard_tail_reads > 2 {
            // Beyond the sign block's reads (the self-add's shift, or the
            // carry diamond's ==K4 + K3-j0 pair — validated structurally
            // below; unknown j0 uses fail the exact-form parses).
            return Ok(false);
        }
        // The early-return test.
        let Some(Statement::If { condition, then_body, else_body }) = tail.first() else {
            return Ok(false);
        };
        if !matches!(then_body.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x)
            || !else_body.is_empty()
        {
            return Ok(false);
        }
        // `((a & i) | b) == 0` or `(a & i) == 0`, a/b punned, i the shift.
        let Expression::Binary { operator: BinaryOperator::Equal, left: test, right: zero } = condition
        else {
            return Ok(false);
        };
        if crate::analysis::constant_value(zero) != Some(0) {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let parse_and = |expr: &Expression| -> Option<usize> {
            let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = expr else {
                return None;
            };
            let Expression::Variable(a) = left.as_ref() else { return None };
            let Expression::Variable(i) = right.as_ref() else { return None };
            if i != shift {
                return None;
            }
            local_index(a)
        };
        let (test_and_local, test_or_local) = match test.as_ref() {
            Expression::Binary { operator: BinaryOperator::BitOr, left, right } => {
                let Some(a) = parse_and(left) else { return Ok(false) };
                let Expression::Variable(b) = right.as_ref() else { return Ok(false) };
                let Some(b) = local_index(b) else { return Ok(false) };
                (a, Some(b))
            }
            other => {
                let Some(a) = parse_and(other) else { return Ok(false) };
                (a, None)
            }
        };
        // An optional inexact guard wraps the mutations: `if (huge+x>0.0)
        // { [if (l<0) l += C2>>j0;] mutations }` (s_floor arm2). Inside
        // it a rewrite is CONDITIONAL — the original must survive the
        // guard-false path, so it lands in the home, not r0.
        let mut float_guard: Option<(u64, u64)> = None;
        enum SignBlock {
            Add { local: usize, constant: i64 },
            CarryDiamond {
                local: usize,          // i0 — takes +1
                other: usize,          // i1 — the carry source, receives j
                equal_bound: i16,      // j0 == K4
                shift_base: i16,       // K3 in `1 << (K3 - j0)`
            },
        }
        let mut sign_block: Option<SignBlock> = None;
        let mut mutation_statements: &[Statement] = &tail[1..];
        if let Some(Statement::If { condition, then_body, else_body }) = tail.get(1) {
            let Some(guard_bits) = float_guard_condition(condition) else {
                return Ok(false);
            };
            if !else_body.is_empty() {
                return Ok(false);
            }
            float_guard = Some(guard_bits);
            let mut body: &[Statement] = then_body;
            if let Some(Statement::If { condition, then_body: sign_body, else_body }) = body.first() {
                // `if (l < 0) ...`
                let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition
                else {
                    return Ok(false);
                };
                if crate::analysis::constant_value(right) != Some(0) {
                    return Ok(false);
                }
                let Expression::Variable(signed) = left.as_ref() else { return Ok(false) };
                let Some(sign_local) = local_index(signed) else { return Ok(false) };
                if !else_body.is_empty() {
                    return Ok(false);
                }
                match sign_body.as_slice() {
                    // arm2: `l += C2 >> j0;`
                    [Statement::Assign { name: add_name, value: add_value }] => {
                        if local_index(add_name) != Some(sign_local) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Add, left: base, right: shifted } =
                            add_value
                        else {
                            return Ok(false);
                        };
                        if !matches!(base.as_ref(), Expression::Variable(v) if v == add_name.as_str()) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: c2, right: by } =
                            shifted.as_ref()
                        else {
                            return Ok(false);
                        };
                        let Some(c2) = crate::analysis::constant_value(c2) else { return Ok(false) };
                        if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        sign_block = Some(SignBlock::Add { local: sign_local, constant: c2 });
                    }
                    // arm3: `if (j0 == K4) l += 1; else { j = other + (1 << (K3 - j0));
                    //        if (j < other) l += 1; other = j; }`
                    [Statement::If { condition, then_body, else_body }] => {
                        let Some(carry) = carry_local else { return Ok(false) };
                        let Expression::Binary { operator: BinaryOperator::Equal, left, right } = condition
                        else {
                            return Ok(false);
                        };
                        if !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        let Some(equal_bound) =
                            crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        // then: l += 1
                        let [Statement::Assign { name: inc, value: inc_value }] = then_body.as_slice()
                        else {
                            return Ok(false);
                        };
                        if local_index(inc) != Some(sign_local)
                            || !matches!(inc_value,
                                Expression::Binary { operator: BinaryOperator::Add, left, right }
                                    if matches!(left.as_ref(), Expression::Variable(v) if v == inc.as_str())
                                        && crate::analysis::constant_value(right) == Some(1))
                        {
                            return Ok(false);
                        }
                        // else: the carry sequence
                        let [Statement::Assign { name: j_name, value: j_value }, Statement::If { condition: carry_cond, then_body: carry_then, else_body: carry_else }, Statement::Assign { name: copy_name, value: copy_value }] =
                            else_body.as_slice()
                        else {
                            return Ok(false);
                        };
                        if j_name != carry {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Add, left: base, right: one_shift } =
                            j_value
                        else {
                            return Ok(false);
                        };
                        let Expression::Variable(other_name) = base.as_ref() else { return Ok(false) };
                        let Some(other) = local_index(other_name) else { return Ok(false) };
                        let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: one, right: amount } =
                            one_shift.as_ref()
                        else {
                            return Ok(false);
                        };
                        if crate::analysis::constant_value(one) != Some(1) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Subtract, left: k3, right: by } =
                            amount.as_ref()
                        else {
                            return Ok(false);
                        };
                        let Some(shift_base) =
                            crate::analysis::constant_value(k3).and_then(|k| i16::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        // if (j < other) l += 1;
                        if !carry_else.is_empty() {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Less, left: jl, right: jr } =
                            carry_cond
                        else {
                            return Ok(false);
                        };
                        if !matches!(jl.as_ref(), Expression::Variable(v) if v == carry)
                            || !matches!(jr.as_ref(), Expression::Variable(v) if local_index(v) == Some(other))
                        {
                            return Ok(false);
                        }
                        let [Statement::Assign { name: inc2, value: inc2_value }] = carry_then.as_slice()
                        else {
                            return Ok(false);
                        };
                        if local_index(inc2) != Some(sign_local)
                            || !matches!(inc2_value,
                                Expression::Binary { operator: BinaryOperator::Add, left, right }
                                    if matches!(left.as_ref(), Expression::Variable(v) if v == inc2.as_str())
                                        && crate::analysis::constant_value(right) == Some(1))
                        {
                            return Ok(false);
                        }
                        // other = j
                        if local_index(copy_name) != Some(other)
                            || !matches!(copy_value, Expression::Variable(v) if v == carry)
                        {
                            return Ok(false);
                        }
                        sign_block = Some(SignBlock::CarryDiamond {
                            local: sign_local,
                            other,
                            equal_bound,
                            shift_base,
                        });
                    }
                    _ => return Ok(false),
                }
                body = &body[1..];
            }
            mutation_statements = body;
        }
        // The self-add's constant must equal the mask synthesis' lis
        // intermediate — mwcc reuses the materialized register (measured:
        // 0x00100000 for the 0xfffff mask). Anything else is unprobed.
        // The constant wraps to its 32-bit value (0xffffffff = li -1).
        let mask_constant = mask_constant as u32 as i32 as i64;
        let needs_temp_early = i16::try_from(mask_constant).is_err();
        let lis_intermediate = ((mask_constant + 0x8000) >> 16) << 16;
        if let Some(SignBlock::Add { constant, .. }) = sign_block {
            // The self-add's constant must CSE the lis intermediate.
            if !needs_temp_early || constant != lis_intermediate || float_guard.is_none() {
                return Ok(false);
            }
        }
        if matches!(sign_block, Some(SignBlock::CarryDiamond { .. })) && float_guard.is_none() {
            return Ok(false);
        }
        if carry_local.is_some() && !matches!(sign_block, Some(SignBlock::CarryDiamond { .. })) {
            return Ok(false);
        }
        // j0's reads beyond the mask shift: the self-add's shift, or the
        // carry diamond's ==K4 and K3-j0.
        let guard_multi_read = sign_block.is_some() || amount_offset != 0;
        // Mutations, then stores.
        enum Mutation {
            Rewrite(i16),
            AndcShift,
            MaskViaScratch { begin: u8, end: u8 },
        }
        let mut mutations: Vec<(usize, Mutation)> = Vec::new();
        let mut tail_cursor = 0usize;
        while let Some(Statement::Assign { name, value }) = mutation_statements.get(tail_cursor) {
            let Some(index) = local_index(name) else { return Ok(false) };
            if mutations.iter().any(|&(seen, _)| seen == index) {
                return Ok(false);
            }
            let mutation = if let Some(constant) = crate::analysis::constant_value(value) {
                let Ok(small) = i16::try_from(constant) else { return Ok(false) };
                Mutation::Rewrite(small)
            } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = value {
                if !matches!(left.as_ref(), Expression::Variable(v) if v == name.as_str()) {
                    return Ok(false);
                }
                match right.as_ref() {
                    Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift) =>
                    {
                        Mutation::AndcShift
                    }
                    other => {
                        let Some((begin, end)) = crate::analysis::constant_value(other)
                            .and_then(crate::analysis::rlwinm_mask)
                        else {
                            return Ok(false);
                        };
                        if float_guard.is_some() {
                            // Unprobed inside a guard (the r0 handoff to the
                            // store would cross the guard bounds).
                            return Ok(false);
                        }
                        Mutation::MaskViaScratch { begin, end }
                    }
                }
            } else {
                return Ok(false);
            };
            mutations.push((index, mutation));
            tail_cursor += 1;
        }
        // At most one rewrite (the li r0 dedupe across two is unmeasured).
        if mutations.iter().filter(|(_, m)| matches!(m, Mutation::Rewrite(_))).count() > 1 {
            return Ok(false);
        }
        // The r0 materialization sinks below the home-writing mutations
        // regardless of source order (measured D3: andc; li r0; stores) —
        // r0's range stays minimal. A CONDITIONAL rewrite (inside the
        // guard) stays in source position: it writes the home.
        if float_guard.is_none() {
            mutations.sort_by_key(|(_, mutation)| matches!(mutation, Mutation::Rewrite(_)));
        }
        // Stores: one per local, its own offset, in local order. With a
        // guard the mutations exhaust its body and the stores follow the
        // guard-If in the outer tail.
        let stores = if float_guard.is_some() {
            if tail_cursor != mutation_statements.len() {
                return Ok(false);
            }
            &tail[2..]
        } else {
            &mutation_statements[tail_cursor..]
        };
        if stores.len() != locals.len() {
            return Ok(false);
        }
        for (statement, &(name, offset)) in stores.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset)
                || !matches!(value, Expression::Variable(read) if read == name)
            {
                return Ok(false);
            }
        }
        // -- the synthetic position pass --
        use mwcc_vreg::int_alloc::{allocate, Class, Value};
        let needs_temp = i16::try_from(mask_constant).is_err();
        let mut position = 1u32; // 0 = stwu
        let temp_range = needs_temp.then(|| {
            let range = (position, position + 1);
            position += 1;
            range
        });
        let mask_position = position; // li or the addi completing the pair
        position += 1;
        position += 1; // stfd
        // Which locals load: any with a read (test, extract source, andc/mask mutation).
        let has_read = |index: usize| {
            index == test_and_local
                || test_or_local == Some(index)
                || locals[index].0 == guard.source
                || matches!(sign_block, Some(SignBlock::Add { local, .. }) if local == index)
                || matches!(sign_block, Some(SignBlock::CarryDiamond { local, other, .. }) if local == index || other == index)
                || mutations.iter().any(|&(m, ref form)| {
                    m == index
                        && (!matches!(form, Mutation::Rewrite(_)) || float_guard.is_some())
                })
        };
        let mut load_positions: Vec<Option<u32>> = Vec::new();
        for index in 0..locals.len() {
            if has_read(index) {
                load_positions.push(Some(position));
                position += 1;
            } else {
                load_positions.push(None);
            }
        }
        let extract_position = position;
        position += 1;
        let fold_position = position;
        position += 1;
        let sraw_position = position;
        position += 1;
        let and_position = position;
        position += 1;
        let or_position = test_or_local.map(|_| {
            let at = position;
            position += 1;
            at
        });
        let branch_position = position; // bne
        position += 2; // bne + b
        // The inexact-guard block (lfd, lfd, fadd, fcmpo, ble) and the
        // sign-add (cmpwi, bge, sraw2, add).
        if float_guard.is_some() {
            position += 5;
        }
        // The sign block: Add = cmpwi, bge, sraw2, add; CarryDiamond =
        // cmpwi, bge, cmpwi, bne, addi, b, subfic, li, slw, add, cmplw,
        // bge, addi, mr.
        let mut carry_one_range: Option<(u32, u32)> = None;
        let sraw2_position = match &sign_block {
            Some(SignBlock::Add { .. }) => {
                position += 2; // cmpwi + bge
                let at = position;
                position += 2; // sraw2 + add
                Some(at)
            }
            Some(SignBlock::CarryDiamond { .. }) => {
                position += 6; // cmpwi, bge, cmpwi(==K4), bne, addi, b
                let subfic_at = position;
                position += 1;
                let one_at = position;
                position += 1; // li 1
                carry_one_range = Some((one_at, position)); // li..slw
                position += 6; // slw, add, cmplw, bge, addi, mr
                Some(subfic_at) // j0's last read = the subfic
            }
            None => None,
        };
        // Mutations occupy sequential slots (the shared `not` adds one).
        let andc_count = mutations.iter().filter(|(_, m)| matches!(m, Mutation::AndcShift)).count();
        let not_position = (andc_count >= 2).then(|| {
            let at = position;
            position += 1;
            at
        });
        let mut mutation_positions: Vec<u32> = Vec::new();
        for _ in &mutations {
            mutation_positions.push(position);
            position += 1;
        }
        let mut store_positions: Vec<u32> = Vec::new();
        for _ in &locals {
            store_positions.push(position);
            position += 1;
        }
        // -- classify + model --
        let mut values: Vec<Value> = Vec::new();
        let mut tags: Vec<&str> = Vec::new(); // parallel debug tags
        if let Some((lis, addi)) = temp_range {
            // The self-add's constant CSEs the lis intermediate — the
            // temp then lives to the second sraw (measured arm2).
            let last = sraw2_position.unwrap_or(addi);
            values.push(Value { class: Class::Temp, def: lis, last });
            tags.push("temp");
        }
        // With a MULTI-READ guard the fold lands in the home, freeing the
        // r0 timeline — the branch-free mask takes r0 itself (measured
        // arm2: addi r0,r3,-1).
        // ...unless an amount offset (arm3's j0-20) writes r0 inside the
        // mask's live range.
        let mask_in_scratch = guard_multi_read && amount_offset == 0;
        let mask_value_index = if mask_in_scratch {
            None
        } else {
            values.push(Value { class: Class::Mask, def: mask_position, last: sraw_position });
            tags.push("mask");
            Some(values.len() - 1)
        };
        let computed_last = sraw2_position.unwrap_or(fold_position);
        values.push(Value { class: Class::Computed, def: extract_position, last: computed_last });
        tags.push("computed");
        let computed_value_index = values.len() - 1;
        let carry_one_value_index = carry_one_range.map(|(def, last)| {
            values.push(Value { class: Class::Mask, def, last });
            tags.push("carry-one");
            values.len() - 1
        });
        // The shift local: last read = latest of the test and-op and any
        // andc/not mutation.
        let shift_last = if let Some(not_at) = not_position {
            not_at
        } else if let Some(at) = mutations
            .iter()
            .zip(&mutation_positions)
            .filter(|((_, m), _)| matches!(m, Mutation::AndcShift))
            .map(|(_, &at)| at)
            .max()
        {
            at
        } else {
            and_position
        };
        let shift_crosses = shift_last > branch_position;
        let shift_value_index = if shift_crosses {
            values.push(Value { class: Class::Shift, def: sraw_position, last: shift_last });
            tags.push("shift");
            Some(values.len() - 1)
        } else {
            None // r0 (branch-free single use)
        };
        let mut local_value_indices: Vec<Option<usize>> = vec![None; locals.len()];
        for index in 0..locals.len() {
            let Some(load) = load_positions[index] else { continue };
            // The home's last read.
            let mut last = load;
            if locals[index].0 == guard.source {
                last = last.max(extract_position);
            }
            if index == test_and_local {
                last = last.max(and_position);
            }
            if test_or_local == Some(index) {
                last = last.max(or_position.unwrap_or(and_position));
            }
            match &sign_block {
                Some(SignBlock::Add { local, .. }) if *local == index => {
                    // cmpwi + the add read/write the home inside the guard.
                    last = last.max(sraw2_position.expect("sign add") + 1);
                }
                Some(SignBlock::CarryDiamond { local, other, .. })
                    if *local == index || *other == index =>
                {
                    // The homes live through the whole diamond (the mr /
                    // the final addi).
                    last = last.max(sraw2_position.expect("carry") + 7);
                }
                _ => {}
            }
            let mutation = mutations
                .iter()
                .zip(&mutation_positions)
                .find(|((m, _), _)| *m == index);
            let class = match mutation {
                Some(((_, Mutation::Rewrite(_)), _)) if float_guard.is_none() => {
                    // The home dies at its last pre-branch read.
                    Class::LoadDiscarded
                }
                Some(((_, Mutation::Rewrite(_)), _)) => {
                    // A rewrite INSIDE the guard is conditional: the
                    // original flows to the store on the guard-false path.
                    last = last.max(store_positions[index]);
                    Class::LoadSurviving
                }
                Some(((_, Mutation::AndcShift), &at)) => {
                    // andc writes the home; the store reads it.
                    last = last.max(store_positions[index]);
                    let _ = at;
                    Class::LoadSurviving
                }
                Some(((_, Mutation::MaskViaScratch { .. }), &at)) => {
                    // clrlwi reads the home; the store reads r0.
                    last = last.max(at);
                    Class::LoadSurviving
                }
                None => {
                    last = last.max(store_positions[index]);
                    Class::LoadSurviving
                }
            };
            values.push(Value { class, def: load, last });
            tags.push("local");
            local_value_indices[index] = Some(values.len() - 1);
        }
        let registers = allocate(&values);
        let _ = &tags;
        let mask_register = mask_value_index.map(|i| registers[i]).unwrap_or(0);
        let guard_register = registers[computed_value_index];
        let shift_register = shift_value_index.map(|i| registers[i]).unwrap_or(0);
        let home = |index: usize| local_value_indices[index].map(|i| registers[i]);
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        if needs_temp {
            let temp_register = registers[0];
            let high = ((mask_constant + 0x8000) >> 16) as i16;
            let low = mask_constant as i16;
            self.output.instructions.push(Instruction::load_immediate_shifted(temp_register, high));
            self.output.instructions.push(Instruction::AddImmediate {
                d: mask_register,
                a: temp_register,
                immediate: low,
            });
        } else {
            self.output.instructions.push(Instruction::load_immediate(mask_register, mask_constant as i16));
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        for (index, &(_, offset)) in locals.iter().enumerate() {
            if load_positions[index].is_some() {
                self.output.instructions.push(Instruction::LoadWord {
                    d: home(index).expect("loaded"),
                    a: 1,
                    offset: 8 + offset,
                });
            }
        }
        let source_home = home(local_index(guard.source).expect("validated")).expect("source loads");
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: source_home,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: source_home,
                    shift: guard.shift,
                });
            }
        }
        let negative = i16::try_from(-guard.offset_k).expect("validated");
        let shift_amount = if guard_multi_read {
            // Multiple j0 reads: the -K lands in the home; an amount
            // offset (arm3's j0-20) folds separately into r0.
            self.output.instructions.push(Instruction::AddImmediate {
                d: guard_register,
                a: guard_register,
                immediate: negative,
            });
            if amount_offset != 0 {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 0,
                    a: guard_register,
                    immediate: i16::try_from(-amount_offset).expect("validated"),
                });
                0
            } else {
                guard_register
            }
        } else {
            self.output.instructions.push(Instruction::AddImmediate { d: 0, a: guard_register, immediate: negative });
            0
        };
        if logical_shift {
            self.output.instructions.push(Instruction::ShiftRightWord {
                a: shift_register,
                s: mask_register,
                b: shift_amount,
            });
        } else {
            self.output.instructions.push(Instruction::ShiftRightAlgebraicWord {
                a: shift_register,
                s: mask_register,
                b: shift_amount,
            });
        }
        // The test.
        let and_home = home(test_and_local).expect("test local loads");
        if let Some(or_local) = test_or_local {
            self.output.instructions.push(Instruction::And { a: 0, s: and_home, b: shift_register });
            self.output.instructions.push(Instruction::OrRecord {
                a: 0,
                s: home(or_local).expect("test local loads"),
                b: 0,
            });
        } else {
            self.output.instructions.push(Instruction::AndRecord { a: 0, s: and_home, b: shift_register });
        }
        let continuation = self.fresh_label();
        let epilogue = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, continuation); // bne — skip the return
        self.emit_branch_to(epilogue);
        self.bind_label(continuation);
        if let Some((huge, zero)) = float_guard {
            // The nested inexact guard (the G2 recipe): huge/0.0 pool-load
            // back-to-back into f2/f0, fadd clobbers the spilled f1, ble
            // chains to the join.
            self.load_double_constant(2, huge);
            self.load_double_constant(0, zero);
            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
            self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
            self.emit_branch_conditional_to(4, 1, join);
        }
        match &sign_block {
            Some(SignBlock::Add { local, .. }) => {
                // `if (l < 0) l += C2 >> j0` — C2 reuses the lis intermediate.
                let register = home(*local).expect("sign local loads");
                let temp_register = registers[0];
                let skip = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, skip); // bge
                self.output.instructions.push(Instruction::ShiftRightAlgebraicWord {
                    a: 0,
                    s: temp_register,
                    b: guard_register,
                });
                self.output.instructions.push(Instruction::Add { d: register, a: register, b: 0 });
                self.bind_label(skip);
            }
            Some(SignBlock::CarryDiamond { local, other, equal_bound, shift_base }) => {
                // `if (l < 0) { if (j0 == K4) l += 1; else { j = other +
                // (1 << (K3 - j0)); if (j < other) l += 1; other = j; } }`
                // — j lives in r0; the ONE constant takes a model register
                // (arm3: the dead mask's r3).
                let register = home(*local).expect("sign local loads");
                let other_register = home(*other).expect("carry source loads");
                let one_register = carry_one_value_index
                    .map(|i| registers[i])
                    .expect("carry one allocated");
                let continue_at = self.fresh_label(); // the trailing mutations
                let else_at = self.fresh_label();
                let no_carry = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, continue_at); // bge — skip the diamond
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: guard_register,
                    immediate: *equal_bound,
                });
                self.emit_branch_conditional_to(4, 2, else_at); // bne
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: register, immediate: 1 });
                self.emit_branch_to(continue_at);
                self.bind_label(else_at);
                self.output.instructions.push(Instruction::SubtractFromImmediate {
                    d: 0,
                    a: guard_register,
                    immediate: *shift_base,
                });
                self.output.instructions.push(Instruction::load_immediate(one_register, 1));
                self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: one_register, b: 0 });
                self.output.instructions.push(Instruction::Add { d: 0, a: other_register, b: 0 });
                self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: other_register });
                self.emit_branch_conditional_to(4, 0, no_carry); // bge — unsigned no-carry
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: register, immediate: 1 });
                self.bind_label(no_carry);
                self.output.instructions.push(Instruction::move_register(other_register, 0));
                self.bind_label(continue_at);
            }
            None => {}
        }
        // Mutations (the shared `not` precedes the first andc pair).
        if not_position.is_some() {
            self.output.instructions.push(Instruction::Nor { a: 0, s: shift_register, b: shift_register });
        }
        for (index, mutation) in &mutations {
            let index = *index;
            match mutation {
                Mutation::Rewrite(constant) => {
                    // Conditional (guarded) rewrites write the HOME — the
                    // original flows to the store on the guard-false path.
                    let target = if float_guard.is_some() {
                        home(index).expect("conditional rewrite loads")
                    } else {
                        0
                    };
                    self.output.instructions.push(Instruction::load_immediate(target, *constant));
                }
                Mutation::AndcShift => {
                    let register = home(index).expect("loaded");
                    if not_position.is_some() {
                        self.output.instructions.push(Instruction::And { a: register, s: register, b: 0 });
                    } else {
                        self.output.instructions.push(Instruction::AndComplement {
                            a: register,
                            s: register,
                            b: shift_register,
                        });
                    }
                }
                Mutation::MaskViaScratch { begin, end } => {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: 0,
                        s: home(index).expect("loaded"),
                        shift: 0,
                        begin: *begin,
                        end: *end,
                    });
                }
            }
        }
        // Stores (the guard's ble lands here): surviving homes store
        // themselves; UNCONDITIONAL rewrites and mask-via-scratch store
        // from r0.
        self.bind_label(join);
        for (index, &(_, offset)) in locals.iter().enumerate() {
            let from_scratch = float_guard.is_none()
                && mutations.iter().any(|&(m, ref form)| {
                    m == index && !matches!(form, Mutation::AndcShift)
                });
            let register = if from_scratch { 0 } else { home(index).map(|r| r).unwrap_or(0) };
            self.output.instructions.push(Instruction::StoreWord { s: register, a: 1, offset: 8 + offset });
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels: one plus one per LOADED local (measured V1c
        // @7 and W11 @7 with one load, V1b @8 with two — the never-read
        // store-only local costs nothing), plus one for the shared `not`
        // temp (W10 @9).
        self.output.anonymous_label_bump += 1
            + load_positions.iter().filter(|p| p.is_some()).count() as u32
            + not_position.is_some() as u32
            + 2 * float_guard.is_some() as u32
            + match &sign_block {
                Some(SignBlock::Add { .. }) => 2,
                // Three inner conditions (sign, ==K4, the carry compare)
                // at two each, one else arm, one for the ONE temp
                // (measured @18 on the arm3 object).
                Some(SignBlock::CarryDiamond { .. }) => 8,
                None => 0,
            };
        Ok(true)
    }

    /// The computed GUARD local `j0 = ((punned >> S) [& M]) - K` shared by
    /// the punned-writeback family (parsed once, consumed by the branch
    /// and select paths).
    pub(crate) fn try_punned_guard_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Every local: an int punned read of x at a distinct word offset —
        // or ONE computed GUARD local `j0 = ((punned >> S) [& M]) - K`
        // read only by the outer condition (s_floor's exponent extract).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard_local: Option<GuardLocal> = None;
        for local in &function.locals {
            if local.declared_type != Type::Int || local.array_length.is_some() {
                return Ok(false);
            }
            let Some(init) = &local.initializer else {
                return Ok(false);
            };
            if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                if locals.iter().any(|&(_, seen)| seen == offset) {
                    return Ok(false);
                }
                locals.push((local.name.as_str(), offset));
                continue;
            }
            // The computed guard local: strip a trailing `- K`.
            if guard_local.is_some() {
                return Ok(false);
            }
            let (core, offset_k) = match init {
                Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
                    let Some(k) = crate::analysis::constant_value(right) else {
                        return Ok(false);
                    };
                    (left.as_ref(), k)
                }
                other => (other, 0),
            };
            // `(punned >> S) & M` or bare `punned >> S`.
            let (shifted, mask) = match core {
                Expression::Binary { operator: BinaryOperator::BitAnd, left, right } => {
                    let Some(mask) = crate::analysis::constant_value(right) else {
                        return Ok(false);
                    };
                    (left.as_ref(), Some(mask))
                }
                other => (other, None),
            };
            let Expression::Binary { operator: BinaryOperator::ShiftRight, left, right } = shifted else {
                return Ok(false);
            };
            let Expression::Variable(source) = left.as_ref() else {
                return Ok(false);
            };
            let Some(shift) = crate::analysis::constant_value(right) else {
                return Ok(false);
            };
            let Ok(shift) = u8::try_from(shift) else {
                return Ok(false);
            };
            guard_local = Some(GuardLocal {
                name: local.name.as_str(),
                source,
                shift,
                mask,
                offset_k,
            });
        }
        if locals.is_empty() || locals.len() > 2 {
            return Ok(false);
        }
        if let Some(guard) = &guard_local {
            // The source must be a punned local; the guard local reads
            // nowhere else (its home holds only the pre-offset value).
            if !locals.iter().any(|&(name, _)| name == guard.source) {
                return Ok(false);
            }
        }
        // statements = [If{cond, [early-return-x if]? [mutations]}] + one
        // punned store per local writing it back to ITS offset.
        let (Some(Statement::If { condition, then_body, else_body }), stores) =
            (function.statements.first(), &function.statements[1..])
        else {
            return Ok(false);
        };
        if stores.len() != locals.len() {
            return Ok(false);
        }
        // The BLOCK: a recursive tree over the measured statement forms —
        // constant/high/self-mask mutations, nested no-else guards (chained
        // to the join), if/ELSE-IF arms (branch-over + b join), and
        // mid-chain `return x` (straight to the epilogue). Validated here;
        // emitted by the recursive walker below.
        let block: &[Statement] = then_body;
        fn validate_block(
            block: &[Statement],
            locals: &[(&str, i16)],
            x: &str,
            mutated: &mut Vec<usize>,
            conditions: &mut usize,
            arms: &mut usize,
        ) -> bool {
            for statement in block {
                match statement {
                    Statement::Assign { name, value } => {
                        let Some(index) = locals.iter().position(|&(local, _)| local == name.as_str()) else {
                            return false;
                        };
                        if !mutated.contains(&index) {
                            mutated.push(index);
                        }
                        // The chain `i0 = i1 = C`: both locals mutate from
                        // one small constant.
                        if let Expression::Assign { target, value: inner_value } = value {
                            let Expression::Variable(inner) = target.as_ref() else {
                                return false;
                            };
                            let Some(inner_index) =
                                locals.iter().position(|&(local, _)| local == inner.as_str())
                            else {
                                return false;
                            };
                            if !mutated.contains(&inner_index) {
                                mutated.push(inner_index);
                            }
                            if !crate::analysis::constant_value(inner_value)
                                .map(|constant| i16::try_from(constant).is_ok())
                                .unwrap_or(false)
                            {
                                return false;
                            }
                            continue;
                        }
                        let constant_ok = crate::analysis::constant_value(value)
                            .map(|constant| {
                                i16::try_from(constant).is_ok()
                                    || (constant & 0xffff == 0 && u32::try_from(constant).is_ok())
                            })
                            .unwrap_or(false);
                        if constant_ok {
                            continue;
                        }
                        let mask_ok = matches!(
                            value,
                            Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                                if matches!(left.as_ref(), Expression::Variable(read) if read == name.as_str())
                                    && crate::analysis::constant_value(right)
                                        .and_then(crate::analysis::rlwinm_mask)
                                        .is_some()
                        );
                        if !mask_ok {
                            return false;
                        }
                    }
                    Statement::Return(Some(Expression::Variable(value))) if value == x => {}
                    Statement::Return(Some(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    })) if matches!((left.as_ref(), right.as_ref()),
                        (Expression::Variable(a), Expression::Variable(b)) if a == x && b == x) => {}
                    Statement::If { condition: _, then_body, else_body } => {
                        *conditions += 1;
                        if !validate_block(then_body, locals, x, mutated, conditions, arms) {
                            return false;
                        }
                        if !else_body.is_empty() {
                            *arms += 1;
                            if !validate_block(else_body, locals, x, mutated, conditions, arms) {
                                return false;
                            }
                        }
                    }
                    _ => return false,
                }
            }
            true
        }
        let mut mutated: Vec<usize> = Vec::new();
        let mut inner_conditions = 0usize;
        let mut else_arms = 0usize;
        if !validate_block(block, &locals, x, &mut mutated, &mut inner_conditions, &mut else_arms) {
            return Ok(false);
        }
        if !else_body.is_empty() {
            else_arms += 1;
            if !validate_block(else_body, &locals, x, &mut mutated, &mut inner_conditions, &mut else_arms) {
                return Ok(false);
            }
        }
        if mutated.is_empty() {
            return Ok(false);
        }
        fn block_reads(block: &[Statement], name: &str) -> usize {
            block
                .iter()
                .map(|statement| match statement {
                    Statement::Assign { value, .. } => count_name_occurrences(value, name),
                    Statement::Return(Some(value)) => count_name_occurrences(value, name),
                    Statement::If { condition, then_body, else_body } => {
                        count_name_occurrences(condition, name)
                            + block_reads(then_body, name)
                            + block_reads(else_body, name)
                    }
                    _ => 0,
                })
                .sum()
        }
        fn block_condition_reads(block: &[Statement], name: &str) -> usize {
            block
                .iter()
                .map(|statement| match statement {
                    Statement::If { condition, then_body, else_body } => {
                        count_name_occurrences(condition, name)
                            + block_condition_reads(then_body, name)
                            + block_condition_reads(else_body, name)
                    }
                    _ => 0,
                })
                .sum()
        }
        fn block_self_masks(block: &[Statement], name: &str) -> bool {
            block.iter().any(|statement| match statement {
                Statement::Assign { name: target, value } => {
                    target.as_str() == name && crate::analysis::constant_value(value).is_none()
                }
                Statement::If { then_body, else_body, .. } => {
                    block_self_masks(then_body, name) || block_self_masks(else_body, name)
                }
                _ => false,
            })
        }
        // The writebacks: each local stored to its own offset, in order.
        for (statement, &(name, offset)) in stores.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset) {
                return Ok(false);
            }
            if !matches!(value, Expression::Variable(read) if read == name) {
                return Ok(false);
            }
        }
        // The FLOAT-compare guard: `HUGE + x > 0.0` (the static const
        // folded to a literal upstream) — measured: lfd huge BEFORE the
        // spill, fadd clobbering f1 (x is spilled), the pooled 0.0, the
        // loads woven before the fcmpo, ble skip.
        let float_guard: Option<(u64, u64)> = match condition {
            Expression::Binary { operator: BinaryOperator::Greater, left, right } => {
                let zero = match right.as_ref() {
                    Expression::FloatLiteral(value) => Some(value.to_bits()),
                    _ => None,
                };
                let huge = match left.as_ref() {
                    Expression::Binary { operator: BinaryOperator::Add, left: huge, right: xvar } => {
                        if matches!(xvar.as_ref(), Expression::Variable(name) if name == x) {
                            match huge.as_ref() {
                                Expression::FloatLiteral(value) => Some(value.to_bits()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                match (huge, zero) {
                    (Some(huge), Some(zero)) if f64::from_bits(zero) == 0.0 => Some((huge, zero)),
                    _ => None,
                }
            }
            _ => None,
        };
        if float_guard.is_some() && guard_local.is_some() {
            return Ok(false);
        }
        // The BRANCHLESS ZERO-SELECT: `if (j0 cmp K) p = A; else p = B;`
        // where one arm is 0 — 2.6 if-converts to mask algebra with no
        // branches at all (measured L3/L4/S2/S3/R1/R2/R3).
        if let Some(guard) = &guard_local {
            if locals.len() == 1
                && self.try_punned_zero_select(&locals, guard, condition, block, else_body)?
            {
                return Ok(true);
            }
            if locals.len() == 1
                && self.try_punned_hoisted_overwrite(&locals, guard, condition, block, else_body)?
            {
                return Ok(true);
            }
        }
        // The guard-local condition: `j0 < C` only (measured), with j0
        // read nowhere else in the function.
        let mut guard_compare: Option<(i16, i64)> = None;
        if let Some(guard) = &guard_local {

            let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition else {
                return Ok(false);
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
            let condition_reads = count_name_occurrences(condition, guard.name)
                + block_condition_reads(block, guard.name)
                + block_condition_reads(else_body, guard.name);
            let non_condition = block_reads(block, guard.name) - block_condition_reads(block, guard.name)
                + block_reads(else_body, guard.name)
                - block_condition_reads(else_body, guard.name)
                + stores
                    .iter()
                    .map(|statement| match statement {
                        Statement::Store { target, value } => {
                            count_name_occurrences(target, guard.name)
                                + count_name_occurrences(value, guard.name)
                        }
                        _ => 0,
                    })
                    .sum::<usize>();
            if non_condition != 0 {
                return Ok(false);
            }
            if condition_reads == 1 {
                // Single read: the -K folds into the scratch compare.
                guard_compare = Some((bound, guard.offset_k));
            }
            // Multi-read: the home takes the FULL value (addi into the
            // home, measured L1) and every condition reads it plainly.
        }
        // THE LIVENESS RULE (refines the old scratch rule; measured
        // P1/L1/L2 plus the eight 1054 shapes): r0 is denied to the
        // punned locals only when the r0 scratch is actually WRITTEN
        // (the single-read guard fold, a record-form idiom) while an
        // ORIGINAL loaded value is still live past the scratch point —
        // an arm reads it, or some writeback-reaching path skips
        // reassigning it so the stw reads it. L1's multi-read guard
        // (addi into the home, no fold) leaves r0 free; L2's
        // else-returns shape reassigns on every surviving path.
        fn condition_needs_scratch(condition: &Expression) -> bool {
            !matches!(
                condition,
                Expression::Variable(_)
                    | Expression::Binary { left: _, right: _, .. }
                        if matches!(condition, Expression::Variable(_))
                            || matches!(
                                condition,
                                Expression::Binary { left, right, .. }
                                    if matches!(left.as_ref(), Expression::Variable(_))
                                        && matches!(right.as_ref(), Expression::IntegerLiteral(_))
                            )
            )
        }
        fn block_needs_scratch(block: &[Statement]) -> bool {
            block.iter().any(|statement| match statement {
                Statement::If { condition, then_body, else_body } => {
                    condition_needs_scratch(condition)
                        || block_needs_scratch(then_body)
                        || block_needs_scratch(else_body)
                }
                _ => false,
            })
        }
        // Every leaf path either reassigns the local or leaves the
        // function before the writeback.
        fn covered(block: &[Statement], name: &str) -> bool {
            block.iter().any(|statement| match statement {
                Statement::Assign { name: target, .. } => target.as_str() == name,
                Statement::Return(_) => true,
                Statement::If { then_body, else_body, .. } => {
                    !else_body.is_empty() && covered(then_body, name) && covered(else_body, name)
                }
                _ => false,
            })
        }
        let scratch_written =
            guard_compare.is_some() || block_needs_scratch(block) || block_needs_scratch(else_body);
        let any_original_survives = locals.iter().any(|&(name, _)| {
            block_reads(block, name) + block_reads(else_body, name) > 0
                || !(covered(block, name) && !else_body.is_empty() && covered(else_body, name))
        });
        let scratch_taken = scratch_written && any_original_survives;
        let mut next_general = if guard_local.is_some() { 4u8 } else { 3u8 };
        let guard_register = 3u8;
        let mut registers: Vec<u8> = Vec::new();
        let mut r0_used = scratch_taken;
        for _ in &locals {
            if !r0_used {
                registers.push(0);
                r0_used = true;
            } else {
                registers.push(next_general);
                next_general += 1;
            }
        }
        // Live int params below the allocated range are unmeasured — every
        // capture either had none or had them freed by the outer condition.
        let top = registers.iter().copied().max().unwrap_or(0);
        for parameter in &function.parameters {
            if parameter.parameter_type == Type::Double {
                continue;
            }
            let Some(register) = self.lookup_general(&parameter.name) else {
                return Ok(false);
            };
            if register <= top && count_name_occurrences(condition, &parameter.name) == 0 {
                return Ok(false);
            }
        }
        // -- commit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        let hoisted = if guard_local.is_none() && float_guard.is_none() {
            Some(self.emit_condition_test(condition)?)
        } else {
            None
        };
        if let Some((huge, _)) = float_guard {
            // The huge pool load precedes the spill (measured).
            self.load_double_constant(0, huge);
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        if let Some((_, zero)) = float_guard {
            // fadd f1,f0,f1 clobbers x's register — the spill covers the
            // tail's reload; the pooled 0.0 loads before the int reads.
            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 0, b: 1 });
            self.load_double_constant(0, zero);
        }
        for (index, &(_, offset)) in locals.iter().enumerate() {
            self.output.instructions.push(Instruction::LoadWord { d: registers[index], a: 1, offset: 8 + offset });
        }
        if float_guard.is_some() {
            // No has_float_branch bump: the writeback's fcmpo+ble counts
            // only the arm's own labels (measured: pool @50 vs +3's @53).
            self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        }
        if let Some(guard) = &guard_local {
            // The guard local computes AFTER the loads: the fused shift+mask
            // (rlwinm) or plain srawi into its home; a SINGLE condition read
            // folds the -K into the scratch compare, MULTIPLE reads land the
            // full value in the home (measured L1's addi r3,r3,-1023).
            let source_register = locals
                .iter()
                .position(|&(name, _)| name == guard.source)
                .map(|index| registers[index])
                .expect("source is punned");
            match guard.mask {
                Some(mask) => {
                    let rotated = (32 - guard.shift as u32) % 32;
                    let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                        return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                    };
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: guard_register,
                        s: source_register,
                        shift: rotated as u8,
                        begin,
                        end,
                    });
                }
                None => {
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                        a: guard_register,
                        s: source_register,
                        shift: guard.shift,
                    });
                }
            }
            if guard_compare.is_none() && guard.offset_k != 0 {
                let Ok(negative) = i16::try_from(-guard.offset_k) else {
                    return Err(Diagnostic::error("guard offset beyond i16 (roadmap)"));
                };
                self.output.instructions.push(Instruction::AddImmediate {
                    d: guard_register,
                    a: guard_register,
                    immediate: negative,
                });
            }
        }
        let join = self.fresh_label();
        let epilogue = self.fresh_label();
        let outer_laddered = !else_body.is_empty() || (guard_local.is_some() && guard_compare.is_none());
        if outer_laddered && !(guard_local.is_some() && guard_compare.is_none()) {
            // Laddered forms are BYTE-verified only for the multi-read
            // guard (L1: the addi lands in the home and every condition
            // reads it plainly). A single-read fold or plain/float outer
            // condition inside the walker is unfitted (L2's inverted
            // else-return, the hoisted double-emission) — defer.
            return Ok(false);
        }
        if !outer_laddered {
            let (options, condition_bit) = match hoisted {
                Some(encoding) => encoding,
                None if float_guard.is_some() => (4, 1), // ble — the > 0.0 skip
                None => {
                    let (bound, offset_k) = guard_compare.expect("gated above");
                    if offset_k != 0 {
                        let Ok(negative) = i16::try_from(-offset_k) else {
                            return Err(Diagnostic::error("guard offset beyond i16 (roadmap)"));
                        };
                        if bound == 0 {
                            // A zero bound records the fold itself — the
                            // compare is free (measured G1: addic. r0; bge).
                            self.output.instructions.push(Instruction::AddImmediateCarryingRecord {
                                d: 0,
                                a: guard_register,
                                immediate: negative,
                            });
                        } else {
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: 0,
                                a: guard_register,
                                immediate: negative,
                            });
                            self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: bound });
                        }
                    } else {
                        self.output.instructions.push(Instruction::CompareWordImmediate {
                            a: guard_register,
                            immediate: bound,
                        });
                    }
                    (4, 0) // bge — the Less guard's skip sense
                }
            };
            self.emit_branch_conditional_to(options, condition_bit, join);
        }
        // The punned locals resolve in every inner condition through
        // temporary locations at their scratch registers, installed around
        // the whole block walk.
        let mut saved: Vec<(String, Option<crate::generator::Location>)> = Vec::new();
        for (index, &(name, _)) in locals.iter().enumerate() {
            saved.push((
                name.to_string(),
                self.locations.insert(
                    name.to_string(),
                    crate::generator::Location {
                        class: ValueClass::General,
                        register: registers[index],
                        signed: true,
                        width: 32,
                        pointee: None,
                        stride: None,
                    },
                ),
            ));
        }
        let mut bindings: Vec<(String, u8)> = locals
            .iter()
            .enumerate()
            .map(|(index, &(name, _))| (name.to_string(), registers[index]))
            .collect();
        if let Some(guard) = &guard_local {
            bindings.push((guard.name.to_string(), guard_register));
            saved.push((
                guard.name.to_string(),
                self.locations.insert(
                    guard.name.to_string(),
                    crate::generator::Location {
                        class: ValueClass::General,
                        register: guard_register,
                        signed: true,
                        width: 32,
                        pointee: None,
                        stride: None,
                    },
                ),
            ));
        }
        let outer_statement = [function.statements[0].clone()];
        let walked = if outer_laddered {
            self.emit_writeback_block(&outer_statement, &bindings, join, epilogue)
        } else {
            self.emit_writeback_block(block, &bindings, join, epilogue)
        };
        for (name, previous) in saved {
            match previous {
                Some(location) => {
                    self.locations.insert(name, location);
                }
                None => {
                    self.locations.remove(&name);
                }
            }
        }
        walked?;
        self.bind_label(join);
        for (index, &(_, offset)) in locals.iter().enumerate() {
            self.output.instructions.push(Instruction::StoreWord { s: registers[index], a: 1, offset: 8 + offset });
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels: the outer if pair, one per additional punned
        // local, two per inner condition, one per else arm (measured up to
        // the two-condition/one-arm forms; deeper shapes iterate). The
        // laddered outer costs one more (measured L1: @12 vs the +6
        // formula's @11), and each `return x+x` costs one (its expression
        // temp — measured M1 @16 and M4 @15 against the formula's -1).
        fn count_fadd_returns(block: &[Statement]) -> u32 {
            block
                .iter()
                .map(|statement| match statement {
                    Statement::Return(Some(Expression::Binary {
                        operator: BinaryOperator::Add,
                        ..
                    })) => 1,
                    Statement::If { then_body, else_body, .. } => {
                        count_fadd_returns(then_body) + count_fadd_returns(else_body)
                    }
                    _ => 0,
                })
                .sum()
        }
        self.output.anonymous_label_bump += 1
            + locals.len() as u32
            + 2 * inner_conditions as u32
            + else_arms as u32
            + outer_laddered as u32
            + count_fadd_returns(block)
            + count_fadd_returns(else_body);
        Ok(true)
    }

    /// The writeback block WALKER: mutations, tail guards chaining to the
    /// join, if/ELSE-IF arms, and mid-chain `return x` straight to the
    /// epilogue (measured: the N1/N2 nested captures).
    pub(crate) fn emit_writeback_block(
        &mut self,
        block: &[Statement],
        bindings: &[(String, u8)],
        join: mwcc_vreg::Label,
        epilogue: mwcc_vreg::Label,
    ) -> Compilation<()> {
        use mwcc_syntax_trees::Statement;
        let mut index = 0usize;
        while index < block.len() {
            let statement = &block[index];
            let last = index + 1 == block.len();
            match statement {
                Statement::Assign { name, value } => {
                    let register = bindings
                        .iter()
                        .find(|(local, _)| local == name)
                        .map(|&(_, register)| register)
                        .expect("validated");
                    // The chain `i0 = i1 = C` assigns right-to-left: the
                    // inner local first, then the outer from the same
                    // constant (measured G1: li r5,0; li r4,0).
                    if let Expression::Assign { target, value: inner_value } = value {
                        let Expression::Variable(inner) = target.as_ref() else {
                            return Err(Diagnostic::error("chained store target beyond the walker (roadmap)"));
                        };
                        let inner_register = bindings
                            .iter()
                            .find(|(local, _)| local == inner)
                            .map(|&(_, register)| register)
                            .expect("validated");
                        let constant = crate::analysis::constant_value(inner_value).expect("validated");
                        let small = i16::try_from(constant).expect("validated");
                        self.output.instructions.push(Instruction::load_immediate(inner_register, small));
                        self.output.instructions.push(Instruction::load_immediate(register, small));
                        index += 1;
                        continue;
                    }
                    if let Some(constant) = crate::analysis::constant_value(value) {
                        if let Ok(small) = i16::try_from(constant) {
                            self.output.instructions.push(Instruction::load_immediate(register, small));
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::load_immediate_shifted(register, (constant >> 16) as i16));
                        }
                    } else if let Expression::Binary { operator: BinaryOperator::BitAnd, right, .. } = value {
                        let mask = crate::analysis::constant_value(right).expect("validated");
                        let (begin, end) = crate::analysis::rlwinm_mask(mask).expect("validated");
                        self.output.instructions.push(Instruction::RotateAndMask {
                            a: register,
                            s: register,
                            shift: 0,
                            begin,
                            end,
                        });
                    } else {
                        return Err(Diagnostic::error("writeback mutation beyond the walker (roadmap)"));
                    }
                }
                Statement::Return(Some(value)) => {
                    // `return x+x` raises inexact/inf via fadd before the
                    // epilogue (measured M1: fadd f1,f1,f1; b epi); f1 is
                    // never clobbered on walker paths, so a plain return
                    // is the bare branch.
                    if let Expression::Binary { operator: BinaryOperator::Add, left, right } = value {
                        if matches!((left.as_ref(), right.as_ref()),
                            (Expression::Variable(a), Expression::Variable(b)) if a == b)
                        {
                            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 1 });
                        }
                    }
                    self.emit_branch_to(epilogue);
                }
                Statement::If { condition, then_body, else_body } => {
                    if let Some((huge, zero)) = float_guard_condition(condition) {
                        // The NESTED inexact guard (measured G2): huge and
                        // 0.0 pool-load back-to-back into f2/f0, the fadd
                        // clobbers f1 (x stays spilled), ble chains to the
                        // join like any tail guard.
                        if !else_body.is_empty() || !last {
                            return Err(Diagnostic::error(
                                "a non-tail float guard in the walker (roadmap)",
                            ));
                        }
                        self.load_double_constant(2, huge);
                        self.load_double_constant(0, zero);
                        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
                        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
                        self.emit_branch_conditional_to(4, 1, join);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        index += 1;
                        continue;
                    }
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    if let [Statement::Return(Some(_))] = else_body.as_slice() {
                        if matches!(then_body.last(), Some(Statement::Return(_))) {
                            // BOTH arms leave: the else's b-epilogue folds
                            // into the skip branch itself (measured M1:
                            // cmpwi; bne EPI; fadd; b EPI).
                            self.emit_branch_conditional_to(options, condition_bit, epilogue);
                            self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                            index += 1;
                            continue;
                        }
                        // The then FALLS to the join: the arms swap — the
                        // taken sense enters the then arm, the return lands
                        // inline as b epilogue (measured L2: blt; b epi;
                        // muts).
                        let continuation = self.fresh_label();
                        self.emit_branch_conditional_to(options ^ 8, condition_bit, continuation);
                        self.emit_branch_to(epilogue);
                        self.bind_label(continuation);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        index += 1;
                        continue;
                    }
                    if !else_body.is_empty() {
                        // if/ELSE-IF: branch over the then arm; b join after
                        // it — omitted when every then path already leaves
                        // (measured M1: fadd; b epi; ELSE with no b join).
                        fn block_leaves(block: &[Statement]) -> bool {
                            match block.last() {
                                Some(Statement::Return(_)) => true,
                                Some(Statement::If { then_body, else_body, .. }) => {
                                    !else_body.is_empty()
                                        && block_leaves(then_body)
                                        && block_leaves(else_body)
                                }
                                _ => false,
                            }
                        }
                        let else_label = self.fresh_label();
                        self.emit_branch_conditional_to(options, condition_bit, else_label);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        if !block_leaves(then_body) {
                            self.emit_branch_to(join);
                        }
                        self.bind_label(else_label);
                        self.emit_writeback_block(else_body, bindings, join, epilogue)?;
                    } else if let [Statement::Return(Some(_))] = then_body.as_slice() {
                        // The mid-chain return: skip to the continuation.
                        // The recursion supplies the return emission (the
                        // bare b epilogue, or fadd first for x+x).
                        let continuation = self.fresh_label();
                        self.emit_branch_conditional_to(options, condition_bit, continuation);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        self.bind_label(continuation);
                    } else if last {
                        // A tail guard chains to the block's join.
                        self.emit_branch_conditional_to(options, condition_bit, join);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                    } else {
                        return Err(Diagnostic::error("a non-tail guard in the writeback (roadmap)"));
                    }
                }
                _ => return Err(Diagnostic::error("writeback statement beyond the walker (roadmap)")),
            }
            index += 1;
        }
        Ok(())
    }

}
