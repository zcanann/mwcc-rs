//! Switch/dispatcher families (fpclassify, trig, frsqrte sqrt, raise).

#[allow(unused_imports)]
use super::*;
use mwcc_versions::TrigDispatcherStyle;

impl Generator {
    /// The FPCLASSIFY SWITCH (fire 411, fminmaxdim's __fpclassifyd): a
    /// two-case-plus-default switch on `pun(x) & BIGMASK` whose arms are
    /// short-circuit || diamonds over the pun words. Measured: hx loads
    /// to r4 (live through the arms), the scrutinee rlwinm to r3, the
    /// tree compares r3 against the lis-built big value (cmpw) then 0
    /// (cmpwi); each arm: clrlwi. (record) -> bne TRUE; lwz the LOW word
    /// from the SPILL; cmpwi; beq FALSE; li/b-END per side; default li.
    pub(crate) fn try_fpclassify_switch(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{ArmBody, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        // The default is either `default:` inside the switch, or the
        // trailing `return K;` after it (the real fminmaxdim form).
        let (scrutinee, arms, default) = match function.statements.as_slice() {
            [Statement::Switch {
                scrutinee,
                arms,
                default,
            }] if function.return_expression.is_none() && default.is_some() => {
                let Some(result) = default.as_ref().and_then(|body| body.return_expression())
                else {
                    return Ok(false); // a statement-bodied default is not this shape
                };
                (scrutinee, arms, Some(result))
            }
            [Statement::Switch {
                scrutinee,
                arms,
                default,
            }] if default.is_none() && function.return_expression.is_some() => {
                (scrutinee, arms, function.return_expression.as_ref())
            }
            _ => return Ok(false),
        };
        // pun0(x) & BIGMASK (lis-only mask).
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } = scrutinee
        else {
            return Ok(false);
        };
        if crate::frame::pun_word_offset_pub(left, x) != Some(0) {
            return Ok(false);
        }
        let Some(big_mask) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let big_mask = big_mask as u32 as i64;
        let Some((mask_begin, mask_end)) = crate::analysis::rlwinm_mask(big_mask) else {
            return Ok(false);
        };
        if big_mask & 0xffff != 0 {
            return Ok(false);
        }
        // Exactly two cases: the mask value itself + zero, and a default.
        let Some(default_value) = default else {
            return Ok(false);
        };
        let _ = &default_value;
        let Some(default_constant) =
            crate::analysis::constant_value(default_value).and_then(|k| i16::try_from(k).ok())
        else {
            return Ok(false);
        };
        if arms.len() != 2 {
            return Ok(false);
        }
        // The || diamond: (pun0 & M2) || pun4[& 0xffffffff] -> (A, B).
        struct Diamond {
            second_begin: u8,
            second_end: u8,
            when_true: i16,
            when_false: i16,
        }
        let parse_diamond = |body: &ArmBody| -> Option<Diamond> {
            let ArmBody::Statements(statements) = body else {
                return None;
            };
            let [Statement::If {
                condition,
                then_body,
                else_body,
            }] = statements.as_slice()
            else {
                return None;
            };
            let Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left,
                right,
            } = condition
            else {
                return None;
            };
            let Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left: p0,
                right: m2,
            } = left.as_ref()
            else {
                return None;
            };
            if crate::frame::pun_word_offset_pub(p0, x) != Some(0) {
                return None;
            }
            let (second_begin, second_end) =
                crate::analysis::rlwinm_mask(crate::analysis::constant_value(m2)?)?;
            // The low word, optionally masked with the identity 0xffffffff.
            let low_ok = match right.as_ref() {
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left: p4,
                    right: identity,
                } => {
                    crate::frame::pun_word_offset_pub(p4, x) == Some(4)
                        && crate::analysis::constant_value(identity).map(|c| c as u32)
                            == Some(0xffff_ffff)
                }
                other => crate::frame::pun_word_offset_pub(other, x) == Some(4),
            };
            if !low_ok {
                return None;
            }
            let value_of = |body: &[Statement]| -> Option<i16> {
                let [Statement::Return(Some(value))] = body else {
                    return None;
                };
                crate::analysis::constant_value(value).and_then(|k| i16::try_from(k).ok())
            };
            Some(Diamond {
                second_begin,
                second_end,
                when_true: value_of(then_body)?,
                when_false: value_of(else_body)?,
            })
        };
        let mut big_arm: Option<Diamond> = None;
        let mut zero_arm: Option<Diamond> = None;
        for arm in arms {
            let diamond = parse_diamond(&arm.body);
            if arm.value == big_mask {
                big_arm = diamond;
            } else if arm.value == 0 {
                zero_arm = diamond;
            } else {
                return Ok(false);
            }
        }
        let (Some(big_arm), Some(zero_arm)) = (big_arm, zero_arm) else {
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
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(
                0,
                (big_mask >> 16) as i16,
            ));
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: mask_begin,
            end: mask_end,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        let big_at = self.fresh_label();
        let zero_at = self.fresh_label();
        let default_at = self.fresh_label();
        let end_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, big_at); // beq
        self.emit_branch_conditional_to(4, 0, default_at); // bge
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, zero_at); // beq
        self.emit_branch_to(default_at);
        let mut emit_diamond = |generator: &mut Self, diamond: &Diamond, label| {
            generator.bind_label(label);
            let when_true = generator.fresh_label();
            let when_false = generator.fresh_label();
            generator
                .output
                .instructions
                .push(Instruction::AndMaskRecord {
                    a: 0,
                    s: 4,
                    begin: diamond.second_begin,
                    end: diamond.second_end,
                });
            generator.emit_branch_conditional_to(4, 2, when_true); // bne
            generator.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            });
            generator
                .output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
            generator.emit_branch_conditional_to(12, 2, when_false); // beq
            generator.bind_label(when_true);
            generator
                .output
                .instructions
                .push(Instruction::load_immediate(3, diamond.when_true));
            generator.emit_branch_to(end_at);
            generator.bind_label(when_false);
            generator
                .output
                .instructions
                .push(Instruction::load_immediate(3, diamond.when_false));
            generator.emit_branch_to(end_at);
        };
        emit_diamond(self, &big_arm, big_at);
        emit_diamond(self, &zero_arm, zero_at);
        self.bind_label(default_at);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, default_constant));
        self.bind_label(end_at);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @18 on the fpclassify object vs the
        // +0 base's @5).
        self.output.anonymous_label_bump += 13;
        Ok(true)
    }

    /// The TRIG DISPATCHER (fire 408, s_sin/s_cos): the fdlibm range
    /// dispatch — a small-|x| kernel call, the inf/NaN x-x rung, then
    /// __ieee754_rem_pio2 into a frame array and a four-way switch of
    /// kernel calls (negated for quadrants 2/3). Measured: frame 32
    /// (x spill 8, y[2] at 16), the K1 synthesis in the mflr latency
    /// slot, cmpw REGISTER compares against lis-built bounds, the
    /// binary switch tree [cmpwi 1: beq C1 | bge -> cmpwi 3: bge DEF,
    /// b C2 | cmpwi 0: bge C0, b DEF], per-arm lfd/li/lfd argument
    /// loads, and fneg on the negated results.
    pub(crate) fn try_trig_dispatcher(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double || !function.guards.is_empty() {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        // Locals: y[2] (double array), z = 0.0, and the two ints.
        let mut array: Option<&str> = None;
        let mut zero_local: Option<&str> = None;
        let mut ints: Vec<&str> = Vec::new();
        for local in &function.locals {
            match (local.declared_type, local.array_length) {
                (Type::Double, Some(2)) if array.is_none() => array = Some(local.name.as_str()),
                (Type::Double, None)
                    if matches!(&local.initializer, Some(Expression::FloatLiteral(z)) if *z == 0.0)
                        && zero_local.is_none() =>
                {
                    zero_local = Some(local.name.as_str())
                }
                (Type::Int, None) if local.initializer.is_none() => ints.push(local.name.as_str()),
                _ => return Ok(false),
            }
        }
        let (Some(array), Some(zero_local)) = (array, zero_local) else {
            return Ok(false);
        };
        if ints.len() != 2 {
            return Ok(false);
        }
        // statements (the parser FLATTENS the else-if returns):
        // ix = pun(x); ix &= 0x7fffffff; if (ix<=K1) return call;
        // if (ix>=K2) return x-x; n = rem(x,y); switch (n&3) {...}.
        // Two tails: the four-way SWITCH of kernels (sin/cos), or the
        // direct parity call `return kernel(y0,y1,1-((n&1)<<1))` (tan) —
        // the latter arrives as the function's trailing return.
        let (head, switch_tail, return_tail): (
            &[Statement],
            Option<(
                &Expression,
                &Vec<mwcc_syntax_trees::SwitchArm>,
                &Option<mwcc_syntax_trees::ArmBody>,
            )>,
            Option<&Expression>,
        ) = match function.statements.as_slice() {
            [head @ .., Statement::Switch {
                scrutinee,
                arms,
                default,
            }] if head.len() == 5 => (head, Some((scrutinee, arms, default)), None),
            [head @ .., Statement::Return(Some(value))] if head.len() == 5 => {
                (head, None, Some(value))
            }
            _ => return Ok(false),
        };
        let [Statement::Assign {
            name: ix1,
            value: pun,
        }, Statement::Assign {
            name: ix2,
            value: mask,
        }, Statement::If {
            condition: small,
            then_body: small_arm,
            else_body: small_else,
        }, Statement::If {
            condition: huge_cond,
            then_body: huge_arm,
            else_body: huge_else,
        }, Statement::Assign {
            name: n_name,
            value: rem_call,
        }] = head
        else {
            return Ok(false);
        };
        if !small_else.is_empty() || !huge_else.is_empty() {
            return Ok(false);
        }
        let ix = ix1.as_str();
        if ix2 != ix
            || !ints.contains(&ix)
            || crate::frame::pun_word_offset_pub(pun, x) != Some(0)
            || !matches!(mask, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if v == ix)
                    && crate::analysis::constant_value(right) == Some(0x7fff_ffff))
        {
            return Ok(false);
        }
        // if (ix <= K1) return kernel(x, z, 0);
        let Expression::Binary {
            operator: BinaryOperator::LessEqual,
            left,
            right,
        } = small
        else {
            return Ok(false);
        };
        if !matches!(left.as_ref(), Expression::Variable(v) if v == ix) {
            return Ok(false);
        }
        let Some(k1) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let [Statement::Return(Some(Expression::Call {
            name: small_callee,
            arguments: small_args,
        }))] = small_arm.as_slice()
        else {
            return Ok(false);
        };
        // kernel(x, z, 0) or kernel(x, z) — the int arg optional (cos).
        let small_int = match small_args.as_slice() {
            [Expression::Variable(a), Expression::Variable(z)] if a == x && z == zero_local => None,
            [Expression::Variable(a), Expression::Variable(z), n]
                if a == x && z == zero_local && crate::analysis::constant_value(n).is_some() =>
            {
                Some(crate::analysis::constant_value(n).expect("checked") as i16)
            }
            _ => return Ok(false),
        };
        // if (ix >= K2) return x - x;
        let Expression::Binary {
            operator: BinaryOperator::GreaterEqual,
            left,
            right,
        } = huge_cond
        else {
            return Ok(false);
        };
        if !matches!(left.as_ref(), Expression::Variable(v) if v == ix) {
            return Ok(false);
        }
        let Some(k2) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        if k1 & 0xffff == 0 || k2 & 0xffff != 0 {
            // K1 synthesizes lis+addi; K2 is lis-only (measured shapes).
            return Ok(false);
        }
        if !matches!(huge_arm.as_slice(), [Statement::Return(Some(Expression::Binary { operator: BinaryOperator::Subtract, left, right }))]
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::Variable(v) if v == x))
        {
            return Ok(false);
        }
        // n = rem_pio2(x, y); switch (n & 3) { ... }
        if !ints.contains(&n_name.as_str()) || n_name == ix {
            return Ok(false);
        }
        let Expression::Call {
            name: rem_callee,
            arguments: rem_args,
        } = rem_call
        else {
            return Ok(false);
        };
        if !matches!(rem_args.as_slice(), [Expression::Variable(a), Expression::Variable(y)]
            if a == x && y == array)
        {
            return Ok(false);
        }
        // The tan tail: return kernel(y[0], y[1], 1 - ((n & 1) << 1)).
        let parity_tail: Option<String> = if switch_tail.is_none() {
            let Some(Expression::Call { name, arguments }) = return_tail else {
                return Ok(false);
            };
            let ok = matches!(arguments.as_slice(),
                [Expression::Index { base, index: i0 }, Expression::Index { base: b1, index: i1 }, parity]
                    if matches!(base.as_ref(), Expression::Variable(v) if v == array)
                        && matches!(b1.as_ref(), Expression::Variable(v) if v == array)
                        && crate::analysis::constant_value(i0) == Some(0)
                        && crate::analysis::constant_value(i1) == Some(1)
                        && matches!(parity, Expression::Binary { operator: BinaryOperator::Subtract, left: one, right: shifted }
                            if crate::analysis::constant_value(one) == Some(1)
                                && matches!(shifted.as_ref(), Expression::Binary { operator: BinaryOperator::ShiftLeft, left: masked, right: by_one }
                                    if crate::analysis::constant_value(by_one) == Some(1)
                                        && matches!(masked.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: nv, right: m1 }
                                            if matches!(nv.as_ref(), Expression::Variable(v) if v == n_name.as_str())
                                                && crate::analysis::constant_value(m1) == Some(1)))));
            if !ok {
                return Ok(false);
            }
            Some(name.clone())
        } else {
            None
        };
        if let Some((scrutinee, _, _)) = &switch_tail {
            if !matches!(*scrutinee, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if v == n_name.as_str())
                    && crate::analysis::constant_value(right) == Some(3))
            {
                return Ok(false);
            }
        }
        // The four arms: (callee, int arg, negated) per quadrant 0..3.
        struct Quadrant {
            callee: String,
            int_argument: Option<i16>,
            negated: bool,
        }
        let parse_quadrant = |result: &Expression| -> Option<Quadrant> {
            let (call, negated) = match result {
                Expression::Unary {
                    operator: UnaryOperator::Negate,
                    operand,
                } => (operand.as_ref(), true),
                other => (other, false),
            };
            let Expression::Call { name, arguments } = call else {
                return None;
            };
            let int_argument = match arguments.as_slice() {
                [Expression::Index { base, index: i0 }, Expression::Index {
                    base: b1,
                    index: i1,
                }] if matches!(base.as_ref(), Expression::Variable(v) if v == array)
                    && matches!(b1.as_ref(), Expression::Variable(v) if v == array)
                    && crate::analysis::constant_value(i0) == Some(0)
                    && crate::analysis::constant_value(i1) == Some(1) =>
                {
                    None
                }
                [Expression::Index { base, index: i0 }, Expression::Index {
                    base: b1,
                    index: i1,
                }, n]
                    if matches!(base.as_ref(), Expression::Variable(v) if v == array)
                        && matches!(b1.as_ref(), Expression::Variable(v) if v == array)
                        && crate::analysis::constant_value(i0) == Some(0)
                        && crate::analysis::constant_value(i1) == Some(1)
                        && crate::analysis::constant_value(n).is_some() =>
                {
                    Some(crate::analysis::constant_value(n).expect("checked") as i16)
                }
                _ => return None,
            };
            Some(Quadrant {
                callee: name.clone(),
                int_argument,
                negated,
            })
        };
        let mut quadrants: Vec<Option<Quadrant>> = vec![None, None, None, None];
        if let Some((_, arms, default)) = &switch_tail {
            for arm in arms.iter() {
                let index = arm.value;
                if !(0..3).contains(&index) {
                    return Ok(false);
                }
                let Some(result) = arm.result() else {
                    return Ok(false);
                };
                quadrants[index as usize] = parse_quadrant(result);
            }
            let Some(default_result) = default.as_ref().and_then(|body| body.return_expression())
            else {
                return Ok(false);
            };
            quadrants[3] = parse_quadrant(default_result);
            if quadrants.iter().any(|quadrant| quadrant.is_none()) {
                return Ok(false);
            }
        }
        // -- emit --
        self.non_leaf = true;
        self.frame_size = 32;
        let legacy_reloading = self.behavior.trig_dispatcher_style
            == mwcc_versions::TrigDispatcherStyle::LegacyReloading;
        if legacy_reloading {
            self.output
                .instructions
                .push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(
                    3,
                    ((k1 + 0x8000) >> 16) as i16,
                ));
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: k1 as i16,
            });
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
            self.load_double_constant(2, 0.0f64.to_bits());
        } else {
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -32,
                });
            self.output
                .instructions
                .push(Instruction::MoveFromLinkRegister { d: 0 });
            // K1's lis fills the mflr latency slot.
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(
                    3,
                    ((k1 + 0x8000) >> 16) as i16,
                ));
            self.output
                .instructions
                .push(Instruction::StoreFloatDouble {
                    s: 1,
                    a: 1,
                    offset: 8,
                });
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: k1 as i16,
            });
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 3,
            shift: 0,
            begin: 1,
            end: 31,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        let epilogue = self.fresh_label();
        let huge_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, huge_at); // bgt
                                                         // The small arm: kernel(x, z, 0).
        if !legacy_reloading {
            self.load_double_constant(2, 0.0f64.to_bits());
        } else {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        if let Some(int_argument) = small_int {
            self.output
                .instructions
                .push(Instruction::load_immediate(3, int_argument));
        }
        self.record_relocation(RelocationKind::Rel24, small_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: small_callee.clone(),
        });
        self.emit_branch_to(epilogue);
        // else if (ix >= K2) return x - x;
        self.bind_label(huge_at);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, (k2 >> 16) as i16));
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 3, b: 0 });
        let rem_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, rem_at); // blt
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 8,
            });
            self.output
                .instructions
                .push(Instruction::FloatSubtractDouble { d: 1, a: 0, b: 0 });
        } else {
            self.output
                .instructions
                .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 1 });
        }
        self.emit_branch_to(epilogue);
        // n = rem_pio2(x, &y); the switch tree.
        self.bind_label(rem_at);
        if legacy_reloading {
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 8,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 1,
            immediate: 16,
        });
        self.record_relocation(RelocationKind::Rel24, rem_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: rem_callee.clone(),
        });
        if let Some(parity_callee) = &parity_tail {
            // tan: rlwinm r0,r3,1,30,30 ((n&1)<<1 fused); lfd f1/f2;
            // subfic r3,r0,1 between the loads and the call; fall to EPI.
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 0,
                s: 3,
                shift: 1,
                begin: 30,
                end: 30,
            });
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 1,
                a: 1,
                offset: 16,
            });
            self.output.instructions.push(Instruction::LoadFloatDouble {
                d: 2,
                a: 1,
                offset: 24,
            });
            self.output
                .instructions
                .push(Instruction::SubtractFromImmediate {
                    d: 3,
                    a: 0,
                    immediate: 1,
                });
            self.record_relocation(RelocationKind::Rel24, parity_callee);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: parity_callee.clone(),
            });
            self.bind_label(epilogue);
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            });
            if !legacy_reloading {
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
            }
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            });
            if legacy_reloading {
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
            }
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            // Pre-pool labels (measure via objprobe on the tan object).
            self.output.anonymous_label_bump += if legacy_reloading { 7 } else { 8 };
            return Ok(true);
        }
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 3,
            shift: 0,
            begin: 30,
            end: 31,
        });
        let case0 = self.fresh_label();
        let case1 = self.fresh_label();
        let case2 = self.fresh_label();
        let case3 = self.fresh_label();
        let mid = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, case1); // beq
        self.emit_branch_conditional_to(4, 0, mid); // bge -> the 2/3 side
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, case0); // bge
        self.emit_branch_to(case3);
        self.bind_label(mid);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, case3); // bge
        self.emit_branch_to(case2);
        // The arms.
        let mut emit_arm = |generator: &mut Self, quadrant: &Quadrant, label, falls: bool| {
            generator.bind_label(label);
            generator
                .output
                .instructions
                .push(Instruction::LoadFloatDouble {
                    d: 1,
                    a: 1,
                    offset: 16,
                });
            if let Some(int_argument) = quadrant.int_argument {
                generator
                    .output
                    .instructions
                    .push(Instruction::load_immediate(3, int_argument));
            }
            generator
                .output
                .instructions
                .push(Instruction::LoadFloatDouble {
                    d: 2,
                    a: 1,
                    offset: 24,
                });
            generator.record_relocation(RelocationKind::Rel24, &quadrant.callee);
            generator
                .output
                .instructions
                .push(Instruction::BranchAndLink {
                    target: quadrant.callee.clone(),
                });
            if quadrant.negated {
                generator
                    .output
                    .instructions
                    .push(Instruction::FloatNegate { d: 1, b: 1 });
            }
            if !falls {
                generator.emit_branch_to(epilogue);
            }
        };
        let [Some(q0), Some(q1), Some(q2), Some(q3)] = &quadrants[..] else {
            unreachable!("validated above");
        };
        emit_arm(self, q0, case0, false);
        emit_arm(self, q1, case1, false);
        emit_arm(self, q2, case2, false);
        emit_arm(self, q3, case3, true);
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        if !legacy_reloading {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        if legacy_reloading {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // Pre-pool labels. Deferred compilation expands the dispatcher's
        // internal CFG block even though its emitted instructions are stable,
        // and that hidden block contracts at each measured generation
        // boundary. Isolated probes place its first pool entry at @25 (build
        // 163), @29 (build 53), and @24 (build 81+) from bases @2/@5.
        let expanded_dispatcher_labels = self.behavior.deferred_inlining;
        self.output.anonymous_label_bump += match (
            self.behavior.trig_dispatcher_style,
            expanded_dispatcher_labels,
        ) {
            (TrigDispatcherStyle::EarlyLiveParameter, true) => 24,
            (TrigDispatcherStyle::LiveParameter, true) => 19,
            (TrigDispatcherStyle::LegacyReloading, true) => 23,
            (
                TrigDispatcherStyle::EarlyLiveParameter | TrigDispatcherStyle::LiveParameter,
                false,
            ) => 13,
            (TrigDispatcherStyle::LegacyReloading, false) => 12,
        };
        Ok(true)
    }

    /// The __frsqrte NEWTON SQRT (fire 407, the Dolphin math_inlines
    /// pattern): a LEAF float ladder around N reciprocal-sqrt refinement
    /// steps. Measured: lfd 0.0; fcmpo; ble; frsqrte f2,f1; lfd f4(.5);
    /// lfd f3(3.0); N x [fmul f0,f2,f2; fmul f2,f4,f2; fnmsub f0,f1,f0,f3;
    /// fmul f2,f2,f0] with the LAST step's product landing in f0; fmul
    /// f1,f1,f0; blr — then the ladder: fcmpu f0,f1 (==0, operands
    /// pool-first); fmr f1,f0; fcmpu f1,f0 (bare x, swapped); lis+lfs
    /// through the NAN/INFINITY int-array globals (Addr16 pairs).
    pub(crate) fn try_frsqrte_sqrt(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double || !function.guards.is_empty() {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        let [guess_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if guess_local.declared_type != Type::Double || guess_local.initializer.is_some() {
            return Ok(false);
        }
        let guess = guess_local.name.as_str();
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // if (x > 0.0)
        if !matches!(condition, Expression::Binary { operator: BinaryOperator::Greater, left, right }
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::FloatLiteral(zero) if *zero == 0.0))
        {
            return Ok(false);
        }
        // then: guess = __frsqrte(x); N refinements; return x * guess.
        let [Statement::Assign {
            name: seed_name,
            value: seed,
        }, refinements @ .., Statement::Return(Some(product))] = then_body.as_slice()
        else {
            return Ok(false);
        };
        if seed_name != guess
            || !matches!(seed, Expression::Call { name, arguments }
                if name == "__frsqrte"
                    && matches!(arguments.as_slice(), [Expression::Variable(v)] if v == x))
        {
            return Ok(false);
        }
        if refinements.is_empty() {
            return Ok(false);
        }
        // Each: guess = .5 * guess * (3.0 - guess * guess * x)
        for refinement in refinements {
            let Statement::Assign { name, value } = refinement else {
                return Ok(false);
            };
            if name != guess {
                return Ok(false);
            }
            let ok = matches!(value, Expression::Binary { operator: BinaryOperator::Multiply, left, right }
                if matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: half, right: g }
                    if matches!(half.as_ref(), Expression::FloatLiteral(h) if *h == 0.5)
                        && matches!(g.as_ref(), Expression::Variable(v) if v == guess))
                    && matches!(right.as_ref(), Expression::Binary { operator: BinaryOperator::Subtract, left: three, right: ggx }
                        if matches!(three.as_ref(), Expression::FloatLiteral(t) if *t == 3.0)
                            && matches!(ggx.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: gg, right: xv }
                                if matches!(xv.as_ref(), Expression::Variable(v) if v == x)
                                    && matches!(gg.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: g1, right: g2 }
                                        if matches!(g1.as_ref(), Expression::Variable(v) if v == guess)
                                            && matches!(g2.as_ref(), Expression::Variable(v) if v == guess)))));
            if !ok {
                return Ok(false);
            }
        }
        if !matches!(product, Expression::Binary { operator: BinaryOperator::Multiply, left, right }
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::Variable(v) if v == guess))
        {
            return Ok(false);
        }
        // else: if (x == 0.0) return 0; else if (x) return *(float*)NAN;
        // ... with the trailing return *(float*)INF.
        let [Statement::If {
            condition: zero_cond,
            then_body: zero_then,
            else_body: zero_else,
        }] = else_body.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(zero_cond, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::FloatLiteral(zero) if *zero == 0.0))
        {
            return Ok(false);
        }
        if !matches!(zero_then.as_slice(), [Statement::Return(Some(value))]
            if crate::analysis::constant_value(value) == Some(0))
        {
            return Ok(false);
        }
        let float_global = |expression: &Expression| -> Option<String> {
            let Expression::Dereference { pointer } = expression else {
                return None;
            };
            let Expression::Cast {
                target_type: Type::Pointer(Pointee::Float),
                operand,
            } = pointer.as_ref()
            else {
                return None;
            };
            let Expression::Variable(name) = operand.as_ref() else {
                return None;
            };
            Some(name.clone())
        };
        let [Statement::If {
            condition: nan_cond,
            then_body: nan_then,
            else_body: nan_else,
        }] = zero_else.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(nan_cond, Expression::Variable(v) if v == x) || !nan_else.is_empty() {
            return Ok(false);
        }
        let [Statement::Return(Some(nan_value))] = nan_then.as_slice() else {
            return Ok(false);
        };
        let (Some(nan_symbol), Some(Some(infinity_symbol))) = (
            float_global(nan_value),
            function
                .return_expression
                .as_ref()
                .map(|value| float_global(value)),
        ) else {
            return Ok(false);
        };
        // -- emit (a leaf: no frame at all) --
        let steps = refinements.len();
        self.load_double_constant(0, 0.0f64.to_bits());
        self.output
            .instructions
            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        let ladder = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, ladder); // ble
        self.output
            .instructions
            .push(Instruction::FloatReciprocalSqrtEstimate { d: 2, b: 1 });
        self.load_double_constant(4, 0.5f64.to_bits());
        self.load_double_constant(3, 3.0f64.to_bits());
        for step in 0..steps {
            let last = step + 1 == steps;
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble { d: 0, a: 2, c: 2 });
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble { d: 2, a: 4, c: 2 });
            self.output
                .instructions
                .push(Instruction::FloatNegativeMultiplySubtractDouble {
                    d: 0,
                    a: 1,
                    c: 0,
                    b: 3,
                });
            self.output
                .instructions
                .push(Instruction::FloatMultiplyDouble {
                    d: if last { 0 } else { 2 },
                    a: 2,
                    c: 0,
                });
        }
        self.output
            .instructions
            .push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(ladder);
        // x == 0.0: fcmpu with the POOL value first; return the pooled 0.
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 0, b: 1 });
        let nan_at = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, nan_at); // bne
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(nan_at);
        // bare x: fcmpu the other way; INFINITY on equal-to-zero.
        self.output
            .instructions
            .push(Instruction::FloatCompareUnordered { a: 1, b: 0 });
        let infinity_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, infinity_at); // beq
        self.emit_address_high(3, &nan_symbol);
        self.record_relocation(RelocationKind::Addr16Lo, &nan_symbol);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.bind_label(infinity_at);
        self.emit_address_high(3, &infinity_symbol);
        self.record_relocation(RelocationKind::Addr16Lo, &infinity_symbol);
        self.output.instructions.push(Instruction::LoadFloatSingle {
            d: 1,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @17 on the math_inlines object vs the
        // +0 base's @5).
        self.output.anonymous_label_bump += 12;
        Ok(true)
    }

    /// The raise FAMILY (the call-class acceptance target): a function-pointer
    /// local loaded from a static dispatch table, tested through guard blocks,
    /// conditionally cleared, and finally CALLED — with the local and the int
    /// parameter living in callee-saved registers across the calls. Every order
    /// below is the measured 44-instruction signal.c raise() capture; the
    /// registers are allocator-chosen (v_temp -> r31, v_sig -> r30 from the
    /// call-crossing pool; the address chain's virtual takes the freed r3).
    pub(crate) fn try_raise_family(&mut self, function: &Function) -> Compilation<bool> {
        macro_rules! decline {
            ($n:expr) => {{
                if std::env::var("RAISE_DEBUG").is_ok() {
                    eprintln!("raise decline {}", $n);
                }
                return Ok(false);
            }};
        }
        if !function.guards.is_empty() || function.return_type != Type::Int {
            decline!(1);
        }
        let [param] = function.parameters.as_slice() else {
            decline!(2)
        };
        if param.parameter_type != Type::Int {
            decline!(3);
        }
        let sig = param.name.as_str();
        let [local] = function.locals.as_slice() else {
            decline!(4)
        };
        if local.initializer.is_some() || local.array_length.is_some() {
            decline!(5);
        }
        let temp = local.name.as_str();
        if !matches!(&function.return_expression, Some(expression) if constant_value(expression) == Some(0))
        {
            decline!(6);
        }
        let [s0, s1, s2, s3, s4, s5] = function.statements.as_slice() else {
            decline!(7)
        };
        let is_sig = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == sig);
        let is_temp = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == temp);
        // temp compared to a constant, through an optional cast (the source
        // writes `(unsigned long) temp != 1`).
        let temp_versus =
            |expression: &Expression, operator: BinaryOperator, constant: i64| -> bool {
                let Expression::Binary {
                    operator: found,
                    left,
                    right,
                } = expression
                else {
                    return false;
                };
                if *found != operator || constant_value(right) != Some(constant) {
                    return false;
                }
                match left.as_ref() {
                    Expression::Cast { operand, .. } => is_temp(operand),
                    other => is_temp(other),
                }
            };
        // The table subscript `funcs[sig - 1]`, returning the table's name.
        let table_of = |expression: &Expression| -> Option<String> {
            let Expression::Index { base, index } = expression else {
                return None;
            };
            let Expression::Variable(table) = base.as_ref() else {
                return None;
            };
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } = index.as_ref()
            else {
                return None;
            };
            (is_sig(left) && constant_value(right) == Some(1)).then(|| table.clone())
        };
        // s0: if (sig < 1 || sig > BOUND) return -1;
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = s0
        else {
            decline!(8)
        };
        if !else_body.is_empty()
            || !matches!(then_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(-1))
        {
            decline!(9);
        }
        let Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left,
            right,
        } = condition
        else {
            decline!(10)
        };
        let low_test = matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Less, left, right }
            if is_sig(left) && constant_value(right) == Some(1));
        let Expression::Binary {
            operator: BinaryOperator::Greater,
            left: bound_left,
            right: bound_right,
        } = right.as_ref()
        else {
            decline!(11)
        };
        let Some(bound) = constant_value(bound_right).and_then(|bound| i16::try_from(bound).ok())
        else {
            decline!(12)
        };
        if !low_test || !is_sig(bound_left) {
            decline!(13);
        }
        // s1: temp = funcs[sig - 1];
        let Statement::Assign {
            name: s1_name,
            value: s1_value,
        } = s1
        else {
            decline!(14)
        };
        let Some(table) = table_of(s1_value) else {
            decline!(15)
        };
        if s1_name != temp {
            decline!(16);
        }
        // s2: if ((cast) temp != 1) funcs[sig - 1] = 0;
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = s2
        else {
            decline!(17)
        };
        if !else_body.is_empty() || !temp_versus(condition, BinaryOperator::NotEqual, 1) {
            decline!(18);
        }
        let [Statement::Store { target, value }] = then_body.as_slice() else {
            decline!(19)
        };
        if table_of(target).as_deref() != Some(table.as_str())
            || !matches!(constant_value(value), Some(0))
        {
            decline!(20);
        }
        // s3: if ((cast) temp == 1 || (temp == 0 && sig == 1)) return 0;
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = s3
        else {
            decline!(21)
        };
        if !else_body.is_empty()
            || !matches!(then_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(0))
        {
            decline!(22);
        }
        let Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left,
            right,
        } = condition
        else {
            decline!(23)
        };
        if !temp_versus(left, BinaryOperator::Equal, 1) {
            decline!(24);
        }
        let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: and_left,
            right: and_right,
        } = right.as_ref()
        else {
            decline!(25)
        };
        if !temp_versus(and_left, BinaryOperator::Equal, 0)
            || !matches!(and_right.as_ref(), Expression::Binary { operator: BinaryOperator::Equal, left, right }
                if is_sig(left) && constant_value(right) == Some(1))
        {
            decline!(26);
        }
        // s4: if (temp == 0) exit(0);
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = s4
        else {
            decline!(27)
        };
        if !else_body.is_empty() || !temp_versus(condition, BinaryOperator::Equal, 0) {
            decline!(28);
        }
        let [Statement::Expression(Expression::Call {
            name: exit_name,
            arguments,
        })] = then_body.as_slice()
        else {
            decline!(29)
        };
        if arguments.len() != 1 || constant_value(&arguments[0]) != Some(0) {
            decline!(30);
        }
        let exit_name = exit_name.clone();
        // s5: temp(sig);
        if !matches!(s5, Statement::Expression(Expression::Call { name, arguments })
            if name == temp && arguments.len() == 1 && is_sig(&arguments[0]))
        {
            decline!(31);
        }

        // ---- emission (the measured 44-instruction schedule) ----
        self.frame_size = 16;
        self.non_leaf = true;
        self.epilogue_lr_before_gprs = true;
        let virtual_temp = self.fresh_virtual_general();
        let virtual_sig = self.fresh_virtual_general();
        self.callee_saved = vec![virtual_temp, virtual_sig];
        let plan = mwcc_vreg::FramePlan::sized_for(vec![virtual_temp, virtual_sig]);
        self.output.instructions.extend(plan.prologue());
        let result = Eabi::general_result().number;
        let legacy_raise =
            self.behavior.raise_family_style == RaiseFamilyStyle::StagedLoadLinkRegister;
        self.output.instructions.push(if legacy_raise {
            Instruction::AddImmediate {
                d: virtual_sig,
                a: result,
                immediate: 0,
            }
        } else {
            Instruction::move_register(virtual_sig, result)
        });
        let taken = self.fresh_label();
        let load = self.fresh_label();
        let skip_store = self.fresh_label();
        let return_zero = self.fresh_label();
        let after = self.fresh_label();
        let call_label = self.fresh_label();
        let epilogue = self.fresh_label();
        // RANGE: blt into the taken block, ble past it to the load.
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: virtual_sig,
                immediate: 1,
            });
        self.emit_branch_conditional_to(12, 0, taken);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: virtual_sig,
                immediate: bound,
            });
        self.emit_branch_conditional_to(4, 1, load);
        self.bind_label(taken);
        self.output
            .instructions
            .push(Instruction::load_immediate(result, -1));
        self.emit_branch_to(epilogue);
        // LOAD: the address chain in a fresh virtual (takes the freed r3), the
        // element folded through lwzu's pre-decrement.
        self.bind_label(load);
        let address = self.fresh_virtual_general();
        self.emit_address_high(address, &table);
        if legacy_raise {
            self.record_relocation(RelocationKind::Addr16Lo, &table);
            self.output.instructions.push(Instruction::AddImmediate {
                d: address,
                a: address,
                immediate: 0,
            });
        }
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: virtual_sig,
                shift: 2,
            });
        if !legacy_raise {
            self.record_relocation(RelocationKind::Addr16Lo, &table);
            self.output.instructions.push(Instruction::AddImmediate {
                d: address,
                a: address,
                immediate: 0,
            });
        }
        self.output.instructions.push(Instruction::Add {
            d: address,
            a: address,
            b: GENERAL_SCRATCH,
        });
        let loaded_temp = if legacy_raise {
            GENERAL_SCRATCH
        } else {
            virtual_temp
        };
        self.output
            .instructions
            .push(Instruction::LoadWordWithUpdate {
                d: loaded_temp,
                a: address,
                offset: -4,
            });
        // STORE-IF: clear the slot through the updated base.
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: loaded_temp,
                immediate: 1,
            });
        if legacy_raise {
            self.output
                .instructions
                .push(Instruction::move_register(virtual_temp, loaded_temp));
        }
        self.emit_branch_conditional_to(12, 2, skip_store);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: GENERAL_SCRATCH,
            a: address,
            offset: 0,
        });
        // GUARD3: the mixed ==||(&&) chain sharing one cold return block.
        self.bind_label(skip_store);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: virtual_temp,
                immediate: 1,
            });
        self.emit_branch_conditional_to(12, 2, return_zero);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: virtual_temp,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, after);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: virtual_sig,
                immediate: 1,
            });
        self.emit_branch_conditional_to(4, 2, after);
        self.bind_label(return_zero);
        self.output
            .instructions
            .push(Instruction::load_immediate(result, 0));
        self.emit_branch_to(epilogue);
        // CALL-IF: branch over the exit call.
        self.bind_label(after);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: virtual_temp,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, call_label);
        self.output
            .instructions
            .push(Instruction::load_immediate(result, 0));
        self.record_relocation(RelocationKind::Rel24, &exit_name);
        self.output
            .instructions
            .push(Instruction::BranchAndLink { target: exit_name });
        // TAIL: mainline dispatches through CTR; build 163 uses LR and schedules
        // the argument copy between `mtlr` and `blrl`.
        self.bind_label(call_label);
        if legacy_raise {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 12,
                a: virtual_temp,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
            self.output.instructions.push(Instruction::AddImmediate {
                d: result,
                a: virtual_sig,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
        } else {
            self.output
                .instructions
                .push(Instruction::move_register(12, virtual_temp));
            self.output
                .instructions
                .push(Instruction::move_register(result, virtual_sig));
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 12 });
            self.output
                .instructions
                .push(Instruction::BranchToCountRegisterAndLink);
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(result, 0));
        self.bind_label(epilogue);
        self.output.anonymous_label_bump += 13;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
