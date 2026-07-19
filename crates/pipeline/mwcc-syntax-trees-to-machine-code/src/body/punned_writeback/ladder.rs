//! Punned-double LADDER writeback: the s_floor-style three-arm ladder over punned int locals.

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
        let [Statement::If {
            condition: ladder1,
            then_body: low_arm,
            else_body: high_arm,
        }, store_statements @ ..] = function.statements.as_slice()
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
        let parse_guard_compare =
            |condition: &Expression, operator: BinaryOperator| -> Option<i16> {
                let Expression::Binary {
                    operator: op,
                    left,
                    right,
                } = condition
                else {
                    return None;
                };
                if *op != operator
                    || !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name)
                {
                    return None;
                }
                crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
            };
        let Some(k1) = parse_guard_compare(ladder1, BinaryOperator::Less) else {
            return Ok(false);
        };
        // low_arm = [If{j0<0, arm1, arm2}].
        let [Statement::If {
            condition: split,
            then_body: arm1,
            else_body: arm2,
        }] = low_arm.as_slice()
        else {
            return Ok(false);
        };
        if parse_guard_compare(split, BinaryOperator::Less) != Some(0) {
            return Ok(false);
        }
        // high_arm = [If{j0>K2, mid, arm3}].
        let [Statement::If {
            condition: ladder2,
            then_body: mid,
            else_body: arm3,
        }] = high_arm.as_slice()
        else {
            return Ok(false);
        };
        let Some(k2) = parse_guard_compare(ladder2, BinaryOperator::Greater) else {
            return Ok(false);
        };
        // mid = [If{j0==K3, [Return x+x], [Return x]}].
        let [Statement::If {
            condition: mid_cond,
            then_body: mid_then,
            else_body: mid_else,
        }] = mid.as_slice()
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
        let [Statement::If {
            condition: guard1_cond,
            then_body: guard1_body,
            else_body: guard1_else,
        }] = arm1.as_slice()
        else {
            return Ok(false);
        };
        let Some((huge_bits, zero_bits)) = float_guard_condition(guard1_cond) else {
            return Ok(false);
        };
        if !guard1_else.is_empty() {
            return Ok(false);
        }
        let [Statement::If {
            condition: sign1,
            then_body: sign1_then,
            else_body: sign1_else,
        }] = guard1_body.as_slice()
        else {
            return Ok(false);
        };
        // The sign comparison: `i0 >= 0` (s_floor) or `i0 < 0` (s_ceil) —
        // the emitted branch is the inverted sense to the else arm either
        // way.
        let Expression::Binary {
            operator: sign1_op,
            left: sign1_l,
            right: sign1_r,
        } = sign1
        else {
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
                [Statement::Assign {
                    name,
                    value: Expression::Assign { target, value },
                }] if local_index(name) == Some(i0)
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
                    let representable =
                        |constant: i64| i16::try_from(constant).is_ok() || constant & 0xffff == 0;
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
        let [Statement::If {
            condition: mag_cond,
            then_body: mag_then,
            else_body: mag_else,
        }] = sign1_else.as_slice()
        else {
            return Ok(false);
        };
        if !mag_else.is_empty() {
            return Ok(false);
        }
        let Some(mag_mask) = (|| {
            let Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } = mag_cond
            else {
                return None;
            };
            if crate::analysis::constant_value(right) != Some(0) {
                return None;
            }
            let Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: or_l,
                right: or_r,
            } = left.as_ref()
            else {
                return None;
            };
            if !matches!(or_r.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)) {
                return None;
            }
            match or_l.as_ref() {
                Expression::Variable(v) if local_index(v) == Some(i0) => Some(None),
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left: and_l,
                    right: and_r,
                } if matches!(and_l.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0)) =>
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
        let Some(ConstPair::Pair {
            first: mag_first,
            second: mag_second,
        }) = parse_pair(mag_then)
        else {
            return Ok(false);
        };
        // ARM2 (fire 399): [i = C >> j0, If{test, [Ret x]}, If{huge, [If{i0<0, [i0 += C2>>j0]}, i0 &= ~i, i1 = 0]}].
        let [Statement::Assign {
            name: a2_shift_name,
            value: a2_shift_value,
        }, Statement::If {
            condition: a2_test,
            then_body: a2_ret,
            else_body: a2_test_else,
        }, Statement::If {
            condition: a2_guard,
            then_body: a2_guard_body,
            else_body: a2_guard_else,
        }] = arm2.as_slice()
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
        if !matches!(a2_ret.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x)
        {
            return Ok(false);
        }
        // test: ((i0 & i) | i1) == 0
        let a2_test_ok = (|| {
            let Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } = a2_test
            else {
                return false;
            };
            if crate::analysis::constant_value(right) != Some(0) {
                return false;
            }
            let Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: or_l,
                right: or_r,
            } = left.as_ref()
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
        let [Statement::If {
            condition: a2_sign,
            then_body: a2_add,
            else_body: a2_sign_else,
        }, Statement::Assign {
            name: a2_andc_name,
            value: a2_andc_value,
        }, Statement::Assign {
            name: a2_rw_name,
            value: a2_rw_value,
        }] = a2_guard_body.as_slice()
        else {
            return Ok(false);
        };
        let parse_sign = |condition: &Expression| -> Option<(u8, u8)> {
            let Expression::Binary {
                operator,
                left,
                right,
            } = condition
            else {
                return None;
            };
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
        let [Statement::Assign {
            name: a3_shift_name,
            value: a3_shift_value,
        }, Statement::If {
            condition: a3_test,
            then_body: a3_ret,
            else_body: a3_test_else,
        }, Statement::If {
            condition: a3_guard,
            then_body: a3_guard_body,
            else_body: a3_guard_else,
        }] = arm3.as_slice()
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
        let (Ok(a3_mask_small), Ok(a3_offset_neg)) =
            (i16::try_from(a3_mask), i16::try_from(-a3_offset))
        else {
            return Ok(false);
        };
        if !a3_logical || a3_offset == 0 {
            return Ok(false);
        }
        if !matches!(a3_ret.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x)
        {
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
        let [Statement::If {
            condition: a3_sign,
            then_body: a3_diamond,
            else_body: a3_sign_else,
        }, Statement::Assign {
            name: a3_andc_name,
            value: a3_andc_value,
        }] = a3_guard_body.as_slice()
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
        let [Statement::If {
            condition: eq_cond,
            then_body: eq_then,
            else_body: eq_else,
        }] = a3_diamond.as_slice()
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
        let [Statement::Assign {
            name: j_name,
            value: j_value,
        }, Statement::If {
            condition: carry_cond,
            then_body: carry_then,
            else_body: carry_else,
        }, Statement::Assign {
            name: copy_name,
            value: copy_value,
        }] = eq_else.as_slice()
        else {
            return Ok(false);
        };
        let Some(k6) = (|| {
            if j_name != carry {
                return None;
            }
            let Expression::Binary {
                operator: BinaryOperator::Add,
                left: base,
                right: one_shift,
            } = j_value
            else {
                return None;
            };
            if !matches!(base.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)) {
                return None;
            }
            let Expression::Binary {
                operator: BinaryOperator::ShiftLeft,
                left: one,
                right: amount,
            } = one_shift.as_ref()
            else {
                return None;
            };
            if crate::analysis::constant_value(one) != Some(1) {
                return None;
            }
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: k6,
                right: by,
            } = amount.as_ref()
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
            Value {
                class: Class::Temp,
                def: 4,
                last: 5,
            },
            Value {
                class: Class::Temp,
                def: arm2_base,
                last: arm2_base + 14,
            }, // lis..sraw2 (CSE)
            Value {
                class: Class::Mask,
                def: arm3_base + 1,
                last: arm3_base + 2,
            },
            Value {
                class: Class::Mask,
                def: arm3_base + 18,
                last: arm3_base + 19,
            }, // the ONE
            Value {
                class: Class::Scrutinee,
                def: 5,
                last: arm3_base + 17,
            }, // ..subfic
            Value {
                class: Class::LoadSurviving,
                def: 2,
                last: join_at,
            },
            Value {
                class: Class::LoadSurviving,
                def: 3,
                last: join_at + 1,
            },
            Value {
                class: Class::ArmShift,
                def: arm2_base + 2,
                last: arm2_base + 16,
            },
            Value {
                class: Class::ArmShift,
                def: arm3_base + 2,
                last: arm3_base + 25,
            },
        ];
        let registers = allocate(&values);
        let legacy_roles =
            policy::legacy_ladder_registers(self.behavior.punned_shift_writeback_style);
        let extract_temp = legacy_roles
            .as_ref()
            .map(|roles| roles.extract)
            .unwrap_or(registers[0]);
        let a2_temp = legacy_roles
            .as_ref()
            .map(|roles| roles.arm_temp)
            .unwrap_or(registers[1]);
        let a3_mask_reg = legacy_roles
            .as_ref()
            .map(|roles| roles.arm_mask)
            .unwrap_or(registers[2]);
        let one_reg = legacy_roles
            .as_ref()
            .map(|roles| roles.carry_one)
            .unwrap_or(registers[3]);
        let j0_reg = legacy_roles
            .as_ref()
            .map(|roles| roles.scrutinee)
            .unwrap_or(registers[4]);
        let i0_reg = legacy_roles
            .as_ref()
            .map(|roles| roles.source_home)
            .unwrap_or(if i0 == 0 { registers[5] } else { registers[6] });
        let i1_reg = legacy_roles
            .as_ref()
            .map(|roles| roles.other)
            .unwrap_or(if i0 == 0 { registers[6] } else { registers[5] });
        let source_load = legacy_roles
            .as_ref()
            .map(|roles| roles.source_load)
            .unwrap_or(i0_reg);
        let a2_i = legacy_roles
            .as_ref()
            .map(|roles| roles.arm_shift)
            .unwrap_or(registers[7]);
        let a3_i = legacy_roles
            .as_ref()
            .map(|roles| roles.arm_shift)
            .unwrap_or(registers[8]);
        // NB: loads emit in frame-offset order = locals order; registers[5]
        // belongs to locals[0].
        let load0 = if i0 == 0 { source_load } else { i1_reg };
        let load1 = if i0 == 0 { i1_reg } else { source_load };
        let store0 = if i0 == 0 { i0_reg } else { i1_reg };
        let store1 = if i0 == 0 { i1_reg } else { i0_reg };
        let legacy_reloading = legacy_roles.is_some();
        // -- emit --
        self.frame_size = 16;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: load0,
            a: 1,
            offset: 8 + locals[0].1,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: load1,
            a: 1,
            offset: 8 + locals[1].1,
        });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: extract_temp,
                    s: source_load,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: extract_temp,
                        s: source_load,
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
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: j0_reg,
                immediate: k1,
            });
        if source_load != i0_reg {
            self.output
                .instructions
                .push(Instruction::move_register(i0_reg, source_load));
        }
        self.emit_branch_conditional_to(4, 0, ladder2_at); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: j0_reg,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 0, arm2_at); // bge
                                                        // ARM1.
        self.load_double_constant(2, huge_bits);
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        self.load_double_constant(0, zero_bits);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let arm1_else = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: i0_reg,
                immediate: 0,
            });
        self.emit_branch_conditional_to(sign1_branch.0, sign1_branch.1, arm1_else);
        let emit_constant = |generator: &mut Self, register: u8, constant: i64| {
            if let Ok(small) = i16::try_from(constant) {
                generator
                    .output
                    .instructions
                    .push(Instruction::load_immediate(register, small));
            } else {
                generator
                    .output
                    .instructions
                    .push(Instruction::load_immediate_shifted(
                        register,
                        (constant >> 16) as i16,
                    ));
            }
        };
        match &sign1_pair {
            ConstPair::Chained0 => {
                // The chained `i0 = i1 = 0` assigns inner-first.
                self.output
                    .instructions
                    .push(Instruction::load_immediate(i1_reg, 0));
                self.output
                    .instructions
                    .push(Instruction::load_immediate(i0_reg, 0));
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
            self.output.instructions.push(Instruction::OrRecord {
                a: 0,
                s: 0,
                b: i1_reg,
            });
        } else {
            self.output.instructions.push(Instruction::OrRecord {
                a: 0,
                s: i0_reg,
                b: i1_reg,
            });
        }
        self.emit_branch_conditional_to(12, 2, join); // beq
        emit_constant(self, i0_reg, mag_first);
        emit_constant(self, i1_reg, mag_second);
        self.emit_branch_to(join);
        // ARM2.
        self.bind_label(arm2_at);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(
                a2_temp,
                (a2_lis >> 16) as i16,
            ));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: a2_temp,
            immediate: a2_mask as i16,
        });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicWord {
                a: a2_i,
                s: 0,
                b: j0_reg,
            });
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: i0_reg,
            b: a2_i,
        });
        self.output.instructions.push(Instruction::OrRecord {
            a: 0,
            s: i1_reg,
            b: 0,
        });
        let a2_cont = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a2_cont); // bne
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        self.emit_branch_to(epilogue);
        self.bind_label(a2_cont);
        self.load_double_constant(2, huge_bits);
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        self.load_double_constant(0, zero_bits);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let a2_skip = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: i0_reg,
                immediate: 0,
            });
        self.emit_branch_conditional_to(a2_sign_branch.0, a2_sign_branch.1, a2_skip);
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicWord {
                a: 0,
                s: a2_temp,
                b: j0_reg,
            });
        self.output.instructions.push(Instruction::Add {
            d: i0_reg,
            a: i0_reg,
            b: 0,
        });
        self.bind_label(a2_skip);
        self.output.instructions.push(Instruction::AndComplement {
            a: i0_reg,
            s: i0_reg,
            b: a2_i,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(i1_reg, 0));
        self.emit_branch_to(join);
        // LADDER 2 + MID.
        self.bind_label(ladder2_at);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: j0_reg,
                immediate: k2,
            });
        self.emit_branch_conditional_to(4, 1, arm3_at); // ble
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: j0_reg,
                immediate: k3,
            });
        if legacy_reloading {
            let mid_return = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, mid_return); // bne — return x
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 8,
            });
            self.output
                .instructions
                .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 0 });
            self.emit_branch_to(epilogue);
            self.bind_label(mid_return);
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
            self.emit_branch_to(epilogue);
        } else {
            self.emit_branch_conditional_to(4, 2, epilogue); // bne — return x
            self.output
                .instructions
                .push(Instruction::FloatAddDouble { d: 1, a: 1, b: 1 });
            self.emit_branch_to(epilogue);
        }
        // ARM3.
        self.bind_label(arm3_at);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: j0_reg,
            immediate: a3_offset_neg,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(a3_mask_reg, a3_mask_small));
        self.output.instructions.push(Instruction::ShiftRightWord {
            a: a3_i,
            s: a3_mask_reg,
            b: 0,
        });
        self.output.instructions.push(Instruction::AndRecord {
            a: 0,
            s: i1_reg,
            b: a3_i,
        });
        let a3_cont = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a3_cont); // bne
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        self.emit_branch_to(epilogue);
        self.bind_label(a3_cont);
        self.load_double_constant(2, huge_bits);
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        self.load_double_constant(0, zero_bits);
        self.output
            .instructions
            .push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let a3_andc = self.fresh_label();
        let a3_carry = self.fresh_label();
        let a3_no_carry = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: i0_reg,
                immediate: 0,
            });
        self.emit_branch_conditional_to(a3_sign_branch.0, a3_sign_branch.1, a3_andc);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: j0_reg,
                immediate: k5,
            });
        self.emit_branch_conditional_to(4, 2, a3_carry); // bne
        self.output.instructions.push(Instruction::AddImmediate {
            d: i0_reg,
            a: i0_reg,
            immediate: 1,
        });
        self.emit_branch_to(a3_andc);
        self.bind_label(a3_carry);
        self.output
            .instructions
            .push(Instruction::SubtractFromImmediate {
                d: 0,
                a: j0_reg,
                immediate: k6,
            });
        self.output
            .instructions
            .push(Instruction::load_immediate(one_reg, 1));
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: 0,
            s: one_reg,
            b: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: 0,
            a: i1_reg,
            b: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: i1_reg });
        self.emit_branch_conditional_to(4, 0, a3_no_carry); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: i0_reg,
            a: i0_reg,
            immediate: 1,
        });
        self.bind_label(a3_no_carry);
        self.output
            .instructions
            .push(Instruction::move_register(i1_reg, 0));
        self.bind_label(a3_andc);
        self.output.instructions.push(Instruction::AndComplement {
            a: i1_reg,
            s: i1_reg,
            b: a3_i,
        });
        // JOIN + EPI.
        self.bind_label(join);
        self.output.instructions.push(Instruction::StoreWord {
            s: store0,
            a: 1,
            offset: 8 + locals[0].1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: store1,
            a: 1,
            offset: 8 + locals[1].1,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 8,
        });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // Pre-pool labels: mainline advances 40; build 163 retains four
        // additional ladder-edge slots before the shared double constants.
        // Deferred compilation retains another 36 internal CFG labels in every
        // measured generation (builds 163/53/81 and the 2.4.7 mainline) without
        // changing the emitted instruction stream.
        let deferred_label_bump = if self.behavior.deferred_inlining {
            36
        } else {
            0
        };
        self.output.anonymous_label_bump += 40
            + legacy_roles
                .as_ref()
                .map(|roles| roles.constant_label_bump)
                .unwrap_or(0)
            + deferred_label_bump;
        Ok(true)
    }
}
