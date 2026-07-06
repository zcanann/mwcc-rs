//! Guard-block mutations: if(cond){ reassign distinct params } with an expression return.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// GUARD-BLOCK MUTATIONS (the s_floor skeleton, fire 377): a chain of
    /// nested no-else ifs whose innermost body only ASSIGNS constants to int
    /// params, followed by a return expression. Every guard branches to ONE
    /// join; the block mutates the params in their own registers; the join
    /// computes the return (measured: `if(c){i0=0;i1=0;} return i0|i1` =
    /// cmpwi; beq J; li; li; J: or; blr — and the nested form re-tests each
    /// guard to the same join).
    pub(crate) fn try_guard_block_mutations(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(return_expression) = &function.return_expression else {
            return Ok(false);
        };
        // Flatten the guard chain: each level is exactly [If{no else}].
        let mut conditions: Vec<&Expression> = Vec::new();
        let mut body: &[Statement] = &function.statements;
        loop {
            match body {
                [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => {
                    conditions.push(condition);
                    body = then_body;
                }
                _ => break,
            }
        }
        if conditions.is_empty() {
            return Ok(false);
        }
        // A MID-CHAIN early return heads the innermost block (measured:
        // `if ((i0|i1)==0) return 7;` = the record-form test, bne PAST the
        // inline return to the mutations).
        let mut early_return: Option<(&Expression, &Expression)> = None;
        if let [Statement::If { condition, then_body, else_body }, rest @ ..] = body {
            if else_body.is_empty() {
                if let [Statement::Return(Some(value))] = then_body.as_slice() {
                    early_return = Some((condition, value));
                    body = rest;
                }
            }
        }
        // The innermost block: assigns to DISTINCT int params — an i16
        // constant (li), a lis-able constant (measured: 0xbff00000), or a
        // leaf-plus-i16 over a param no EARLIER assign in the block already
        // overwrote (measured: i0 = i1 + 1 before i1's own overwrite).
        enum BlockValue {
            Small(i16),
            High(i16),
            LeafAdd(u8, i16),
            Mask(u8, u8),
        }
        let mut assigns: Vec<(u8, BlockValue)> = Vec::new();
        for statement in body {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            let Some(location) = self.locations.get(name.as_str()) else {
                return Ok(false);
            };
            if location.class != ValueClass::General || location.width != 32 {
                return Ok(false);
            }
            let target = location.register;
            let block_value = if let Some(constant) = crate::analysis::constant_value(value) {
                if let Ok(small) = i16::try_from(constant) {
                    BlockValue::Small(small)
                } else if constant & 0xffff == 0 && u32::try_from(constant).is_ok() {
                    BlockValue::High((constant >> 16) as i16)
                } else {
                    return Ok(false);
                }
            } else {
                // Self-masking (`i0 &= C`, desugared): the in-place rlwinm
                // (measured: clrlwi r3,r3,21 in source order).
                if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = value {
                    if let Expression::Variable(read) = left.as_ref() {
                        if read == name {
                            if let Some(mask) = crate::analysis::constant_value(right) {
                                if let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) {
                                    if assigns.iter().any(|&(written, _)| written == target) {
                                        return Ok(false);
                                    }
                                    assigns.push((target, BlockValue::Mask(begin, end)));
                                    continue;
                                }
                            }
                        }
                    }
                    return Ok(false);
                }
                // leaf ± i16 (Add with a possibly-negative constant).
                let (leaf, offset) = match value {
                    Expression::Variable(read) => (read, 0i64),
                    Expression::Binary { operator: BinaryOperator::Add, left, right } => {
                        let Expression::Variable(read) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(offset) = crate::analysis::constant_value(right) else {
                            return Ok(false);
                        };
                        (read, offset)
                    }
                    Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
                        let Expression::Variable(read) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(offset) = crate::analysis::constant_value(right) else {
                            return Ok(false);
                        };
                        (read, -offset)
                    }
                    _ => return Ok(false),
                };
                let Some(read_location) = self.locations.get(leaf.as_str()) else {
                    return Ok(false);
                };
                if read_location.class != ValueClass::General || read_location.width != 32 {
                    return Ok(false);
                }
                let Ok(offset) = i16::try_from(offset) else {
                    return Ok(false);
                };
                if offset == 0 {
                    // A bare register move inside the block is unmeasured.
                    return Ok(false);
                }
                // The read must precede any overwrite of its register — and
                // a SELF-read (i0 = i0 + 5) reorders in mwcc (the
                // independent li hoists above the self-addi; probed) — defer.
                if read_location.register == target
                    || assigns.iter().any(|&(written, _)| written == read_location.register)
                {
                    return Ok(false);
                }
                BlockValue::LeafAdd(read_location.register, offset)
            };
            if assigns.iter().any(|&(register, _)| register == target) {
                return Ok(false);
            }
            assigns.push((target, block_value));
        }
        if early_return.is_none() && assigns.len() < 2 && conditions.len() < 2 {
            // The single-guard single-assign shapes belong to the measured
            // reassign/select arms.
            return Ok(false);
        }
        if assigns.is_empty() {
            return Ok(false);
        }
        // A bare-variable return folds the guards to conditional RETURNS
        // (bclr) instead of branch-to-join — the reassign arms' territory;
        // this arm takes the expression-return join form only (measured).
        if matches!(return_expression, Expression::Variable(_)) {
            return Ok(false);
        }
        // The return must be claimable by the plain tail evaluator: an
        // expression over params (no calls — gated above).
        // -- commit --
        let join = self.fresh_label();
        for condition in conditions {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.emit_branch_conditional_to(options, condition_bit, join);
        }
        if let Some((condition, value)) = early_return {
            let result = Eabi::general_result().number;
            // A bare return of the value already in r3 FOLDS to a
            // conditional return (measured: or.; beqlr).
            if let Expression::Variable(name) = value {
                if self.lookup_general(name) == Some(result) {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                    for (register, block_value) in &assigns {
                        match block_value {
                            BlockValue::Small(constant) => {
                                self.output.instructions.push(Instruction::load_immediate(*register, *constant));
                            }
                            BlockValue::High(high) => {
                                self.output.instructions.push(Instruction::load_immediate_shifted(*register, *high));
                            }
                            BlockValue::LeafAdd(source, offset) => {
                                self.output.instructions.push(Instruction::AddImmediate {
                                    d: *register,
                                    a: *source,
                                    immediate: *offset,
                                });
                            }
                            BlockValue::Mask(begin, end) => {
                                self.output.instructions.push(Instruction::RotateAndMask {
                                    a: *register,
                                    s: *register,
                                    shift: 0,
                                    begin: *begin,
                                    end: *end,
                                });
                            }
                        }
                    }
                    self.bind_label(join);
                    self.evaluate_tail(return_expression, function.return_type, result)?;
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
            }
            // Skip the inline return when the early condition fails; the
            // skip lands on the MUTATIONS, not the join.
            let mutations = self.fresh_label();
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.emit_branch_conditional_to(options, condition_bit, mutations);
            match crate::analysis::constant_value(value) {
                Some(constant) if i16::try_from(constant).is_ok() => {
                    self.output.instructions.push(Instruction::load_immediate(result, constant as i16));
                }
                Some(_) => return Err(Diagnostic::error("early-return constant beyond i16 (roadmap)")),
                None => {
                    self.evaluate_tail(value, function.return_type, result)?;
                }
            }
            self.emit_epilogue_and_return();
            self.bind_label(mutations);
        }
        for (register, value) in &assigns {
            match value {
                BlockValue::Small(constant) => {
                    self.output.instructions.push(Instruction::load_immediate(*register, *constant));
                }
                BlockValue::High(high) => {
                    self.output.instructions.push(Instruction::load_immediate_shifted(*register, *high));
                }
                BlockValue::LeafAdd(source, offset) => {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: *register,
                        a: *source,
                        immediate: *offset,
                    });
                }
                BlockValue::Mask(begin, end) => {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: *register,
                        s: *register,
                        shift: 0,
                        begin: *begin,
                        end: *end,
                    });
                }
            }
        }
        self.bind_label(join);
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expression, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

}
