//! Count-register (CTR / bdnz) loop families, including the rotated form.
//!
//! Split from a single 2795-line `loops.rs` (behavior-identical).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// The CTR LOOP (fire 419, e_fmod's `while(n--)` walker): a counted
    /// loop whose body BRANCHES escapes the ×8 unroll entirely — mwcc
    /// emits `mtctr n; cmpwi n,0; beq(lr); BODY; bdnz BODY`. The skip
    /// branch mirrors the entry test exactly: `while(n--)` skips only on
    /// n==0 (a negative n runs 2^32 times, and the unsigned CTR does
    /// too — faithful). Captured micro-shape: `hz = hx - K` fuses into
    /// `addic. r0` (the condition-only computed rides r0 through the
    /// arm), the diamond writes the param home directly in both arms,
    /// and post-loop code takes `beq END` instead of `beqlr`. The
    /// `for(i<n)` variant if-converts its diamond differently (eager
    /// else + `mr` join) and is NOT claimed here; straight-line bodies
    /// take the ×8 unroll machinery (deferred, the counted gate).
    pub(crate) fn try_ctr_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(kind, LoopKind::While | LoopKind::DoWhile) {
            return Ok(false);
        }
        // The condition: a bare `n--` of an int parameter.
        let Expression::PostStep {
            target,
            operator: BinaryOperator::Subtract,
        } = condition
        else {
            return Ok(false);
        };
        let Expression::Variable(count) = target.as_ref() else {
            return Ok(false);
        };
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *count && parameter.parameter_type == Type::Int)
        {
            return Ok(false);
        }
        // The body: `hz = hx - K; if (hz < 0) hx = hx + hx; else hx = hz + hz;`
        let [Statement::Assign {
            name: hz,
            value: hz_value,
        }, Statement::If {
            condition: test,
            then_body,
            else_body,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } = hz_value
        else {
            return Ok(false);
        };
        let Expression::Variable(hx) = left.as_ref() else {
            return Ok(false);
        };
        // The head: `hx - K` folds into `addic. r0` (fire 419); `hx - hy`
        // (hy an int parameter) into `subf. r0, hy, hx` (fire 420).
        enum Head {
            Immediate(i16),
            Register(String),
        }
        let head =
            match right.as_ref() {
                Expression::IntegerLiteral(k) => {
                    let Ok(negated_k) = i16::try_from(-*k) else {
                        return Ok(false);
                    };
                    Head::Immediate(negated_k)
                }
                Expression::Variable(hy) if hy != hx && hy != hz && hy != count => {
                    if !function.parameters.iter().any(|parameter| {
                        parameter.name == *hy && parameter.parameter_type == Type::Int
                    }) {
                        return Ok(false);
                    }
                    Head::Register(hy.clone())
                }
                _ => return Ok(false),
            };
        if hx == hz || hx == count || hz == count {
            return Ok(false);
        }
        // hx: an int parameter sitting in r3 (every capture); hz: an int
        // local or a dead-on-entry int parameter (its home is never
        // touched — the value rides r0).
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *hx && parameter.parameter_type == Type::Int)
        {
            return Ok(false);
        }
        let hz_is_local = function.locals.iter().any(|local| {
            local.name == *hz && local.declared_type == Type::Int && local.initializer.is_none()
        });
        let hz_is_dead_parameter = function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *hz && parameter.parameter_type == Type::Int);
        if !hz_is_local && !hz_is_dead_parameter {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left: test_left,
            right: test_right,
        } = test
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(test_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        let doubles_into = |statement: &Statement, target: &str, doubled: &str| -> bool {
            let Statement::Assign { name, value } = statement else {
                return false;
            };
            if name != target {
                return false;
            }
            let Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } = value
            else {
                return false;
            };
            matches!(left.as_ref(), Expression::Variable(v) if v == doubled)
                && matches!(right.as_ref(), Expression::Variable(v) if v == doubled)
        };
        // The then arm: `hx = hx + hx;` (double, fire 419) or the PAIR
        // CARRY STEP `hx = hx + hx + (lx >> 31); lx = lx + lx;` (fire
        // 420, e_fmod's 2-word left shift — the srwi leads, the LOW
        // doubling schedules between it and the two adds, which
        // associate hx + (hx + carry)). The low word must be UNSIGNED
        // (a signed one would srawi).
        enum ThenArm {
            Double,
            PairStep(String),
        }
        let then_arm = match then_body.as_slice() {
            [single] if doubles_into(single, hx, hx) => ThenArm::Double,
            [Statement::Assign {
                name: high_name,
                value: high_value,
            }, low_step] => {
                if high_name != hx {
                    return Ok(false);
                }
                let Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: sum,
                    right: carry,
                } = high_value
                else {
                    return Ok(false);
                };
                let Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: first,
                    right: second,
                } = sum.as_ref()
                else {
                    return Ok(false);
                };
                if !matches!(first.as_ref(), Expression::Variable(v) if v == hx)
                    || !matches!(second.as_ref(), Expression::Variable(v) if v == hx)
                {
                    return Ok(false);
                }
                let Expression::Binary {
                    operator: BinaryOperator::ShiftRight,
                    left: low,
                    right: amount,
                } = carry.as_ref()
                else {
                    return Ok(false);
                };
                let Expression::Variable(lx) = low.as_ref() else {
                    return Ok(false);
                };
                if !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
                    || lx == hx
                    || lx == hz
                    || lx == count
                    || matches!(&head, Head::Register(hy) if lx == hy)
                    || !doubles_into(low_step, lx, lx)
                    || !function.parameters.iter().any(|parameter| {
                        parameter.name == *lx && parameter.parameter_type == Type::UnsignedInt
                    })
                {
                    return Ok(false);
                }
                ThenArm::PairStep(lx.clone())
            }
            _ => return Ok(false),
        };
        let [else_single] = else_body.as_slice() else {
            return Ok(false);
        };
        if !doubles_into(else_single, hx, hz) {
            return Ok(false);
        }
        // The tail: `return hx` (skip = beqlr) or `return hx + K2` (skip =
        // beq END). Both captured with hx in r3.
        enum Tail {
            Home,
            AddImmediate(i16),
        }
        let tail = match &function.return_expression {
            Some(Expression::Variable(v)) if v == hx => Tail::Home,
            Some(Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            }) if matches!(left.as_ref(), Expression::Variable(v) if v == hx) => {
                let Expression::IntegerLiteral(k2) = right.as_ref() else {
                    return Ok(false);
                };
                let Ok(k2) = i16::try_from(*k2) else {
                    return Ok(false);
                };
                Tail::AddImmediate(k2)
            }
            _ => return Ok(false),
        };
        let Some(hx_register) = self.lookup_general(hx) else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        let Some(count_register) = self.lookup_general(count) else {
            return Ok(false);
        };
        let head_register = match &head {
            Head::Immediate(_) => None,
            Head::Register(hy) => match self.lookup_general(hy) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
        };
        let pair_low_register = match &then_arm {
            ThenArm::Double => None,
            ThenArm::PairStep(lx) => match self.lookup_general(lx) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
        };
        // -- emit --
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: count_register });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: count_register,
                immediate: 0,
            });
        let end_label = self.fresh_label();
        match tail {
            Tail::Home => {
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options: 12,
                        condition_bit: 2,
                    })
            }
            Tail::AddImmediate(_) => self.emit_branch_conditional_to(12, 2, end_label),
        }
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        match &head {
            Head::Immediate(negated_k) => {
                self.output
                    .instructions
                    .push(Instruction::AddImmediateCarryingRecord {
                        d: 0,
                        a: hx_register,
                        immediate: *negated_k,
                    })
            }
            Head::Register(_) => self
                .output
                .instructions
                .push(Instruction::SubtractFromRecord {
                    d: 0,
                    a: head_register.unwrap(),
                    b: hx_register,
                }),
        }
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        match &then_arm {
            ThenArm::Double => {
                self.output.instructions.push(Instruction::Add {
                    d: hx_register,
                    a: hx_register,
                    b: hx_register,
                });
            }
            ThenArm::PairStep(_) => {
                let low = pair_low_register.unwrap();
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: 0,
                        s: low,
                        shift: 31,
                    });
                self.output.instructions.push(Instruction::Add {
                    d: low,
                    a: low,
                    b: low,
                });
                self.output.instructions.push(Instruction::Add {
                    d: 0,
                    a: hx_register,
                    b: 0,
                });
                self.output.instructions.push(Instruction::Add {
                    d: hx_register,
                    a: hx_register,
                    b: 0,
                });
            }
        }
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        self.output.instructions.push(Instruction::Add {
            d: hx_register,
            a: 0,
            b: 0,
        });
        self.bind_label(join_label);
        self.emit_branch_conditional_to(16, 0, body_label); // bdnz
        if let Tail::AddImmediate(k2) = tail {
            self.bind_label(end_label);
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: hx_register,
                immediate: k2,
            });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The CTR PAIR LOOP (fire 421): e_fmod's core `while(n--)` captured
    /// whole — the 2-word compare-subtract-and-shift walk:
    ///   hz = hx - hy; lz = lx - ly; if (lx < ly) hz -= 1;
    ///   if (hz < 0) { hx = hx+hx+(lx>>31); lx = lx+lx; }
    ///   else        { hx = hz+hz+(lz>>31); lx = lz+lz; }
    /// Emission facts (all measured): the borrow `cmplw lx,ly` hoists
    /// ABOVE both subtracts (they fill its latency); hz/lz take the
    /// FREED COUNT HOME and the next register up, via plain `subf` (no
    /// record — the `hz -= 1` borrow decrement sits between def and
    /// test, so the diamond re-tests with an explicit cmpwi); the then
    /// arm is the fire-420 pair step verbatim; the else arm's
    /// intermediates land DIRECTLY in r3 (hx is not a source there) and
    /// `lx = lz + lz` writes lx's home from lz. @N +0.
    pub(crate) fn try_ctr_pair_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        // A SCAFFOLD PREFIX may precede the loop (fire 425): the seam is
        // pure concatenation — scaffold ops emit in source order before
        // the mtctr, a loop-crossing sign local takes the next-free
        // register BEFORE the count home frees, and the loop's internal
        // temps allocate around it (hz keeps the freed count home, lz
        // shifts past the sign). Probed forms only: `param &= LOWMASK`
        // (in-place clrlwi), and the sign-extract pair `sign = param &
        // 0x80000000; param ^= sign` (clrrwi + xor).
        let [scaffold @ .., Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Expression::PostStep {
            target,
            operator: BinaryOperator::Subtract,
        } = condition
        else {
            return Ok(false);
        };
        let Expression::Variable(count) = target.as_ref() else {
            return Ok(false);
        };
        // The exact captured signature: (int hx, unsigned lx, int hy,
        // unsigned ly, int n) with n LAST — the freed-count-home rule for
        // hz/lz is only measured in that layout.
        let [p_hx, p_lx, p_hy, p_ly, p_n] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_hy.parameter_type != Type::Int
            || p_ly.parameter_type != Type::UnsignedInt
            || p_n.parameter_type != Type::Int
            || p_n.name != *count
        {
            return Ok(false);
        }
        let (hx, lx, hy, ly) = (
            p_hx.name.as_str(),
            p_lx.name.as_str(),
            p_hy.name.as_str(),
            p_ly.name.as_str(),
        );
        // Parse the scaffold prefix (probed forms only).
        enum ScaffoldOp {
            MaskParam { name: String, clear: u8 },
            SignExtract { source: String },
            XorParam { name: String },
        }
        let is_int_parameter = |name: &str| {
            function.parameters.iter().any(|parameter| {
                parameter.name == name
                    && matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt)
            })
        };
        let mut scaffold_ops: Vec<ScaffoldOp> = Vec::new();
        let mut sign_local: Option<&str> = None;
        for statement in scaffold {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            let Expression::Binary {
                operator,
                left,
                right,
            } = value
            else {
                return Ok(false);
            };
            match operator {
                // param &= (1<<n)-1  →  clrlwi param, param, 32-n (in place).
                BinaryOperator::BitAnd
                    if is_int_parameter(name)
                        && matches!(left.as_ref(), Expression::Variable(v) if v == name) =>
                {
                    let Expression::IntegerLiteral(mask) = right.as_ref() else {
                        return Ok(false);
                    };
                    let mask = *mask as u32;
                    if mask == 0 || !(mask as u64 + 1).is_power_of_two() {
                        return Ok(false);
                    }
                    let clear = mask.leading_zeros() as u8;
                    scaffold_ops.push(ScaffoldOp::MaskParam {
                        name: name.clone(),
                        clear,
                    });
                }
                // sign = param & 0x80000000  →  clrrwi sign, param, 31.
                BinaryOperator::BitAnd if !is_int_parameter(name) && sign_local.is_none() => {
                    let Expression::Variable(source) = left.as_ref() else {
                        return Ok(false);
                    };
                    if !is_int_parameter(source)
                        || !matches!(right.as_ref(), Expression::IntegerLiteral(m) if *m as u32 == 0x8000_0000)
                        || !function.locals.iter().any(|local| {
                            local.name == *name
                                && local.declared_type == Type::Int
                                && local.initializer.is_none()
                        })
                    {
                        return Ok(false);
                    }
                    sign_local = Some(name.as_str());
                    scaffold_ops.push(ScaffoldOp::SignExtract {
                        source: source.clone(),
                    });
                }
                // param ^= sign  →  xor param, param, sign (in place).
                BinaryOperator::BitXor
                    if is_int_parameter(name)
                        && matches!(left.as_ref(), Expression::Variable(v) if v == name) =>
                {
                    if !matches!(right.as_ref(), Expression::Variable(v) if Some(v.as_str()) == sign_local)
                    {
                        return Ok(false);
                    }
                    scaffold_ops.push(ScaffoldOp::XorParam { name: name.clone() });
                }
                _ => return Ok(false),
            }
        }
        // Body: [hz = hx - hy][lz = lx - ly][if (lx < ly) hz -= 1][diamond].
        let [Statement::Assign {
            name: hz,
            value: hz_value,
        }, Statement::Assign {
            name: lz,
            value: lz_value,
        }, Statement::If {
            condition: borrow_test,
            then_body: borrow_then,
            else_body: borrow_else,
        }, Statement::If {
            condition: test,
            then_body,
            else_body,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        let subtracts = |value: &Expression, from: &str, taken: &str| -> bool {
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } = value
            else {
                return false;
            };
            matches!(left.as_ref(), Expression::Variable(v) if v == from)
                && matches!(right.as_ref(), Expression::Variable(v) if v == taken)
        };
        if !subtracts(hz_value, hx, hy) || !subtracts(lz_value, lx, ly) {
            return Ok(false);
        }
        let names_distinct = {
            let mut names = [hx, lx, hy, ly, count.as_str(), hz.as_str(), lz.as_str()];
            names.sort_unstable();
            names.windows(2).all(|pair| pair[0] != pair[1])
        };
        if !names_distinct {
            return Ok(false);
        }
        if let Some(sign) = sign_local {
            if [hx, lx, hy, ly, count.as_str(), hz.as_str(), lz.as_str()].contains(&sign) {
                return Ok(false);
            }
        }
        let is_free_local = |name: &str, declared: Type| {
            function.locals.iter().any(|local| {
                local.name == name && local.declared_type == declared && local.initializer.is_none()
            })
        };
        if !is_free_local(hz, Type::Int) || !is_free_local(lz, Type::UnsignedInt) {
            return Ok(false);
        }
        // The borrow: if (lx < ly) hz -= 1; (unsigned compare, no else).
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left: borrow_left,
            right: borrow_right,
        } = borrow_test
        else {
            return Ok(false);
        };
        if !matches!(borrow_left.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(borrow_right.as_ref(), Expression::Variable(v) if v == ly)
            || !borrow_else.is_empty()
        {
            return Ok(false);
        }
        let [Statement::Assign {
            name: decremented,
            value: decrement,
        }] = borrow_then.as_slice()
        else {
            return Ok(false);
        };
        if decremented != hz {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: dec_left,
            right: dec_right,
        } = decrement
        else {
            return Ok(false);
        };
        if !matches!(dec_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(dec_right.as_ref(), Expression::IntegerLiteral(1))
        {
            return Ok(false);
        }
        // The diamond: if (hz < 0) {pair step from lx} else {pair step from hz/lz into hx/lx}.
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left: test_left,
            right: test_right,
        } = test
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(test_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        // An arm: high_target = high+high+(low>>31); lx = low+low;
        let pair_step = |statements: &[Statement], high: &str, low: &str| -> bool {
            let [Statement::Assign {
                name: high_name,
                value: high_value,
            }, Statement::Assign {
                name: low_name,
                value: low_value,
            }] = statements
            else {
                return false;
            };
            if high_name != hx || low_name != lx {
                return false;
            }
            let Expression::Binary {
                operator: BinaryOperator::Add,
                left: sum,
                right: carry,
            } = high_value
            else {
                return false;
            };
            let Expression::Binary {
                operator: BinaryOperator::Add,
                left: first,
                right: second,
            } = sum.as_ref()
            else {
                return false;
            };
            if !matches!(first.as_ref(), Expression::Variable(v) if v == high)
                || !matches!(second.as_ref(), Expression::Variable(v) if v == high)
            {
                return false;
            }
            let Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                left: shifted,
                right: amount,
            } = carry.as_ref()
            else {
                return false;
            };
            if !matches!(shifted.as_ref(), Expression::Variable(v) if v == low)
                || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
            {
                return false;
            }
            let Expression::Binary {
                operator: BinaryOperator::Add,
                left: low_first,
                right: low_second,
            } = low_value
            else {
                return false;
            };
            matches!(low_first.as_ref(), Expression::Variable(v) if v == low)
                && matches!(low_second.as_ref(), Expression::Variable(v) if v == low)
        };
        if !pair_step(then_body, hx, lx) {
            return Ok(false);
        }
        // The else arm may LEAD with the zero exit `if ((hz | lz) == 0)
        // return K;` — emitted INLINE as `or. r0,hz,lz; bne CONT; li r3,K;
        // blr` (a bare mid-loop return, no exit label; fire 422).
        enum ExitValue {
            Immediate(i16),
            Sign,
        }
        let (early_return, else_step) = match else_body.as_slice() {
            [Statement::If {
                condition: exit_test,
                then_body: exit_then,
                else_body: exit_else,
            }, rest @ ..] => {
                let Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: or_side,
                    right: zero_side,
                } = exit_test
                else {
                    return Ok(false);
                };
                let Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: or_left,
                    right: or_right,
                } = or_side.as_ref()
                else {
                    return Ok(false);
                };
                if !matches!(or_left.as_ref(), Expression::Variable(v) if v == hz)
                    || !matches!(or_right.as_ref(), Expression::Variable(v) if v == lz)
                    || !matches!(zero_side.as_ref(), Expression::IntegerLiteral(0))
                    || !exit_else.is_empty()
                {
                    return Ok(false);
                }
                let exit_value = match exit_then.as_slice() {
                    [Statement::Return(Some(Expression::IntegerLiteral(returned)))] => {
                        let Ok(returned) = i16::try_from(*returned) else {
                            return Ok(false);
                        };
                        ExitValue::Immediate(returned)
                    }
                    [Statement::Return(Some(Expression::Variable(v)))]
                        if Some(v.as_str()) == sign_local =>
                    {
                        ExitValue::Sign
                    }
                    _ => return Ok(false),
                };
                (Some(exit_value), rest)
            }
            _ => (None, else_body.as_slice()),
        };
        if !pair_step(else_step, hz, lz) {
            return Ok(false);
        }
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == hx) {
            return Ok(false);
        }
        let (
            Some(hx_register),
            Some(lx_register),
            Some(hy_register),
            Some(ly_register),
            Some(count_register),
        ) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(hy),
            self.lookup_general(ly),
            self.lookup_general(count),
        )
        else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // The sign local takes the next-free register BEFORE the count home
        // frees; hz keeps the freed count home; lz shifts past the sign.
        let sign_register = if sign_local.is_some() {
            Some(3 + function.parameters.len() as u8)
        } else {
            None
        };
        let hz_register = count_register;
        let lz_register =
            3 + function.parameters.len() as u8 + if sign_local.is_some() { 1 } else { 0 };
        if lz_register > 10 {
            return Ok(false);
        }
        // Resolve every scaffold register before any emission.
        enum ResolvedScaffold {
            Mask { register: u8, clear: u8 },
            Extract { source_register: u8 },
            Xor { register: u8 },
        }
        let mut resolved_scaffold = Vec::new();
        for op in &scaffold_ops {
            match op {
                ScaffoldOp::MaskParam { name, clear } => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Mask {
                        register,
                        clear: *clear,
                    });
                }
                ScaffoldOp::SignExtract { source } => {
                    let Some(source_register) = self.lookup_general(source) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Extract { source_register });
                }
                ScaffoldOp::XorParam { name } => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Xor { register });
                }
            }
        }
        // -- emit --
        for op in &resolved_scaffold {
            match op {
                ResolvedScaffold::Mask { register, clear } => {
                    self.output
                        .instructions
                        .push(Instruction::AndContiguousMask {
                            a: *register,
                            s: *register,
                            begin: *clear,
                            end: 31,
                        })
                }
                ResolvedScaffold::Extract { source_register } => {
                    self.output
                        .instructions
                        .push(Instruction::AndContiguousMask {
                            a: sign_register.unwrap(),
                            s: *source_register,
                            begin: 0,
                            end: 0,
                        })
                }
                ResolvedScaffold::Xor { register } => {
                    self.output.instructions.push(Instruction::Xor {
                        a: *register,
                        s: *register,
                        b: sign_register.unwrap(),
                    })
                }
            }
        }
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: count_register });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: count_register,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord {
                a: lx_register,
                b: ly_register,
            });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: hz_register,
            a: hy_register,
            b: hx_register,
        });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: lz_register,
            a: ly_register,
            b: lx_register,
        });
        let no_borrow_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, no_borrow_label); // bge
        self.output.instructions.push(Instruction::AddImmediate {
            d: hz_register,
            a: hz_register,
            immediate: -1,
        });
        self.bind_label(no_borrow_label);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: hz_register,
                immediate: 0,
            });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: lx_register,
                shift: 31,
            });
        self.output.instructions.push(Instruction::Add {
            d: lx_register,
            a: lx_register,
            b: lx_register,
        });
        self.output.instructions.push(Instruction::Add {
            d: 0,
            a: hx_register,
            b: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: hx_register,
            a: hx_register,
            b: 0,
        });
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        if let Some(exit_value) = &early_return {
            self.output.instructions.push(Instruction::OrRecord {
                a: 0,
                s: hz_register,
                b: lz_register,
            });
            let continue_label = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, continue_label); // bne
            match exit_value {
                ExitValue::Immediate(returned) => {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(3, *returned));
                }
                ExitValue::Sign => {
                    self.output
                        .instructions
                        .push(Instruction::move_register(3, sign_register.unwrap()));
                }
            }
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            self.bind_label(continue_label);
        }
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: lz_register,
                shift: 31,
            });
        self.output.instructions.push(Instruction::Add {
            d: lx_register,
            a: lz_register,
            b: lz_register,
        });
        self.output.instructions.push(Instruction::Add {
            d: hx_register,
            a: hz_register,
            b: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: hx_register,
            a: hz_register,
            b: hx_register,
        });
        self.bind_label(join_label);
        self.emit_branch_conditional_to(16, 0, body_label); // bdnz
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ROTATED LOOP (fire 413, the e_fmod ilogb family): mwcc emits
    /// non-counted loops as `init; b TEST; BODY: [step][body]; TEST:
    /// cond; b<positive> BODY; [mr]` with NO unrolling (counted loops
    /// take the ctr/unroll machinery instead — deferred). Registers per
    /// the captures: params in place; a condition-only computed value
    /// takes r0 (even across the backward branch); the returned local
    /// takes a param home freed during init, else the next free.
    pub(crate) fn try_rotated_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if !matches!(function.return_type, Type::Int | Type::Void)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind,
            initializer,
            condition: Some(condition),
            step,
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if function.return_type != Type::Void
            && !matches!(&function.return_expression, Some(Expression::Variable(_)))
        {
            return Ok(false);
        }
        // Locals: uninitialized (bound via the comma init), or initialized
        // with a small constant (an init-plan entry).
        let mut local_constant_inits: Vec<(&str, i16)> = Vec::new();
        for local in &function.locals {
            if local.declared_type != Type::Int || local.array_length.is_some() {
                return Ok(false);
            }
            if let Some(init) = &local.initializer {
                let Some(constant) =
                    crate::analysis::constant_value(init).and_then(|k| i16::try_from(k).ok())
                else {
                    return Ok(false);
                };
                local_constant_inits.push((local.name.as_str(), constant));
            }
        }
        // Homes: params in their registers.
        let mut homes: Vec<(String, u8)> = Vec::new();
        let mut char_pointers: Vec<String> = Vec::new();
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::Int => {}
                Type::Pointer(Pointee::Char) => char_pointers.push(parameter.name.clone()),
                _ => return Ok(false),
            }
            let Some(register) = self.lookup_general(&parameter.name) else {
                return Ok(false);
            };
            homes.push((parameter.name.clone(), register));
        }
        let home_of = |homes: &[(String, u8)], name: &str| {
            homes.iter().find(|(n, _)| n == name).map(|&(_, r)| r)
        };
        // The init list: `a = C`, `a = param`, or `a = param << K` — a
        // param read by its LAST init use frees its home.
        enum Init {
            Constant {
                register: u8,
                value: i16,
            },
            ShiftOfParam {
                register: u8,
                source: u8,
                amount: u8,
            },
        }
        let mut init_plan: Vec<Init> = Vec::new();
        for (name, constant) in &local_constant_inits {
            let top = homes
                .iter()
                .map(|&(_, r)| r)
                .filter(|&r| r != 0)
                .max()
                .unwrap_or(2);
            let register = top + 1;
            init_plan.push(Init::Constant {
                register,
                value: *constant,
            });
            homes.push((name.to_string(), register));
        }
        if let Some(init) = initializer {
            // Flatten the comma list.
            let mut elements: Vec<&Expression> = Vec::new();
            let mut cursor = init;
            loop {
                match cursor {
                    Expression::Comma { left, right } => {
                        elements.push(right.as_ref());
                        cursor = left.as_ref();
                    }
                    other => {
                        elements.push(other);
                        break;
                    }
                }
            }
            elements.reverse();
            // First pass: aliases (i = param) rename in place; param-reads
            // mark freed homes.
            let mut freed: Vec<u8> = Vec::new();
            let mut pending: Vec<(&str, &Expression)> = Vec::new();
            for element in &elements {
                let Expression::Assign { target, value } = element else {
                    return Ok(false);
                };
                let Expression::Variable(name) = target.as_ref() else {
                    return Ok(false);
                };
                match value.as_ref() {
                    Expression::Variable(source) => {
                        // An alias: the local IS the param, renamed.
                        let Some(register) = home_of(&homes, source) else {
                            return Ok(false);
                        };
                        homes.push((name.clone(), register));
                    }
                    Expression::Binary {
                        operator: BinaryOperator::ShiftLeft,
                        left,
                        right,
                    } => {
                        let Expression::Variable(source) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(source_register) = home_of(&homes, source) else {
                            return Ok(false);
                        };
                        let Some(amount) = crate::analysis::constant_value(right)
                            .and_then(|k| u8::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        // A condition-only computed value lives in r0.
                        init_plan.push(Init::ShiftOfParam {
                            register: 0,
                            source: source_register,
                            amount,
                        });
                        homes.push((name.clone(), 0));
                        freed.push(source_register);
                    }
                    other if crate::analysis::constant_value(other).is_some() => {
                        pending.push((name.as_str(), other));
                    }
                    _ => return Ok(false),
                }
            }
            // Second pass: constants take a freed param home, else the next
            // free register after the params.
            for (name, value) in pending {
                let constant = crate::analysis::constant_value(value).expect("checked");
                let Ok(small) = i16::try_from(constant) else {
                    return Ok(false);
                };
                let register = if let Some(register) = freed.pop() {
                    register
                } else {
                    let top = homes
                        .iter()
                        .map(|&(_, r)| r)
                        .filter(|&r| r != 0)
                        .max()
                        .unwrap_or(2);
                    top + 1
                };
                init_plan.push(Init::Constant {
                    register,
                    value: small,
                });
                homes.push((name.to_string(), register));
            }
        }
        // The condition: var OP const, var OP var, or the char-walk
        // truthiness `*p` (lbz + extsb. record test).
        enum LoopTest {
            Constant {
                register: u8,
                constant: i64,
                big: bool,
            },
            Register {
                left: u8,
                right: u8,
            },
            CharLoad {
                pointer: u8,
            },
        }
        let (loop_test, back_branch) = match condition {
            Expression::Binary {
                operator: cond_op,
                left: cond_left,
                right: cond_right,
            } => {
                let Expression::Variable(cond_var) = cond_left.as_ref() else {
                    return Ok(false);
                };
                let Some(cond_register) = home_of(&homes, cond_var) else {
                    return Ok(false);
                };
                let back = match cond_op {
                    BinaryOperator::Greater => (12u8, 1u8), // bgt
                    BinaryOperator::Less => (12u8, 0u8),    // blt
                    _ => return Ok(false),
                };
                if let Some(constant) = crate::analysis::constant_value(cond_right) {
                    let big = i16::try_from(constant).is_err();
                    if big && constant & 0xffff != 0 {
                        return Ok(false); // lis-only bounds measured
                    }
                    (
                        LoopTest::Constant {
                            register: cond_register,
                            constant,
                            big,
                        },
                        back,
                    )
                } else if let Expression::Variable(right_var) = cond_right.as_ref() {
                    let Some(right_register) = home_of(&homes, right_var) else {
                        return Ok(false);
                    };
                    (
                        LoopTest::Register {
                            left: cond_register,
                            right: right_register,
                        },
                        back,
                    )
                } else {
                    return Ok(false);
                }
            }
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else {
                    return Ok(false);
                };
                if !char_pointers.iter().any(|p| p == name) {
                    return Ok(false);
                }
                let Some(register) = home_of(&homes, name) else {
                    return Ok(false);
                };
                (LoopTest::CharLoad { pointer: register }, (4u8, 2u8)) // bne
            }
            _ => return Ok(false),
        };
        let hoists_big_bound = matches!(&loop_test, LoopTest::Constant { big: true, .. });
        // Body + step ops: compound self-ops on homed locals.
        enum LoopOp {
            AddImmediate {
                register: u8,
                value: i16,
            },
            SelfAdd {
                register: u8,
            },
            ShiftLeft {
                register: u8,
                amount: u8,
            },
            /// `*dst = *src` where src is the walk's condition pointer —
            /// the char loaded by the TEST carries across the back edge
            /// into this store (measured S2).
            CarriedStore {
                destination: u8,
            },
        }
        let condition_pointer: Option<&str> = match condition {
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        };
        let parse_op = |homes: &[(String, u8)], statement: &Statement| -> Option<LoopOp> {
            if let Statement::Store { target, value } = statement {
                // `*dst = *src` with src the condition pointer.
                let Expression::Dereference { pointer: dst } = target else {
                    return None;
                };
                let Expression::Variable(dst_name) = dst.as_ref() else {
                    return None;
                };
                let destination = home_of(homes, dst_name)?;
                let Expression::Dereference { pointer: src } = value else {
                    return None;
                };
                let Expression::Variable(src_name) = src.as_ref() else {
                    return None;
                };
                if condition_pointer != Some(src_name.as_str()) {
                    return None;
                }
                return Some(LoopOp::CarriedStore { destination });
            }
            let Statement::Assign { name, value } = statement else {
                return None;
            };
            let register = home_of(homes, name)?;
            let Expression::Binary {
                operator,
                left,
                right,
            } = value
            else {
                return None;
            };
            if !matches!(left.as_ref(), Expression::Variable(v) if v == name) {
                return None;
            }
            match operator {
                BinaryOperator::Add if matches!(right.as_ref(), Expression::Variable(v) if v == name) => {
                    Some(LoopOp::SelfAdd { register })
                }
                BinaryOperator::Add => {
                    let value = i16::try_from(crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::AddImmediate { register, value })
                }
                BinaryOperator::Subtract => {
                    let value = i16::try_from(-crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::AddImmediate { register, value })
                }
                BinaryOperator::ShiftLeft => {
                    let amount = u8::try_from(crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::ShiftLeft { register, amount })
                }
                _ => None,
            }
        };
        // Loop ops in emission order: the STEP first (it feeds the
        // condition — measured), then the body statements in source order.
        let mut loop_ops: Vec<LoopOp> = Vec::new();
        match kind {
            LoopKind::For => {
                let Some(step) = step else { return Ok(false) };
                let step_statement = Statement::Assign {
                    name: match step {
                        Expression::Assign { target, .. } => match target.as_ref() {
                            Expression::Variable(name) => name.clone(),
                            _ => return Ok(false),
                        },
                        _ => return Ok(false),
                    },
                    value: match step {
                        Expression::Assign { value, .. } => value.as_ref().clone(),
                        _ => return Ok(false),
                    },
                };
                let Some(op) = parse_op(&homes, &step_statement) else {
                    return Ok(false);
                };
                loop_ops.push(op);
            }
            LoopKind::While => {
                if step.is_some() {
                    return Ok(false);
                }
            }
            LoopKind::DoWhile => {
                if step.is_some() {
                    return Ok(false);
                }
            }
        }
        for statement in body {
            let Some(op) = parse_op(&homes, statement) else {
                return Ok(false);
            };
            loop_ops.push(op);
        }
        if loop_ops.is_empty() {
            return Ok(false);
        }
        // COUNTED loops (the condition variable stepped by a constant in a
        // For/While) take mwcc's unroll machinery — claiming them rotated
        // would be WRONG BYTES. Only the do-while keeps constant steps
        // (measured D1: no unroll).
        if !matches!(kind, LoopKind::DoWhile) {
            let condition_variable_register = match &loop_test {
                LoopTest::Constant { register, .. } => Some(*register),
                LoopTest::Register { left, .. } => Some(*left),
                LoopTest::CharLoad { .. } => None,
            };
            if let Some(register) = condition_variable_register {
                let stepped_by_constant = loop_ops.iter().any(|op| {
                    matches!(op, LoopOp::AddImmediate { register: stepped, .. } if *stepped == register)
                });
                if stepped_by_constant {
                    return Ok(false);
                }
            }
        }
        let has_carried_store = loop_ops
            .iter()
            .any(|op| matches!(op, LoopOp::CarriedStore { .. }));
        // The carried char takes the next free register (S2: r5).
        let carry_register = if has_carried_store {
            let top = homes
                .iter()
                .map(|&(_, r)| r)
                .filter(|&r| r != 0)
                .max()
                .unwrap_or(2);
            top + 1
        } else {
            0
        };
        let return_register = if function.return_type == Type::Void {
            3 // no move needed
        } else {
            let Some(Expression::Variable(returned)) = &function.return_expression else {
                return Ok(false);
            };
            let Some(register) = home_of(&homes, returned) else {
                return Ok(false);
            };
            register
        };
        // -- emit --
        // Init: param-reading shifts first, then constants (the freed-home
        // order); a big bound hoists to r0 before the loop.
        for init in &init_plan {
            match init {
                Init::ShiftOfParam {
                    register,
                    source,
                    amount,
                } => {
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: *register,
                            s: *source,
                            shift: *amount,
                        });
                }
                Init::Constant { register, value } => {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(*register, *value));
                }
            }
        }
        if let LoopTest::Constant {
            constant,
            big: true,
            ..
        } = &loop_test
        {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(
                    0,
                    (constant >> 16) as i16,
                ));
        }
        let _ = hoists_big_bound;
        let body_at = self.fresh_label();
        let test_at = self.fresh_label();
        if !matches!(kind, LoopKind::DoWhile) {
            // The rotated entry; a do-while falls straight into its body.
            self.emit_branch_to(test_at);
        }
        self.bind_label(body_at);
        for op in &loop_ops {
            match op {
                LoopOp::AddImmediate { register, value } => {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: *register,
                        a: *register,
                        immediate: *value,
                    });
                }
                LoopOp::SelfAdd { register } => {
                    self.output.instructions.push(Instruction::Add {
                        d: *register,
                        a: *register,
                        b: *register,
                    });
                }
                LoopOp::ShiftLeft { register, amount } => {
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: *register,
                            s: *register,
                            shift: *amount,
                        });
                }
                LoopOp::CarriedStore { destination } => {
                    self.output.instructions.push(Instruction::StoreByte {
                        s: carry_register,
                        a: *destination,
                        offset: 0,
                    });
                }
            }
        }
        self.bind_label(test_at);
        match &loop_test {
            LoopTest::Constant {
                register,
                constant,
                big: true,
            } => {
                let _ = constant;
                self.output
                    .instructions
                    .push(Instruction::CompareWord { a: *register, b: 0 });
            }
            LoopTest::Constant {
                register,
                constant,
                big: false,
            } => {
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate {
                        a: *register,
                        immediate: *constant as i16,
                    });
            }
            LoopTest::Register { left, right } => {
                self.output.instructions.push(Instruction::CompareWord {
                    a: *left,
                    b: *right,
                });
            }
            LoopTest::CharLoad { pointer } => {
                // A carried store loads into its carry register; a bare
                // walk uses r0.
                let target = if has_carried_store { carry_register } else { 0 };
                self.output.instructions.push(Instruction::LoadByteZero {
                    d: target,
                    a: *pointer,
                    offset: 0,
                });
                self.output
                    .instructions
                    .push(Instruction::ExtendSignByteRecord { a: 0, s: target });
            }
        }
        self.emit_branch_conditional_to(back_branch.0, back_branch.1, body_at);
        if function.return_type != Type::Void && return_register != 3 {
            self.output
                .instructions
                .push(Instruction::move_register(3, return_register));
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // @N: measured after implementation (objprobe) — placeholder 0.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }
}
