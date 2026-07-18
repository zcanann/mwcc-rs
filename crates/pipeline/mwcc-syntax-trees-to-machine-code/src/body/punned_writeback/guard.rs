//! Punned-double GUARD writeback: guarded punned-int rewrites feeding a double reassembly.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
                Expression::Binary {
                    operator: BinaryOperator::Subtract,
                    left,
                    right,
                } => {
                    let Some(k) = crate::analysis::constant_value(right) else {
                        return Ok(false);
                    };
                    (left.as_ref(), k)
                }
                other => (other, 0),
            };
            // `(punned >> S) & M` or bare `punned >> S`.
            let (shifted, mask) = match core {
                Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left,
                    right,
                } => {
                    let Some(mask) = crate::analysis::constant_value(right) else {
                        return Ok(false);
                    };
                    (left.as_ref(), Some(mask))
                }
                other => (other, None),
            };
            let Expression::Binary {
                operator: BinaryOperator::ShiftRight,
                left,
                right,
            } = shifted
            else {
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
        let (
            Some(Statement::If {
                condition,
                then_body,
                else_body,
            }),
            stores,
        ) = (function.statements.first(), &function.statements[1..])
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
                        let Some(index) =
                            locals.iter().position(|&(local, _)| local == name.as_str())
                        else {
                            return false;
                        };
                        if !mutated.contains(&index) {
                            mutated.push(index);
                        }
                        // The chain `i0 = i1 = C`: both locals mutate from
                        // one small constant.
                        if let Expression::Assign {
                            target,
                            value: inner_value,
                        } = value
                        {
                            let Expression::Variable(inner) = target.as_ref() else {
                                return false;
                            };
                            let Some(inner_index) = locals
                                .iter()
                                .position(|&(local, _)| local == inner.as_str())
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
                        (Expression::Variable(a), Expression::Variable(b)) if a == x && b == x) => {
                    }
                    Statement::If {
                        condition: _,
                        then_body,
                        else_body,
                    } => {
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
        if !validate_block(
            block,
            &locals,
            x,
            &mut mutated,
            &mut inner_conditions,
            &mut else_arms,
        ) {
            return Ok(false);
        }
        if !else_body.is_empty() {
            else_arms += 1;
            if !validate_block(
                else_body,
                &locals,
                x,
                &mut mutated,
                &mut inner_conditions,
                &mut else_arms,
            ) {
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
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    } => {
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
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    } => {
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
                Statement::Assign {
                    name: target,
                    value,
                } => target.as_str() == name && crate::analysis::constant_value(value).is_none(),
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => block_self_masks(then_body, name) || block_self_masks(else_body, name),
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
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left,
                right,
            } => {
                let zero = match right.as_ref() {
                    Expression::FloatLiteral(value) => Some(value.to_bits()),
                    _ => None,
                };
                let huge = match left.as_ref() {
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: huge,
                        right: xvar,
                    } => {
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
            let Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } = condition
            else {
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
            let non_condition = block_reads(block, guard.name)
                - block_condition_reads(block, guard.name)
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
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
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
                Statement::If {
                    then_body,
                    else_body,
                    ..
                } => !else_body.is_empty() && covered(then_body, name) && covered(else_body, name),
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
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        let hoisted = if guard_local.is_none() && float_guard.is_none() {
            Some(self.emit_condition_test(condition)?)
        } else {
            None
        };
        if let Some((huge, _)) = float_guard {
            // The huge pool load precedes the spill (measured).
            self.load_double_constant(0, huge);
        }
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: 1,
                a: 1,
                offset: 8,
            });
        if let Some((_, zero)) = float_guard {
            // fadd f1,f0,f1 clobbers x's register — the spill covers the
            // tail's reload; the pooled 0.0 loads before the int reads.
            self.output
                .instructions
                .push(Instruction::FloatAddDouble { d: 1, a: 0, b: 1 });
            self.load_double_constant(0, zero);
        }
        for (index, &(_, offset)) in locals.iter().enumerate() {
            self.output.instructions.push(Instruction::LoadWord {
                d: registers[index],
                a: 1,
                offset: 8 + offset,
            });
        }
        if float_guard.is_some() {
            // No has_float_branch bump: the writeback's fcmpo+ble counts
            // only the arm's own labels (measured: pool @50 vs +3's @53).
            self.output
                .instructions
                .push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
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
                    self.output
                        .instructions
                        .push(Instruction::ShiftRightAlgebraicImmediate {
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
        let outer_laddered =
            !else_body.is_empty() || (guard_local.is_some() && guard_compare.is_none());
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
                            self.output.instructions.push(
                                Instruction::AddImmediateCarryingRecord {
                                    d: 0,
                                    a: guard_register,
                                    immediate: negative,
                                },
                            );
                        } else {
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: 0,
                                a: guard_register,
                                immediate: negative,
                            });
                            self.output
                                .instructions
                                .push(Instruction::CompareWordImmediate {
                                    a: 0,
                                    immediate: bound,
                                });
                        }
                    } else {
                        self.output
                            .instructions
                            .push(Instruction::CompareWordImmediate {
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
            self.output.instructions.push(Instruction::StoreWord {
                s: registers[index],
                a: 1,
                offset: 8 + offset,
            });
        }
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
                    Statement::If {
                        then_body,
                        else_body,
                        ..
                    } => count_fadd_returns(then_body) + count_fadd_returns(else_body),
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
}
