//! Integer conditional (if/else) emission, condition tests, forward-branch patching.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Type;

impl Generator {
    pub(crate) fn patch_forward(&mut self, branch_index: usize, target: usize) {
        if let Instruction::BranchConditionalForward { target: slot, .. } =
            &mut self.output.instructions[branch_index]
        {
            *slot = target;
        }
    }

    pub(crate) fn emit_conditional(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
        origin: ConditionalOrigin,
    ) -> Compilation<()> {
        // A logical (&&/||) condition feeding a select/guard would compute the operator as a
        // 0/1 value and then re-normalize/select on it (`(a&&b) ? 1 : 0` -> `(a&&b) != 0`),
        // whereas mwcc short-circuits the logical operator directly into the arms (each term
        // branches to the return blocks: `cmpwi r3,0; beq END; cmpwi r4,0; beq END; li
        // r3,1`). That short-circuit-to-arms lowering is the general control-flow path
        // (roadmap #21); until then defer rather than ship the normalize-shaped diff. (A bare
        // `return a && b` goes through evaluate_general, not here, and stays byte-exact.)
        if matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
                ..
            }
        ) {
            return Err(Diagnostic::error("a logical (&&/||) condition in a select/guard needs short-circuit lowering (roadmap #21)"));
        }

        if self.try_emit_legacy_nested_phi_select(
            condition,
            when_true,
            when_false,
            destination,
            tail,
            origin,
        )? {
            return Ok(());
        }

        // `cond ? <leaf/const> : <nested select>` — a ternary chain like `a==1 ? 10 : (a==2 ? 20
        // : 0)`. In tail position mwcc tests the condition, returns the true arm early when it
        // holds, and emits the false arm (the next select) as the fall-through:
        // `cmpwi a,1; bne else; li r3,10; blr; else: <a==2?20:0>`. Emit that and recurse into the
        // false arm; the caller's `blr` ends the fall-through. The true arm must be a placeable
        // leaf/constant; a computed true arm with a nested false arm is left to defer.
        if tail {
            if let Expression::Conditional {
                condition: inner_condition,
                when_true: inner_true,
                when_false: inner_false,
                origin: inner_origin,
            } = when_false
            {
                if leaf_name(when_true).is_some() || constant_value(when_true).is_some() {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    let branch_index = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalForward {
                            options,
                            condition_bit,
                            target: 0,
                        });
                    self.place_select_value(when_true, destination)?;
                    self.output
                        .instructions
                        .push(Instruction::BranchToLinkRegister);
                    let else_label = self.output.instructions.len();
                    if let Instruction::BranchConditionalForward { target, .. } =
                        &mut self.output.instructions[branch_index]
                    {
                        *target = else_label;
                    }
                    return self.emit_conditional(
                        inner_condition,
                        inner_true,
                        inner_false,
                        destination,
                        tail,
                        *inner_origin,
                    );
                }
            }
            // `cond ? <leaf/const> : <memory read>` — the ctype tolower shape
            // (`c == -1 ? -1 : map[(u8)c]`): the same early-return layout, the
            // memory-reading false arm as the fall-through (measured: cmpwi;
            // bne ELSE; li r3,-1; blr; ELSE: <the load>; caller's blr).
            if matches!(
                when_false,
                Expression::Index { .. } | Expression::Dereference { .. }
            ) && (leaf_name(when_true).is_some() || constant_value(when_true).is_some())
            {
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                let else_label = self.fresh_label();
                self.emit_branch_conditional_to(options, condition_bit, else_label);
                self.place_select_value(when_true, destination)?;
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegister);
                self.bind_label(else_label);
                return self.evaluate_general(when_false, destination);
            }
        }

        // Build 163 preserves a simple integer tail select as two source-level
        // return arms. Later builds strength-reduce these leaf/constant shapes
        // (sign masks, clamps, consecutive constants) into arithmetic.
        let simple_arm =
            |arm: &Expression| leaf_name(arm).is_some() || constant_value(arm).is_some();
        let integer_arms = !self.is_float_value(when_true) && !self.is_float_value(when_false);
        if self.try_emit_legacy_framed_simple_select(
            condition,
            when_true,
            when_false,
            destination,
            tail,
            origin,
        )? {
            return Ok(());
        }
        if self.try_emit_legacy_phi_select(
            condition,
            when_true,
            when_false,
            destination,
            tail,
            origin,
        )? {
            return Ok(());
        }
        if self.try_emit_legacy_computed_select(
            condition,
            when_true,
            when_false,
            destination,
            tail,
            origin,
        )? {
            return Ok(());
        }
        if self.try_emit_legacy_leaf_computed_tail_select(
            condition,
            when_true,
            when_false,
            destination,
            tail,
            origin,
        )? {
            return Ok(());
        }
        if self.behavior.integer_select_style == mwcc_versions::IntegerSelectStyle::BranchPreserving
            && tail
            && !self.non_leaf
            && simple_arm(when_true)
            && simple_arm(when_false)
            && integer_arms
        {
            // A float truth/comparison feeding an integer return select consumes
            // two internal labels in legacy MWCC; the all-integer diamond uses
            // three. The emitted branch/value sequence is otherwise shared.
            let floating_condition = match condition {
                Expression::Binary { left, right, .. } => {
                    self.is_float_value(left) || self.is_float_value(right)
                }
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand,
                } => match operand.as_ref() {
                    Expression::Binary { left, right, .. } => {
                        self.is_float_value(left) || self.is_float_value(right)
                    }
                    operand => self.is_float_value(operand),
                },
                _ => self.is_float_value(condition),
            };
            self.output.anonymous_label_bump += if floating_condition { 2 } else { 3 };
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let arm_is_destination = |arm: &Expression| {
                leaf_name(arm)
                    .and_then(|name| self.lookup_general(name))
                    .is_some_and(|register| register == destination)
            };
            if arm_is_destination(when_true) {
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                self.place_select_value(when_false, destination)?;
                return Ok(());
            }
            if arm_is_destination(when_false) {
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options,
                        condition_bit,
                    });
                self.place_select_value(when_true, destination)?;
                return Ok(());
            }
            let false_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.place_select_value(when_true, destination)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let false_arm = self.output.instructions.len();
            self.patch_forward(false_branch, false_arm);
            self.place_select_value(when_false, destination)?;
            return Ok(());
        }

        // Build 163 preserves the source-level branch diamond for canonical
        // INTEGER boolean selects. Later 2.4.x builds strength-reduce these to
        // the branchless comparison idioms handled below.
        let bool_constants = matches!(
            (constant_value(when_true), constant_value(when_false)),
            (Some(1), Some(0)) | (Some(0), Some(1))
        );
        let integer_condition = match condition {
            Expression::Binary {
                operator,
                left,
                right,
            } if is_comparison(*operator) => {
                !self.is_float_value(left) && !self.is_float_value(right)
            }
            _ => !self.is_float_value(condition),
        };
        if self.behavior.integer_select_style == mwcc_versions::IntegerSelectStyle::BranchPreserving
            && bool_constants
            && integer_condition
        {
            self.output.anonymous_label_bump += 3;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let false_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.place_select_value(when_true, destination)?;
            let join_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            let false_arm = self.output.instructions.len();
            self.patch_forward(false_branch, false_arm);
            self.place_select_value(when_false, destination)?;
            let join = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
                *target = join;
            }
            return Ok(());
        }

        // `comparison ? 1 : 0` is the comparison; `comparison ? 0 : 1` is its negation.
        if let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        {
            if is_comparison(*operator) {
                // The `(cmp) ? 1 : 0` / `? 0 : 1` TERNARY advances mwcc's anonymous-`@N` counter by
                // 3 (the ternary construct), like the non-comparison `?1:0` path below; a direct
                // `return a > b` does not. Only observable in a frame function's extab numbering.
                // A FLOAT comparison condition bumps elsewhere (its own anonymous block), so guard
                // this to integer comparisons.
                match (constant_value(when_true), constant_value(when_false)) {
                    (Some(1), Some(0)) => {
                        if !self.is_float_value(left) && !self.is_float_value(right) {
                            self.output.anonymous_label_bump += 3;
                        }
                        return self.evaluate_general(condition, destination);
                    }
                    (Some(0), Some(1)) => {
                        if !self.is_float_value(left) && !self.is_float_value(right) {
                            self.output.anonymous_label_bump += 3;
                        }
                        let flipped = flip_comparison(*operator).unwrap();
                        return self.emit_comparison(flipped, left, right, destination);
                    }
                    _ => {}
                }
            }
        }

        // For a non-comparison condition, `cond ? 1 : 0` is the truthiness `cond != 0`
        // and `cond ? 0 : 1` is `cond == 0` — and the value (even a complex one) now
        // computes through the comparison idioms, which the allocator unlocked.
        let condition_is_comparison =
            matches!(condition, Expression::Binary { operator, .. } if is_comparison(*operator));
        if !condition_is_comparison {
            let zero = Expression::IntegerLiteral(0);
            match (constant_value(when_true), constant_value(when_false)) {
                // The `cond ? 1 : 0` / `? 0 : 1` TERNARY — even lowered to a branchless `cond != 0`
                // / `cond == 0` comparison — advances mwcc's anonymous-`@N` counter by 3, like a
                // float conditional branch; a DIRECT `cond != 0` does not. Only visible in a frame
                // function's extab/extabindex numbering (a leaf function has no anonymous symbols).
                (Some(1), Some(0)) => {
                    self.output.anonymous_label_bump += 3;
                    return self.emit_comparison(
                        BinaryOperator::NotEqual,
                        condition,
                        &zero,
                        destination,
                    );
                }
                (Some(0), Some(1)) => {
                    self.output.anonymous_label_bump += 3;
                    return self.emit_comparison(
                        BinaryOperator::Equal,
                        condition,
                        &zero,
                        destination,
                    );
                }
                _ => {}
            }
        }

        // `x < 0 ? -1 : 0` (and its mirror `x >= 0 ? 0 : -1`) is the sign mask:
        // arithmetic-shift the sign bit across the word, `srawi d, x, 31`. The
        // complement `x < 0 ? 0 : -1` instead broadcasts the inverted sign,
        // `srwi d, x, 31; addi d, d, -1` (0/1 then minus one).
        if let Some((value, complemented)) = sign_mask_select(condition, when_true, when_false) {
            if self.signedness_of(value)? {
                let register = self.general_register_of_leaf(value)?;
                if complemented {
                    self.output
                        .instructions
                        .push(Instruction::ShiftRightLogicalImmediate {
                            a: destination,
                            s: register,
                            shift: 31,
                        });
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: destination,
                        a: destination,
                        immediate: -1,
                    });
                } else {
                    self.output
                        .instructions
                        .push(Instruction::ShiftRightAlgebraicImmediate {
                            a: destination,
                            s: register,
                            shift: 31,
                        });
                }
                // This branchless sign-mask SELECT (a ternary) advances mwcc's anonymous-`@N`
                // counter by 3, like the other ternary forms (bool/comparison ternary, float
                // branch); the instructions already match, so only the frame fn's extab @N differs.
                self.output.anonymous_label_bump += 3;
                return Ok(());
            }
        }

        // `(x < 0)` / `(x >= 0)` / `(x > 0)` `? c1 : c2` with consecutive non-zero
        // constants: the shifted sign bit (`srawi`/`srwi`) plus an offset, after a
        // `neg; andc` preamble for the `> 0` case.
        if let Some(select) = sign_consecutive_select(condition, when_true, when_false) {
            if self.signedness_of(select.value)? {
                let register = self.general_register_of_leaf(select.value)?;
                // The shifted sign bit lands in the value's own (now-dead) register, then
                // an `addi` carries the offset to the destination — `srawi r3,r3; addi
                // r0,r3,2`. This keeps it off the scratch, which the `> 0` case needs for
                // its `neg; andc` preamble, and matches mwcc whether the destination is a
                // real register (a return, addi in place) or the scratch (a store).
                let shift_source = match select.preamble {
                    MaskPreamble::None => register,
                    MaskPreamble::Andc => {
                        self.output.instructions.push(Instruction::Negate {
                            d: GENERAL_SCRATCH,
                            a: register,
                        });
                        self.output.instructions.push(Instruction::AndComplement {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                            b: register,
                        });
                        GENERAL_SCRATCH
                    }
                    MaskPreamble::Or => {
                        self.output.instructions.push(Instruction::Negate {
                            d: GENERAL_SCRATCH,
                            a: register,
                        });
                        self.output.instructions.push(Instruction::Or {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                            b: register,
                        });
                        GENERAL_SCRATCH
                    }
                    MaskPreamble::Orc => {
                        self.output.instructions.push(Instruction::Negate {
                            d: GENERAL_SCRATCH,
                            a: register,
                        });
                        self.output.instructions.push(Instruction::OrComplement {
                            a: GENERAL_SCRATCH,
                            s: register,
                            b: GENERAL_SCRATCH,
                        });
                        GENERAL_SCRATCH
                    }
                };
                self.output.instructions.push(if select.arithmetic {
                    Instruction::ShiftRightAlgebraicImmediate {
                        a: register,
                        s: shift_source,
                        shift: 31,
                    }
                } else {
                    Instruction::ShiftRightLogicalImmediate {
                        a: register,
                        s: shift_source,
                        shift: 31,
                    }
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: register,
                    immediate: select.offset,
                });
                return Ok(());
            }
        }

        // `(x == 0) ? c1 : c2` with consecutive constants — the cntlzw 0/1-flag idiom (NOT a sign
        // mask). `cntlzw r0,x` is 32 iff x==0, so the flag `(r0>>5)&1` is `(x==0)?1:0`. When the
        // true arm is the LOWER constant the flag goes to the scratch (`rlwinm r0,r0,27,31,31`) and
        // is negated into the destination (`neg d,r0; addi d,c2` -> c2-(x==0)); when it is the
        // HIGHER constant the flag goes straight to the destination (`srwi d,r0,5; addi d,c2`).
        // The scratch (store/value) destination uses a different register layout in mwcc (the flag
        // can't share r0 with cntlzw), so handle only the in-register destination and defer the rest.
        if destination != GENERAL_SCRATCH {
            if let Some((value, c1, c2)) = zero_equal_consecutive(condition, when_true, when_false)
            {
                let register = self.general_register_of_leaf(value)?;
                self.output
                    .instructions
                    .push(Instruction::CountLeadingZeros {
                        a: GENERAL_SCRATCH,
                        s: register,
                    });
                if c1 < c2 {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        shift: 27,
                        begin: 31,
                        end: 31,
                    });
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: GENERAL_SCRATCH,
                    });
                } else {
                    self.output
                        .instructions
                        .push(Instruction::ShiftRightLogicalImmediate {
                            a: destination,
                            s: GENERAL_SCRATCH,
                            shift: 5,
                        });
                }
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: destination,
                    immediate: c2,
                });
                return Ok(());
            }
        }

        // `(x REL 0) ? x : 0` (clamp-to-zero): a sign mask of x combined with x.
        if self.try_emit_sign_clamp(condition, when_true, when_false, destination)? {
            return Ok(());
        }
        // `(a REL 0) ? b : 0` (b a distinct leaf): the same sign mask of a, combined with b.
        if self.try_emit_masked_select(condition, when_true, when_false, destination)? {
            return Ok(());
        }

        // `cond ? c1 : c2` with consecutive non-zero constants is branchless: the
        // truth value (a -1/0 sign mask or a 0/1 bool) plus the lower constant.
        if self.try_emit_consecutive_constants(condition, when_true, when_false, destination)? {
            return Ok(());
        }

        // `cond ? x : 0` (AND) and `cond ? 0 : x` (ANDC) with a plain truth
        // condition are branchless: a mask all-ones when cond != 0, combined with
        // `x` (a leaf, or a non-zero constant materialized in r0).
        // `(a == K) ? C : 0` (K, C non-zero) — mwcc forms the equality mask with NO compare:
        // `addi t,a,-K; subfic r0,a,K; nor d,t,r0` yields a word whose sign bit is set iff a==K
        // (both `a-K` and `K-a` are 0 then, so the NOR is all-ones); `srawi d,d,31` broadcasts it,
        // and `and d,C,d` keeps C only when set — `addi r4,r3,-2; subfic r0,r3,2; nor r3,r4,r0;
        // li r0,20; srawi r3,r3,31; and r3,r0,r3` for `(a==2)?20:0`. This is what a ternary chain
        // recurses into. The constant C stages in r0, so a real-register destination is required.
        if is_zero_literal(when_false) && destination != GENERAL_SCRATCH {
            if let Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } = condition
            {
                if let (Some(value_register), Some(equal_to), Some(result)) = (
                    leaf_name(left).and_then(|name| self.lookup_general(name)),
                    constant_value(right),
                    constant_value(when_true),
                ) {
                    // `a == 0` uses a different (cntlzw) mask, so this `addi/subfic/nor` form is
                    // only for a non-zero K (and a non-zero result C).
                    if result != 0 && equal_to != 0 {
                        if let Ok(constant) = i16::try_from(equal_to) {
                            if let Some(negated) = constant.checked_neg() {
                                let difference = self.fresh_virtual_general_avoiding(vec![
                                    value_register,
                                    destination,
                                ]);
                                self.output.instructions.push(Instruction::AddImmediate {
                                    d: difference,
                                    a: value_register,
                                    immediate: negated,
                                });
                                self.output
                                    .instructions
                                    .push(Instruction::SubtractFromImmediate {
                                        d: GENERAL_SCRATCH,
                                        a: value_register,
                                        immediate: constant,
                                    });
                                self.output.instructions.push(Instruction::Nor {
                                    a: destination,
                                    s: difference,
                                    b: GENERAL_SCRATCH,
                                });
                                self.load_integer_constant(GENERAL_SCRATCH, result);
                                self.output.instructions.push(
                                    Instruction::ShiftRightAlgebraicImmediate {
                                        a: destination,
                                        s: destination,
                                        shift: 31,
                                    },
                                );
                                self.output.instructions.push(Instruction::And {
                                    a: destination,
                                    s: GENERAL_SCRATCH,
                                    b: destination,
                                });
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        if is_zero_literal(when_false) && !is_zero_literal(when_true) {
            if self.try_emit_branchless_mask(condition, when_true, false, destination)? {
                return Ok(());
            }
        }
        if is_zero_literal(when_true) && !is_zero_literal(when_false) {
            if self.try_emit_branchless_mask(condition, when_false, true, destination)? {
                return Ok(());
            }
        }

        if self.try_emit_absolute_value_select(
            condition,
            when_true,
            when_false,
            destination,
            tail,
        )? {
            return Ok(());
        }

        // mwcc only branches for NON-consecutive constant arms; two consecutive
        // constants (|c1-c2| == 1) always take a branchless mask form. If the
        // branchless path above could not produce it (an unhandled condition),
        // we must defer rather than emit the mismatching branch form.
        let consecutive_constants = matches!(
            (constant_value(when_true), constant_value(when_false)),
            (Some(a), Some(b)) if (a - b).abs() == 1
        );
        // `cond ? cond : C` / `cond ? C : cond` (one arm IS the condition, the
        // other a non-zero constant) when the condition leaf occupies the result
        // register: the branch form below materializes the constant into the
        // result first, clobbering the condition before the aliasing arm is read.
        // mwcc keeps the value in r0 (`li r0,C; …; mr r0,cond; mr d,r0`); until
        // that form is modeled we defer rather than miscompile. A non-constant
        // other arm (`cond ? b : cond`) takes the register-move path, which reads
        // the condition before overwriting it, so it stays correct.
        if let Some(condition_register) =
            leaf_name(condition).and_then(|name| self.lookup_general(name))
        {
            let aliases_constant_arm = (same_operand(when_true, condition)
                && constant_value(when_false).is_some_and(|c| c != 0))
                || (same_operand(when_false, condition)
                    && constant_value(when_true).is_some_and(|c| c != 0));
            if tail && condition_register == destination && aliases_constant_arm {
                return Err(Diagnostic::error("cond ? cond : C with the condition in the result register needs the r0 form (roadmap)"));
            }
        }

        // `(cond) ? const : <computed>` (and the mirror) in tail position — one arm a non-zero
        // constant, the other a COMPUTED expression (not a leaf or constant), as produced by a
        // guard with a computed fall-through: `if (a < 0) return -1; return a + 100;`. mwcc stages
        // the constant in r0, forward-branches past the computed arm when the condition selects
        // the constant, evaluates the computed arm into r0, then `mr dest, r0` (the shared epilogue
        // follows the converged `mr` for a non-leaf): `cmpwi r3,0; li r0,-1; blt skip; addi
        // r0,r3,100; skip: mr r3,r0`. Placed before the leaf/constant branch selects below, gated
        // to the computed-arm case so it never intercepts those.
        // Fires in tail position (`mr dest, r0` then the epilogue) and in a value/store context
        // where the destination is r0 (the store then writes r0) — both stage in r0, only the
        // tail/value `mr` differs and is keyed off `destination != r0`.
        // The computed arm is restricted to a SIMPLE ARITHMETIC expression (see
        // is_simple_arithmetic_arm) — that is the only arm shape mwcc materializes with this plain
        // branch select. A comparison (0/1 idiom), load (deref/member/index), call, or cast arm
        // uses different codegen and must NOT be intercepted here (it would emit wrong bytes).
        if (tail || destination == GENERAL_SCRATCH)
            && !is_zero_literal(when_true)
            && !is_zero_literal(when_false)
        {
            let plan = match (constant_value(when_true), constant_value(when_false)) {
                (Some(c), None) if is_simple_arithmetic_arm(when_false) => {
                    Some((c, when_false, true))
                }
                (None, Some(c)) if is_simple_arithmetic_arm(when_true) => {
                    Some((c, when_true, false))
                }
                _ => None,
            };
            if let Some((const_value, computed_arm, const_is_true)) = plan {
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                // Branch (skipping the computed arm) when the condition selects the constant arm:
                // the negated skip-when-false test for a true-arm constant, the test itself otherwise.
                let branch_options = if const_is_true { options ^ 8 } else { options };
                // When the destination is a real register the computed arm does NOT read, stage the
                // constant directly in it and conditionally return — `li r3,-1; bltlr; addi r3,r4,1`
                // — no r0 staging or trailing `mr`. (If the arm reads the destination, `li dest,c`
                // would clobber the value it needs, so the r0-staged form below is used instead.)
                if tail
                    && destination != GENERAL_SCRATCH
                    && !self.registers_used_by(computed_arm).contains(&destination)
                {
                    self.load_integer_constant(destination, const_value);
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalToLinkRegister {
                            options: branch_options,
                            condition_bit,
                        });
                    self.evaluate_general(computed_arm, destination)?;
                    return Ok(());
                }
                self.load_integer_constant(GENERAL_SCRATCH, const_value);
                let branch_index = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options: branch_options,
                        condition_bit,
                        target: 0,
                    });
                self.evaluate_general(computed_arm, GENERAL_SCRATCH)?;
                let label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[branch_index]
                {
                    *target = label;
                }
                if destination != GENERAL_SCRATCH {
                    self.output
                        .instructions
                        .push(Instruction::move_register(destination, GENERAL_SCRATCH));
                }
                return Ok(());
            }
        }

        // `(cond) ? <computed> : <computed>` in tail position — both arms computed (neither a
        // leaf or constant). mwcc stages the FALSE arm in r0, forward-branches past the true arm
        // when the condition is false (keeping the false arm), evaluates the true arm into r0,
        // then `mr dest, r0`: `cmpwi r3,0; addi r0,r3,-1; bge skip; addi r0,r3,1; skip: mr r3,r0`.
        if tail || destination == GENERAL_SCRATCH {
            if is_simple_arithmetic_arm(when_true) && is_simple_arithmetic_arm(when_false) {
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                // When the destination is a real register that NEITHER arm reads, stage the false
                // arm directly in it and conditionally return — `addi r3,r4,-1; bgelr; addi r3,r4,1`
                // — no r0 staging or trailing `mr`.
                if tail
                    && destination != GENERAL_SCRATCH
                    && !self.registers_used_by(when_true).contains(&destination)
                    && !self.registers_used_by(when_false).contains(&destination)
                {
                    self.evaluate_general(when_false, destination)?;
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalToLinkRegister {
                            options,
                            condition_bit,
                        });
                    self.evaluate_general(when_true, destination)?;
                    return Ok(());
                }
                self.evaluate_general(when_false, GENERAL_SCRATCH)?;
                let branch_index = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                self.evaluate_general(when_true, GENERAL_SCRATCH)?;
                let label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[branch_index]
                {
                    *target = label;
                }
                if destination != GENERAL_SCRATCH {
                    self.output
                        .instructions
                        .push(Instruction::move_register(destination, GENERAL_SCRATCH));
                }
                return Ok(());
            }
        }

        // `(cond) ? <leaf> : <arithmetic>` in tail position — the true/early arm is a register
        // leaf, the false/fall-through arm a SIMPLE ARITHMETIC computation: `if (a < 0) return b;
        // return a + 1;` (return the cached value, else compute). mwcc computes the false arm into
        // the destination, returns it on the false branch (`bgelr`), then moves the true leaf over
        // for the true path: `cmpwi r3,0; addi r3,r3,1; bgelr; mr r3,r4`. Restricted to the simple
        // arithmetic arm shape (a comparison/load/call/cast false-arm uses different codegen).
        if tail {
            if let Some(leaf) = leaf_name(when_true) {
                if is_simple_arithmetic_arm(when_false) {
                    if let Some(leaf_register) = self.lookup_general(leaf) {
                        if leaf_register != destination {
                            let (options, condition_bit) = self.emit_condition_test(condition)?;
                            self.evaluate_general(when_false, destination)?;
                            self.output.instructions.push(
                                Instruction::BranchConditionalToLinkRegister {
                                    options,
                                    condition_bit,
                                },
                            );
                            self.output
                                .instructions
                                .push(Instruction::move_register(destination, leaf_register));
                            return Ok(());
                        }
                    }
                }
            }
        }

        // `(cond) ? <arithmetic> : <leaf>` in tail position — the mirror of the case above: the
        // true/early arm is a SIMPLE ARITHMETIC computation, the false/fall-through arm a register
        // leaf, as in `if (a < 0) return a + 1; return b;`. mwcc forward-branches past the computed
        // arm when the condition is false (keeping the leaf in its register), evaluates the true
        // arm INTO the leaf's register, then `mr dest, leaf_reg`:
        // `cmpwi r3,0; bge skip; addi r4,r3,1; skip: mr r3,r4`.
        if tail {
            if let Some(leaf) = leaf_name(when_false) {
                if is_simple_arithmetic_arm(when_true) {
                    if let Some(leaf_register) = self.lookup_general(leaf) {
                        if leaf_register != destination {
                            let (options, condition_bit) = self.emit_condition_test(condition)?;
                            let branch_index = self.output.instructions.len();
                            self.output
                                .instructions
                                .push(Instruction::BranchConditionalForward {
                                    options,
                                    condition_bit,
                                    target: 0,
                                });
                            self.evaluate_general(when_true, leaf_register)?;
                            let label = self.output.instructions.len();
                            if let Instruction::BranchConditionalForward { target, .. } =
                                &mut self.output.instructions[branch_index]
                            {
                                *target = label;
                            }
                            self.output
                                .instructions
                                .push(Instruction::move_register(destination, leaf_register));
                            return Ok(());
                        }
                    }
                }
            }
        }

        // `(cond) ? leaf : C` / `(cond) ? C : leaf` — exactly one arm a non-zero
        // constant, the other a register leaf — when materializing the constant into the
        // result register would clobber the leaf before the move could read it. That
        // happens when the leaf is an operand of the condition (`(a>b) ? 7 : b`) OR when the
        // leaf already lives in the result register (`if (c) return 5; return a` with a in
        // r3 — the destination-first `li r3,5; bnelr; mr r3,a` self-move-coalesces the
        // `mr r3,r3` away, silently losing `a`). In both cases mwcc stages the constant in
        // r0, conditionally moves the leaf over it (a forward branch skips the move when the
        // condition selects the constant arm), then `mr dest, r0`. A leaf that is neither
        // (`(c) ? 1 : x`, x in r4) takes the conditional-return (`li r3,C; bnelr; mr r3,x`)
        // below, which clobbers only the spent condition operand.
        if tail
            && !is_zero_literal(when_true)
            && !is_zero_literal(when_false)
            && (constant_value(when_true).is_some() ^ constant_value(when_false).is_some())
        {
            let (const_value, register_arm, negate) =
                if let Some(constant) = constant_value(when_false) {
                    (constant, when_true, false)
                } else {
                    (constant_value(when_true).unwrap(), when_false, true)
                };
            if let Some(name) = leaf_name(register_arm) {
                if let Some(register) = self.lookup_general(name) {
                    if expression_reads_name(condition, name) || register == destination {
                        let (options, condition_bit) = self.emit_condition_test(condition)?;
                        let branch_options = if negate { options ^ 8 } else { options };
                        self.load_integer_constant(GENERAL_SCRATCH, const_value);
                        let branch_index = self.output.instructions.len();
                        self.output
                            .instructions
                            .push(Instruction::BranchConditionalForward {
                                options: branch_options,
                                condition_bit,
                                target: 0,
                            });
                        self.output
                            .instructions
                            .push(Instruction::move_register(GENERAL_SCRATCH, register));
                        let label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } =
                            &mut self.output.instructions[branch_index]
                        {
                            *target = label;
                        }
                        if destination != GENERAL_SCRATCH {
                            self.output
                                .instructions
                                .push(Instruction::move_register(destination, GENERAL_SCRATCH));
                        }
                        return Ok(());
                    }
                }
            }
        }

        // A select with a non-zero constant arm uses a branch, not a register
        // move: mwcc tests the condition, materializes the constant-bearing arm
        // into the result, conditional-returns on that arm's branch, then the
        // other arm. When both are constant the false arm goes first (`beqlr`). A
        // zero arm instead uses the branchless and/andc forms above (with the
        // other arm materialized), whose register layout differs — those defer.
        if tail
            && !consecutive_constants
            && !is_zero_literal(when_true)
            && !is_zero_literal(when_false)
            && (constant_value(when_false).is_some() || constant_value(when_true).is_some())
        {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if constant_value(when_false).is_some() {
                // false-first: place false, return on the false branch, then true.
                self.place_select_value(when_false, destination)?;
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options,
                        condition_bit,
                    });
                self.place_select_value(when_true, destination)?;
            } else {
                // true-first: place true, return on the negated (true) branch, then false.
                self.place_select_value(when_true, destination)?;
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                self.place_select_value(when_false, destination)?;
            }
            return Ok(());
        }

        // The same two-non-zero-constant select into a value/store (not a tail):
        // materialize the false arm, branch forward when the condition is false, then the
        // true arm — `cmpwi; li c2; bne join; li c1; join: stw`. Consecutive constants
        // take the branchless mask forms above; this is the branch case mwcc uses for the
        // rest. (Routed here for `if (cond) tgt = c1; else tgt = c2;`.)
        if !tail
            && !consecutive_constants
            && !is_zero_literal(when_true)
            && !is_zero_literal(when_false)
            && constant_value(when_true).is_some()
            && constant_value(when_false).is_some()
        {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.place_select_value(when_false, destination)?;
            let branch_index = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.place_select_value(when_true, destination)?;
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_index]
            {
                *target = join;
            }
            return Ok(());
        }

        let true_register = self.general_register_of_leaf(when_true)?;
        let false_register = self.general_register_of_leaf(when_false)?;

        // Emit the condition test and the branch that skips the true path when it fails.
        let (options, condition_bit) = self.emit_condition_test(condition)?;

        // In tail position, when the false value already sits in the result
        // register, return early on the false path instead of branching forward.
        if tail && false_register == destination {
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options,
                    condition_bit,
                });
            if destination != true_register {
                self.output
                    .instructions
                    .push(Instruction::move_register(destination, true_register));
            }
            return Ok(());
        }

        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::move_register(false_register, true_register));

        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_index]
        {
            *target = label;
        }
        if destination != false_register {
            self.output
                .instructions
                .push(Instruction::move_register(destination, false_register));
        }
        Ok(())
    }

    /// Emit the test for a branch condition and return the `(BO, BI)` of the
    /// branch that skips the guarded code when the condition is **false**. A
    /// comparison condition uses `cmpw`/`cmpwi` with the negated relation; any
    /// other expression is tested against zero (`!= 0`).
    pub(crate) fn emit_condition_test(&mut self, condition: &Expression) -> Compilation<(u8, u8)> {
        // Inline composition prefixes a condition with ordered local
        // initializations via the comma operator. Emit those effects before
        // selecting instructions for the condition's surviving right value.
        if let Expression::Comma { left, right } = condition {
            self.emit_comma_side_effect(left)?;
            return self.emit_condition_test(right);
        }
        // `!x` as a condition is `x == 0`: skip the guarded code when x != 0.
        if let Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } = condition
        {
            // Negating a comparison only reverses the branch sense; the compare
            // itself (including floating arithmetic and NaN-aware <=/>= setup)
            // remains owned by its ordinary condition path.
            if matches!(operand.as_ref(), Expression::Binary { operator, .. } if is_comparison(*operator))
            {
                let (options, condition_bit) = self.emit_condition_test(operand)?;
                return Ok((options ^ 8, condition_bit));
            }
            // Floating truthiness is an IEEE equality test against +0.0, not an
            // integer register test. Reuse the ordinary float-comparison path so
            // `if (!f)` emits `fcmpu f,0; bne` (and memory operands retain their
            // measured load placement).
            if self.is_float_leaf(operand) {
                let source = self.float_register_of_leaf(operand)?;
                self.load_float_constant(FLOAT_SCRATCH, 0.0);
                self.output
                    .instructions
                    .push(Instruction::FloatCompareUnordered {
                        a: source,
                        b: FLOAT_SCRATCH,
                    });
                return Ok((4, 2)); // bne — skip when the original value is nonzero
            }
            // `!(x & mask)` is the negated bit-test: rlwinm. then `bne` (skip when
            // the masked bits are set, so the body runs only when they are clear).
            if let Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            } = operand.as_ref()
            {
                if self.try_emit_record_mask_test(left, right)? {
                    return Ok((4, 2)); // bne — skip when the masked bits are set
                }
            }
            if let Expression::BitFieldRead { extracted, .. } = operand.as_ref() {
                self.evaluate_bit_field_condition(extracted, GENERAL_SCRATCH)?;
                return Ok((4, 2)); // bne — skip when the field is nonzero
            }
            let register = self.condition_operand_register(operand)?;
            // A signed `char` is sign-extended with the record-form `extsb.` (sets cr0)
            // — ours loads it with `lbz` (zero-extend), so the explicit sign-extend both
            // corrects the value and tests it. A pointer/unsigned operand uses cmplwi, a
            // wider signed one cmpwi; both `beq`/`bne` the same since 0 is 0 either way.
            if matches!(
                as_member(operand),
                Some((_, _, mwcc_syntax_trees::Type::Char))
            ) {
                self.output
                    .instructions
                    .push(Instruction::ExtendSignByteRecord {
                        a: register,
                        s: register,
                    });
            } else if self.is_signed_byte_load(operand)? {
                self.emit_widen_record(GENERAL_SCRATCH, register, 8, true);
            } else if self.signedness_of(operand)? {
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate {
                        a: register,
                        immediate: 0,
                    });
            } else {
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: register,
                        immediate: 0,
                    });
            }
            return Ok((4, 2)); // bne — skip when x != 0
        }
        if let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        {
            // `((a & C) | b) != 0` — the sign/magnitude compound (measured:
            // clrlwi r0,a,N; or. r0,r0,b; beq — the s_floor negative test).
            if matches!(operator, BinaryOperator::NotEqual) && constant_value(right) == Some(0) {
                if let Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: or_left,
                    right: or_right,
                } = left.as_ref()
                {
                    if let Expression::Binary {
                        operator: BinaryOperator::BitAnd,
                        left: and_left,
                        right: and_right,
                    } = or_left.as_ref()
                    {
                        if let (Some(a), Some(mask), Some(b)) = (
                            leaf_name(and_left).and_then(|name| self.lookup_general(name)),
                            constant_value(and_right),
                            leaf_name(or_right).and_then(|name| self.lookup_general(name)),
                        ) {
                            if let Some((begin, end)) = mask_to_run(mask as u32) {
                                self.output.instructions.push(Instruction::RotateAndMask {
                                    a: GENERAL_SCRATCH,
                                    s: a,
                                    shift: 0,
                                    begin,
                                    end,
                                });
                                self.output.instructions.push(Instruction::OrRecord {
                                    a: GENERAL_SCRATCH,
                                    s: GENERAL_SCRATCH,
                                    b,
                                });
                                return Ok((12, 2)); // beq — skip when zero
                            }
                        }
                    }
                }
            }
            // `(a & C) == 0` — the record-form mask (measured: clrlwi.
            // r0,r3,30; bne — the s_floor integral test's other half).
            if matches!(operator, BinaryOperator::Equal) && constant_value(right) == Some(0) {
                if let Expression::Binary {
                    operator: BinaryOperator::BitAnd,
                    left: and_left,
                    right: and_right,
                } = left.as_ref()
                {
                    if self.try_emit_record_mask_test(and_left, and_right)? {
                        return Ok((4, 2)); // bne — skip when masked bits set
                    }
                }
            }
            // `((a & i) | b) == 0` with a VARIABLE mask — and into the
            // scratch, then the record OR with b FIRST (measured V1:
            // and r0,r5,r0; or. r0,r6,r0; bne — the opposite operand
            // order from the constant-mask form).
            if matches!(operator, BinaryOperator::Equal) && constant_value(right) == Some(0) {
                if let Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: or_left,
                    right: or_right,
                } = left.as_ref()
                {
                    if let Expression::Binary {
                        operator: BinaryOperator::BitAnd,
                        left: and_left,
                        right: and_right,
                    } = or_left.as_ref()
                    {
                        if constant_value(and_right).is_none() {
                            if let (Some(a), Some(mask), Some(b)) = (
                                leaf_name(and_left).and_then(|name| self.lookup_general(name)),
                                leaf_name(and_right).and_then(|name| self.lookup_general(name)),
                                leaf_name(or_right).and_then(|name| self.lookup_general(name)),
                            ) {
                                self.output.instructions.push(Instruction::And {
                                    a: GENERAL_SCRATCH,
                                    s: a,
                                    b: mask,
                                });
                                self.output.instructions.push(Instruction::OrRecord {
                                    a: GENERAL_SCRATCH,
                                    s: b,
                                    b: GENERAL_SCRATCH,
                                });
                                return Ok((4, 2)); // bne — skip when non-zero
                            }
                        }
                    }
                }
            }
            // `(a | b) == 0` — the record-form OR sets cr0 in one op
            // (measured: or. r0,r3,r4; bne — the s_floor integral test).
            if matches!(operator, BinaryOperator::Equal) && constant_value(right) == Some(0) {
                if let Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: or_left,
                    right: or_right,
                } = left.as_ref()
                {
                    if let (Some(a), Some(b)) = (
                        leaf_name(or_left).and_then(|name| self.lookup_general(name)),
                        leaf_name(or_right).and_then(|name| self.lookup_general(name)),
                    ) {
                        self.output.instructions.push(Instruction::OrRecord {
                            a: GENERAL_SCRATCH,
                            s: a,
                            b,
                        });
                        return Ok((4, 2)); // bne — skip when the OR is non-zero
                    }
                }
            }
            if is_comparison(*operator) {
                // A floating-point comparison branches off `fcmpo`/`fcmpu`, not `cmpw`.
                // Either side yielding a float (leaf, load, or arithmetic
                // subtree) selects it. Restricting this to direct operands sent
                // `(member * parameter) < 0` through integer register placement.
                if self.is_float_value(left) || self.is_float_value(right) {
                    return self.emit_float_condition(*operator, left, right);
                }
                // Integer immediates are encoded on the right by `cmpwi`/`cmplwi`.
                // Normalize a side-effect-free constant written on the left so
                // `0 <= result` shares the same lowering as `result >= 0`.
                if (constant_value(left) == Some(0) || as_small_integer(left).is_some())
                    && constant_value(right).is_none()
                {
                    let swapped_operator = match operator {
                        BinaryOperator::Less => BinaryOperator::Greater,
                        BinaryOperator::Greater => BinaryOperator::Less,
                        BinaryOperator::LessEqual => BinaryOperator::GreaterEqual,
                        BinaryOperator::GreaterEqual => BinaryOperator::LessEqual,
                        BinaryOperator::Equal => BinaryOperator::Equal,
                        BinaryOperator::NotEqual => BinaryOperator::NotEqual,
                        _ => unreachable!("is_comparison restricts the operator"),
                    };
                    let normalized = Expression::Binary {
                        operator: swapped_operator,
                        left: Box::new((**right).clone()),
                        right: Box::new((**left).clone()),
                    };
                    return self.emit_condition_test(&normalized);
                }
                // Equality against a 32-bit constant with a zero low half does
                // not need a materialized constant register. MWCC subtracts the
                // high half with `addis` and compares the result with zero:
                // `call(); result != 0x80000000` becomes
                // `addis r0,r3,-32768; cmplwi r0,0`.
                if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                    if let Some(constant) = constant_value(right)
                        .and_then(|value| u32::try_from(value).ok())
                        .filter(|value| (*value & 0xffff) == 0 && *value > 0x7fff)
                    {
                        let left_register = self.condition_operand_register(left)?;
                        if left_register != GENERAL_SCRATCH {
                            let high = (constant >> 16) as u16 as i16;
                            self.output.instructions.push(Instruction::AddImmediateShifted {
                                d: GENERAL_SCRATCH,
                                a: left_register,
                                immediate: high.wrapping_neg(),
                            });
                            self.output.instructions.push(
                                Instruction::CompareLogicalWordImmediate {
                                    a: GENERAL_SCRATCH,
                                    immediate: 0,
                                },
                            );
                            return Ok(false_branch_bo_bi(*operator)
                                .expect("equality is a comparison"));
                        }
                    }
                }
                // Two member loads need distinct temporaries. Keep r3 for the
                // left value and reserve it while selecting the right member's
                // address, which naturally gives a global pointer base r4 and
                // the right value r0 (`lbz r3; lwz r0; cmpw r3,r0`). Narrow
                // integer members undergo the C integer promotions here: every
                // char/short variant fits in `int` on this target.
                if let (
                    Some((_, _, left_type)),
                    Some((_, _, right_type)),
                ) = (as_member(left), as_member(right))
                {
                    let operand_registers: std::collections::HashSet<u8> = self
                        .registers_used_by(left)
                        .into_iter()
                        .chain(self.registers_used_by(right))
                        .collect();
                    let newly_reserved: Vec<u8> = operand_registers
                        .into_iter()
                        .filter(|register| self.reserved.insert(*register))
                        .collect();
                    let left_register = self.lowest_free_general()?;
                    for register in newly_reserved {
                        self.reserved.remove(&register);
                    }
                    self.evaluate_general(left, left_register)?;
                    let inserted = self.reserved.insert(left_register);
                    let right_result = self.evaluate_general(right, GENERAL_SCRATCH);
                    if inserted {
                        self.reserved.remove(&left_register);
                    }
                    right_result?;
                    if left_type == Type::Char {
                        self.output.instructions.push(Instruction::ExtendSignByte {
                            a: left_register,
                            s: left_register,
                        });
                    }
                    if right_type == Type::Char {
                        self.output.instructions.push(Instruction::ExtendSignByte {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                        });
                    }
                    let promoted_signed = |value_type: Type| {
                        value_type.width() < 32 || self.signed_of(value_type)
                    };
                    if promoted_signed(left_type) && promoted_signed(right_type) {
                        self.output.instructions.push(Instruction::CompareWord {
                            a: left_register,
                            b: GENERAL_SCRATCH,
                        });
                    } else {
                        self.output.instructions.push(Instruction::CompareLogicalWord {
                            a: left_register,
                            b: GENERAL_SCRATCH,
                        });
                    }
                    return Ok(false_branch_bo_bi(*operator)
                        .expect("is_comparison restricts the operator"));
                }
                // `unsigned u > 0` / `0 < u` is `u != 0`, and `unsigned u <= 0` / `0 >= u` is
                // `u == 0` — as a branch mwcc uses the equality idiom (`bne`/`beq`), not the
                // unsigned relational one (`bgt`/`ble`). Rewrite to the equality and recurse, the
                // same fold emit_comparison applies in value position (canary 856).
                if !(self.signedness_of(left)? && self.signedness_of(right)?) {
                    let folded = match operator {
                        BinaryOperator::Greater if is_zero_literal(right) => {
                            Some((BinaryOperator::NotEqual, left.as_ref(), right.as_ref()))
                        }
                        BinaryOperator::LessEqual if is_zero_literal(right) => {
                            Some((BinaryOperator::Equal, left.as_ref(), right.as_ref()))
                        }
                        BinaryOperator::Less if is_zero_literal(left) => {
                            Some((BinaryOperator::NotEqual, right.as_ref(), left.as_ref()))
                        }
                        BinaryOperator::GreaterEqual if is_zero_literal(left) => {
                            Some((BinaryOperator::Equal, right.as_ref(), left.as_ref()))
                        }
                        _ => None,
                    };
                    if let Some((equality, operand, zero)) = folded {
                        let rewritten = Expression::Binary {
                            operator: equality,
                            left: Box::new(operand.clone()),
                            right: Box::new(zero.clone()),
                        };
                        return self.emit_condition_test(&rewritten);
                    }
                }
                let signed = self.signedness_of(left)? && self.signedness_of(right)?;
                // A memory-valued left operand may need a temporary address GPR.
                // Keep every fixed register read by the right operand live while
                // selecting that address; otherwise `global.field == parameter`
                // can materialize the global base over the parameter and compare
                // against the address it just wrote (SIBios's `Si.chan == chan`).
                let newly_reserved: Vec<u8> = self
                    .registers_used_by(right)
                    .into_iter()
                    .filter(|register| self.reserved.insert(*register))
                    .collect();
                let left_result = self.condition_operand_register(left);
                for register in newly_reserved {
                    self.reserved.remove(&register);
                }
                let left_register = left_result?;
                // An operand whose register isn't already the right width must be
                // extended before the compare: a `short`/`char` leaf in its home register
                // (mwcc emits extsh/extsb/clrlwi, record form against zero), or a *signed*
                // `char` member — loaded with `lbz`, which zero-extends, so mwcc re-extends
                // in place with `extsb`. (A `short`/`ushort`/`uchar` member comes back
                // width-correct from its lha/lhz/lbz load.) `emit_widen` sources from
                // `left_register`, which is the leaf's home register or the member's scratch.
                let left_extend: Option<(u8, bool)> = self
                    .leaf_info(left)
                    .ok()
                    .filter(|&(register, width, _)| register == left_register && width < 32)
                    .map(|(_, width, narrow_signed)| (width, narrow_signed))
                    // A direct call returns in r3, but the EABI does not promise
                    // that the high bits of a narrow result are clean. Treat its
                    // declared return width exactly like a narrow leaf before a
                    // comparison (`u8 status(); status() == 0` -> clrlwi/cmplwi).
                    // This belongs to condition operand typing, not to any one
                    // callee-saved CFG owner.
                    .or_else(|| match left.as_ref() {
                        Expression::Call { name, .. } => self
                            .call_return_types
                            .get(name)
                            .filter(|return_type| {
                                return_type.width() < 32
                                    && !matches!(
                                        return_type,
                                        mwcc_syntax_trees::Type::Float
                                            | mwcc_syntax_trees::Type::Double
                                    )
                            })
                            .map(|return_type| {
                                (return_type.width(), self.signed_of(*return_type))
                            }),
                        Expression::VirtualCall { return_type, .. }
                            if return_type.width() < 32
                                && !matches!(
                                    return_type,
                                    mwcc_syntax_trees::Type::Float
                                        | mwcc_syntax_trees::Type::Double
                                ) =>
                        {
                            Some((return_type.width(), self.signed_of(*return_type)))
                        }
                        _ => None,
                    })
                    .or_else(|| {
                        matches!(as_member(left), Some((_, _, mwcc_syntax_trees::Type::Char)))
                            .then_some((8, true))
                    });
                match (as_small_integer(right), constant_value(right) == Some(0)) {
                    (Some(constant), _) => {
                        let register = if let Some((width, narrow_signed)) = left_extend {
                            self.emit_widen(GENERAL_SCRATCH, left_register, width, narrow_signed);
                            GENERAL_SCRATCH
                        } else {
                            left_register
                        };
                        if signed {
                            self.output
                                .instructions
                                .push(Instruction::CompareWordImmediate {
                                    a: register,
                                    immediate: constant,
                                });
                        } else {
                            self.output.instructions.push(
                                Instruction::CompareLogicalWordImmediate {
                                    a: register,
                                    immediate: constant as u16,
                                },
                            );
                        }
                    }
                    (None, true) => {
                        if let Some((width, narrow_signed)) = left_extend {
                            if matches!(
                                left.as_ref(),
                                Expression::Call { .. } | Expression::VirtualCall { .. }
                            ) && self.behavior.narrow_call_zero_test_style
                                == mwcc_versions::NarrowCallZeroTestStyle::SeparateCompare
                            {
                                self.emit_widen(
                                    GENERAL_SCRATCH,
                                    left_register,
                                    width,
                                    narrow_signed,
                                );
                                if signed {
                                    self.output.instructions.push(
                                        Instruction::CompareWordImmediate {
                                            a: GENERAL_SCRATCH,
                                            immediate: 0,
                                        },
                                    );
                                } else {
                                    self.output.instructions.push(
                                        Instruction::CompareLogicalWordImmediate {
                                            a: GENERAL_SCRATCH,
                                            immediate: 0,
                                        },
                                    );
                                }
                            } else {
                                self.emit_widen_record(
                                    GENERAL_SCRATCH,
                                    left_register,
                                    width,
                                    narrow_signed,
                                );
                            }
                        } else if signed {
                            self.output
                                .instructions
                                .push(Instruction::CompareWordImmediate {
                                    a: left_register,
                                    immediate: 0,
                                });
                        } else {
                            self.output.instructions.push(
                                Instruction::CompareLogicalWordImmediate {
                                    a: left_register,
                                    immediate: 0,
                                },
                            );
                        }
                    }
                    (None, false) => {
                        let left_leaf =
                            self.leaf_info(left).ok().filter(|&(register, width, _)| {
                                register == left_register && width < 32
                            });
                        let right_leaf = self
                            .leaf_info(right)
                            .ok()
                            .filter(|&(_, width, _)| width < 32);
                        match (left_leaf, right_leaf) {
                            (
                                Some((_, left_width, left_signed)),
                                Some((right_register, right_width, right_signed)),
                            ) => {
                                // Two narrow leaves: mwcc extends the first in place and the
                                // second into the scratch, then compares — `extsh r3,r3; extsh
                                // r0,r4; cmpw r3,r0` (the LR store lands after the first extend,
                                // which writes a non-r0 GPR). clrlwi/cmplw for unsigned.
                                self.emit_widen(
                                    left_register,
                                    left_register,
                                    left_width,
                                    left_signed,
                                );
                                self.emit_widen(
                                    GENERAL_SCRATCH,
                                    right_register,
                                    right_width,
                                    right_signed,
                                );
                                if signed {
                                    self.output.instructions.push(Instruction::CompareWord {
                                        a: left_register,
                                        b: GENERAL_SCRATCH,
                                    });
                                } else {
                                    self.output.instructions.push(
                                        Instruction::CompareLogicalWord {
                                            a: left_register,
                                            b: GENERAL_SCRATCH,
                                        },
                                    );
                                }
                            }
                            _ => {
                                // Only one side narrow, or a narrow value mixed with a member/
                                // load — not modeled; defer rather than miscompile.
                                if left_extend.is_some()
                                    || self.is_narrow_leaf(right)
                                    || matches!(
                                        as_member(right),
                                        Some((_, _, mwcc_syntax_trees::Type::Char))
                                    )
                                {
                                    return Err(Diagnostic::error("a mixed narrow comparison needs both operands extended (roadmap)"));
                                }
                                let right_register = self.condition_operand_register(right)?;
                                if signed {
                                    self.output.instructions.push(Instruction::CompareWord {
                                        a: left_register,
                                        b: right_register,
                                    });
                                } else {
                                    self.output.instructions.push(
                                        Instruction::CompareLogicalWord {
                                            a: left_register,
                                            b: right_register,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
                // Branch when the comparison is false — the shared cr0 branch table.
                return Ok(
                    false_branch_bo_bi(*operator).expect("is_comparison restricts the operator")
                );
            }
        }
        // `if (x & mask)` tests the masked bits with a record-form `rlwinm.` that
        // sets cr0 directly — no separate compare.
        if let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } = condition
        {
            if self.try_emit_record_mask_test(left, right)? {
                return Ok((12, 2)); // beq — skip when the masked bits are all zero
            }
        }
        if let Expression::BitFieldRead { extracted, .. } = condition {
            self.evaluate_bit_field_condition(extracted, GENERAL_SCRATCH)?;
            return Ok((12, 2)); // beq — skip when the field is zero
        }
        if self.try_emit_computed_record_condition(condition)? {
            return Ok((12, 2)); // beq — skip when the recorded result is zero
        }
        // A bare floating condition is `f != 0.0`; the guarded body is skipped
        // when equality holds. Equality uses `fcmpu`, matching C's NaN truthiness
        // (NaN is nonzero/true).
        if self.is_float_leaf(condition) {
            let source = self.float_register_of_leaf(condition)?;
            self.load_float_constant(FLOAT_SCRATCH, 0.0);
            self.output
                .instructions
                .push(Instruction::FloatCompareUnordered {
                    a: source,
                    b: FLOAT_SCRATCH,
                });
            return Ok((12, 2)); // beq — skip when the original value is zero
        }
        // Plain truth test: compare against zero, skip when equal. A pointer/unsigned
        // operand uses cmplwi (mwcc), a signed one cmpwi.
        let register = self.condition_operand_register(condition)?;
        // A narrow leaf (`short`/`char` parameter) tests against zero with the record-form
        // extension into the scratch (`extsh. r0,rS` / `clrlwi. r0,rS,24`), the same one the
        // `!= 0` comparison uses. (A `char` member already arrives loaded; mwcc re-extends
        // it in place with `extsb.`.)
        let narrow = self
            .leaf_info(condition)
            .ok()
            .filter(|&(leaf_register, width, _)| leaf_register == register && width < 32)
            .or_else(|| match condition {
                Expression::Call { name, .. } => self
                    .call_return_types
                    .get(name)
                    .filter(|return_type| {
                        return_type.width() < 32
                            && !matches!(
                                return_type,
                                mwcc_syntax_trees::Type::Float
                                    | mwcc_syntax_trees::Type::Double
                            )
                    })
                    .map(|return_type| {
                        (register, return_type.width(), self.signed_of(*return_type))
                    }),
                _ => None,
            });
        if let Some((_, width, narrow_signed)) = narrow {
            if matches!(condition, Expression::Call { .. })
                && self.behavior.narrow_call_zero_test_style
                    == mwcc_versions::NarrowCallZeroTestStyle::SeparateCompare
            {
                self.emit_widen(GENERAL_SCRATCH, register, width, narrow_signed);
                if narrow_signed {
                    self.output
                        .instructions
                        .push(Instruction::CompareWordImmediate {
                            a: GENERAL_SCRATCH,
                            immediate: 0,
                        });
                } else {
                    self.output.instructions.push(
                        Instruction::CompareLogicalWordImmediate {
                            a: GENERAL_SCRATCH,
                            immediate: 0,
                        },
                    );
                }
            } else {
                self.emit_widen_record(GENERAL_SCRATCH, register, width, narrow_signed);
            }
        } else if matches!(
            as_member(condition),
            Some((_, _, mwcc_syntax_trees::Type::Char))
        ) {
            self.output
                .instructions
                .push(Instruction::ExtendSignByteRecord {
                    a: register,
                    s: register,
                });
        } else if self.is_signed_byte_load(condition)? {
            self.emit_widen_record(GENERAL_SCRATCH, register, 8, true);
        } else if self.signedness_of(condition)? {
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate {
                    a: register,
                    immediate: 0,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate {
                    a: register,
                    immediate: 0,
                });
        }
        Ok((12, 2)) // beq — skip when condition == 0
    }

    /// The register holding a condition operand: a leaf variable stays in its home
    /// register; a struct member loads into the scratch (mwcc compares `r0`).
    pub(crate) fn condition_operand_register(&mut self, operand: &Expression) -> Compilation<u8> {
        if let Some((base, offset, member_type)) = as_member(operand) {
            self.emit_member_load(base, offset, member_type, None, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        // A full-word memory load (`*p`, `a[i]`) goes into the scratch; the caller
        // then compares it. (A narrow signed load needs a record-form extend
        // instead of a compare, so it is not taken here.)
        if self.is_word_load(operand) {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        if self.is_signed_byte_load(operand)? {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        // A global has no home register: load it into the scratch (`lwz r0,gv@sda21`)
        // and let the caller compare, like a memory load.
        if self.is_global(operand) {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        // A computed condition has no persistent home register. Calls retain
        // their EABI result in r3 (which is also build 163's compare operand);
        // other newly-supported computed values may use the scratch.
        if matches!(
            operand,
            Expression::Call { .. } | Expression::CallThrough { .. }
        ) {
            let result = mwcc_target::Eabi::general_result().number;
            self.evaluate_general(operand, result)?;
            return Ok(result);
        }
        if matches!(
            operand,
            Expression::Conditional { .. }
                | Expression::Cast { .. }
                | Expression::Binary {
                    operator: BinaryOperator::LogicalAnd
                        | BinaryOperator::LogicalOr
                        | BinaryOperator::BitAnd
                        | BinaryOperator::BitOr,
                    ..
                }
        ) {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        self.general_register_of_leaf(operand)
    }
}
