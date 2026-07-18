//! Guard-sequence emission: the value-tracking DIRECT fold's conditional-return sequence.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Whether build 163 can let `current` branch from the CR0 value produced by
    /// `previous`.  The false path of an early-return guard reaches the next
    /// guard without executing an instruction that changes CR0.
    fn guard_reuses_previous_cr0(
        &self,
        previous: &Expression,
        current: &Expression,
    ) -> bool {
        self.behavior.integer_select_style
            == mwcc_versions::IntegerSelectStyle::BranchPreserving
            && shares_condition_register(previous, current)
            && self.comparison_operands_signed(previous)
    }

    /// Return the branch-if-false encoding for a comparison whose operands are
    /// already represented in CR0.
    fn reused_guard_branch(condition: &Expression) -> Compilation<(u8, u8)> {
        let Expression::Binary { operator, .. } = condition else {
            return Err(Diagnostic::error(
                "guard CR0 reuse expects a comparison (roadmap)",
            ));
        };
        false_branch_bo_bi(*operator).ok_or_else(|| {
            Diagnostic::error("guard CR0 reuse expects a relational comparison (roadmap)")
        })
    }

    pub(crate) fn emit_guard_sequence(
        &mut self,
        guards: &[GuardedReturn],
        final_return: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let final_in_result = match final_return {
            Expression::Variable(name) => {
                self.locations.get(name).map(|location| location.register) == Some(result)
            }
            _ => false,
        };

        // mwcc reuses one `cmpwi` across consecutive guards that test the same operand against the
        // same constant: `if (a < 0) ...; if (a == 0) ...` shares `cmpwi r3,0`, the second guard
        // branching on the same result (`bne`). Build 163 preserves the guards and threads CR0;
        // mainline may instead remove the second comparison through a branchless tail fold.
        let guard_count = guards.len();
        for (pair_index, pair) in guards.windows(2).enumerate() {
            if let (Some(first), Some(second)) = (
                guard_comparison_key(&pair[0].condition),
                guard_comparison_key(&pair[1].condition),
            ) {
                if first == second {
                    if self.guard_reuses_previous_cr0(
                        &pair[0].condition,
                        &pair[1].condition,
                    ) {
                        continue;
                    }
                    // When the SECOND guard of the pair is the LAST guard, it folds with the final
                    // return into a select (the `is_last` path below). If that select lowers
                    // branchlessly (sign-mask `srawi`/`srwi`, or a consecutive-constant sign select)
                    // it emits NO compare, so the shared key produces no redundant compare and no
                    // cross-guard CR reuse is needed — mwcc emits one compare for the earlier guard
                    // and the branchless tail (e.g. `if(a>0)return 1; if(a<0)return -1; return 0;` ->
                    // `cmpwi;ble;li 1;blr; srawi;blr`; or a `> 0 ? 2 : 3` tail -> `neg;andc;srawi;
                    // addi`). Compare-based tails (==0/!=0/<=0/variable) are NOT branchless here and
                    // keep deferring (they also defer in evaluate_tail), so no DIFF is shipped.
                    let second_is_last = pair_index + 2 == guard_count;
                    if second_is_last
                        && (!final_in_result || constant_value(&pair[1].value).is_some())
                    {
                        let select = if_select(
                            &pair[1].condition,
                            &pair[1].value,
                            final_return,
                            mwcc_syntax_trees::ConditionalOrigin::IfReturns,
                            self.behavior.integer_select_style
                                == mwcc_versions::IntegerSelectStyle::Branchless,
                        );
                        if let Expression::Conditional {
                            condition,
                            when_true,
                            when_false,
                            ..
                        } = &select
                        {
                            if crate::control_flow::select_folds_branchless(
                                condition, when_true, when_false,
                            ) {
                                continue;
                            }
                        }
                    }
                    return Err(Diagnostic::error(
                        "consecutive guards sharing a compare need cross-guard CR reuse (roadmap)",
                    ));
                }
            }
        }

        for (index, guard) in guards.iter().enumerate() {
            let is_last = index + 1 == guards.len();
            let reuse_cr0 = index > 0
                && self.guard_reuses_previous_cr0(
                    &guards[index - 1].condition,
                    &guard.condition,
                );

            // A null-guarded dereference `if (!p) return CONST; return *p;` cannot fold branchless
            // (dereferencing null is unsafe); mwcc emits a real branch with the deref in the
            // fall-through and the constant as the cold tail: `cmplwi p,0; beq COLD; <*p>; blr;
            // COLD: li CONST; blr`.
            if is_last {
                if let Some((pointer, hot, cold)) = guarded_null_dereference(
                    &guard.condition,
                    &guard.value,
                    final_return,
                    return_type,
                ) {
                    if self.emit_guarded_null_access(
                        &guard.condition,
                        pointer,
                        hot,
                        cold,
                        return_type,
                        result,
                    )? {
                        return Ok(());
                    }
                }
            }

            // mwcc compiles the final guard together with the fall-through return as
            // one branchless select `(cond) ? value : final` — the same form as a
            // lone guard — not a third early-return branch. Earlier guards stay as
            // forward-branching early returns.
            // The last guard folds into the fall-through as a single select `(cond) ? value :
            // final` whenever the final isn't already in the result register, OR the guard value
            // is a constant (the select's constant-arm forms cover `(a>10) ? 1 : a` etc., which
            // the in-result `bnelr` path below cannot — it needs a register value).
            if is_last && (!final_in_result || constant_value(&guard.value).is_some()) {
                // The legacy tail is still a source-level guard diamond.  Its
                // first branch reads the previous guard's comparison directly;
                // evaluating a synthesized select would redundantly re-compare.
                if reuse_cr0 {
                    let (options, condition_bit) = Self::reused_guard_branch(&guard.condition)?;
                    let false_branch = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalForward {
                            options,
                            condition_bit,
                            target: 0,
                        });
                    self.evaluate_tail(&guard.value, return_type, result)?;
                    self.output
                        .instructions
                        .push(Instruction::BranchToLinkRegister);
                    let false_arm = self.output.instructions.len();
                    self.patch_forward(false_branch, false_arm);
                    self.evaluate_tail(final_return, return_type, result)?;
                    self.output
                        .instructions
                        .push(Instruction::BranchToLinkRegister);
                    return Ok(());
                }
                let select = if_select(
                    &guard.condition,
                    &guard.value,
                    final_return,
                    mwcc_syntax_trees::ConditionalOrigin::IfReturns,
                    self.behavior.integer_select_style
                        == mwcc_versions::IntegerSelectStyle::Branchless,
                );
                // ATTEMPT the select; when its lowering has no vocabulary for
                // the fall-through (a table load, a cast) mwcc uses a real
                // early-return BRANCH instead (measured: `cmpwi;bne;li;blr;
                // <table>;blr` for the ctype shape) — roll back and continue
                // the loop, which emits the guard as an early return and the
                // final via the fall-through below.
                let instructions_before = self.output.instructions.len();
                let relocations_before = self.output.relocations.len();
                let virtuals_before = self.next_virtual;
                let bump_before = self.output.anonymous_label_bump;
                match self.evaluate_tail(&select, return_type, result) {
                    Ok(()) => {
                        self.output
                            .instructions
                            .push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                    Err(_) => {
                        self.output.instructions.truncate(instructions_before);
                        self.output.relocations.truncate(relocations_before);
                        self.next_virtual = virtuals_before;
                        self.output.anonymous_label_bump = bump_before;
                    }
                }
            }

            let (options, condition_bit) = if reuse_cr0 {
                Self::reused_guard_branch(&guard.condition)?
            } else {
                self.emit_condition_test(&guard.condition)?
            };
            // A (non-last) guard with a CONSTANT value: forward-branch past the return when the
            // condition is false, load the constant into the result, and return —
            // `cmpwi; bge skip; li result, c; blr; skip:`. (A constant has no leaf register, so the
            // leaf paths below would defer at general_register_of_leaf.)
            if let Some(constant) = constant_value(&guard.value) {
                let branch_index = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                self.load_integer_constant(result, constant);
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegister);
                let next = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[branch_index]
                {
                    *target = next;
                }
                continue;
            }
            let value_register = self.general_register_of_leaf(&guard.value)?;

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options,
                        condition_bit,
                    });
                if result != value_register {
                    self.output
                        .instructions
                        .push(Instruction::move_register(result, value_register));
                }
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            // A non-last guard whose value already sits in the result register is a
            // conditional return falling through to the next guard (mwcc: `cmpwi; bnelr`),
            // not a forward branch over the return.
            if result == value_register {
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                continue;
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
                .push(Instruction::move_register(result, value_register));
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let next = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_index]
            {
                *target = next;
            }
        }

        // Final fall-through return.
        self.evaluate_tail(final_return, return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(())
    }
}
