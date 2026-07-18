//! Shared writeback-block emission for the punned-double families.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
                    if let Expression::Assign {
                        target,
                        value: inner_value,
                    } = value
                    {
                        let Expression::Variable(inner) = target.as_ref() else {
                            return Err(Diagnostic::error(
                                "chained store target beyond the walker (roadmap)",
                            ));
                        };
                        let inner_register = bindings
                            .iter()
                            .find(|(local, _)| local == inner)
                            .map(|&(_, register)| register)
                            .expect("validated");
                        let constant =
                            crate::analysis::constant_value(inner_value).expect("validated");
                        let small = i16::try_from(constant).expect("validated");
                        self.output
                            .instructions
                            .push(Instruction::load_immediate(inner_register, small));
                        self.output
                            .instructions
                            .push(Instruction::load_immediate(register, small));
                        index += 1;
                        continue;
                    }
                    if let Some(constant) = crate::analysis::constant_value(value) {
                        if let Ok(small) = i16::try_from(constant) {
                            self.output
                                .instructions
                                .push(Instruction::load_immediate(register, small));
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::load_immediate_shifted(
                                    register,
                                    (constant >> 16) as i16,
                                ));
                        }
                    } else if let Expression::Binary {
                        operator: BinaryOperator::BitAnd,
                        right,
                        ..
                    } = value
                    {
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
                        return Err(Diagnostic::error(
                            "writeback mutation beyond the walker (roadmap)",
                        ));
                    }
                }
                Statement::Return(Some(value)) => {
                    // `return x+x` raises inexact/inf via fadd before the
                    // epilogue (measured M1: fadd f1,f1,f1; b epi); f1 is
                    // never clobbered on walker paths, so a plain return
                    // is the bare branch.
                    if let Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } = value
                    {
                        if matches!((left.as_ref(), right.as_ref()),
                            (Expression::Variable(a), Expression::Variable(b)) if a == b)
                        {
                            self.output.instructions.push(Instruction::FloatAddDouble {
                                d: 1,
                                a: 1,
                                b: 1,
                            });
                        }
                    }
                    self.emit_branch_to(epilogue);
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
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
                        self.output.instructions.push(Instruction::FloatAddDouble {
                            d: 1,
                            a: 2,
                            b: 1,
                        });
                        self.output
                            .instructions
                            .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
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
                                Some(Statement::If {
                                    then_body,
                                    else_body,
                                    ..
                                }) => {
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
                        return Err(Diagnostic::error(
                            "a non-tail guard in the writeback (roadmap)",
                        ));
                    }
                }
                _ => {
                    return Err(Diagnostic::error(
                        "writeback statement beyond the walker (roadmap)",
                    ))
                }
            }
            index += 1;
        }
        Ok(())
    }
}
