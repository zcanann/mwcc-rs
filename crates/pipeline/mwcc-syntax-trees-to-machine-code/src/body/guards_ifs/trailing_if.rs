//! Trailing-if emission: standalone if(cond) body, forward/early-return forms, non-leaf first-if.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A trailing leaf `if (c) then; [else otherwise | else if …]` in a void
    /// function. With no else, the false path is a conditional return (the body
    /// then falls through to the function `blr`). With an else, branch over the
    /// then-body (and its `blr`) to the else, which is either a single statement
    /// or a nested trailing if (an `else if` chain). Each then-body is a single
    /// statement — multiple statements need the scheduler.
    pub(crate) fn emit_trailing_if(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
        nested: bool,
    ) -> Compilation<()> {
        // The top-level condition always sets cr0 with a fresh compare.
        self.emit_trailing_if_inner(condition, then_body, else_body, nested, false)
    }

    /// The `(BO, BI)` of the branch that skips a condition's guarded code when the
    /// condition is false. Normally this emits the compare (`emit_condition_test`);
    /// when `reuse_cr0`, the compare already sits in cr0 from a same-operand parent
    /// test, so read the shared branch table instead of re-testing.
    fn condition_branch(
        &mut self,
        condition: &Expression,
        reuse_cr0: bool,
    ) -> Compilation<(u8, u8)> {
        if reuse_cr0 {
            let Expression::Binary { operator, .. } = condition else {
                return Err(Diagnostic::error(
                    "cr0 reuse expects a comparison (roadmap)",
                ));
            };
            return false_branch_bo_bi(*operator).ok_or_else(|| {
                Diagnostic::error("cr0 reuse expects a relational comparison (roadmap)")
            });
        }
        self.emit_condition_test(condition)
    }

    /// Whether a comparison's operands are both signed — the case in which
    /// `emit_condition_test` emits a plain `cmpw`/`cmpwi` with no unsigned
    /// equality-fold, so a second branch can ride the same cr0 via the raw table.
    fn comparison_operands_signed(&self, condition: &Expression) -> bool {
        matches!(condition, Expression::Binary { left, right, .. }
            if self.signedness_of(left).unwrap_or(false) && self.signedness_of(right).unwrap_or(false))
    }

    /// The body of [`emit_trailing_if`], threading `reuse_cr0`: when an `else if`
    /// compares the SAME operand against the SAME value as its parent, mwcc emits ONE
    /// `cmpwi` and both branches read that cr0 (`ble`/`bge` off the same compare). The
    /// recursion sets `reuse_cr0` on such a child so it branches off the inherited cr0
    /// instead of re-testing. When it reaches the child's branch, control arrived via
    /// the parent's taken forward branch, which does not disturb cr0 — so the reuse is
    /// exact. Only a SIGNED comparison qualifies: an unsigned operand-vs-zero test folds
    /// to an equality idiom in `emit_condition_test`, which the raw reuse table would not
    /// match, so that case stays deferred.
    fn emit_trailing_if_inner(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
        nested: bool,
        reuse_cr0: bool,
    ) -> Compilation<()> {
        // `if (cond) g = X; else g = Y;` — both arms a single store to the same GLOBAL — is
        // byte-identical to the select `g = cond ? X : Y;`: mwcc coalesces to ONE store,
        // speculating one value and conditionally overwriting it (constants branchless-ify;
        // registers `mr`; `li r0,Y; beq; mr r0,X; stw` for the mixed/computed forms). Route
        // it through the conditional-store path, which is byte-exact-or-defer for whatever X
        // and Y are — exactly matching the direct-select lowering (so a form it cannot yet
        // reproduce DEFERS, never emits the two-store retest idiom, which is wrong for a
        // single coalesced target). This applies ONLY to a direct global (SDA-addressed)
        // target: a POINTER-dereference store (`*p = 1; else *p = 2;`) keeps the two-exit
        // branch form below (`cmpwi; beq; li; stw; blr; li; stw; blr`).
        //
        // The coalescing (this select shortcut AND the retest idiom below) is a STANDALONE-
        // diamond optimization: mwcc does NOT apply it to a diamond reached through an
        // else-if chain (`if(c) g=1; else if(d) g=2; else g=3;` stays full nested branches,
        // one store per level). When `nested` (this is the recursive else-if tail) both are
        // suppressed so the two-exit branch form is used per level.
        if let (
            [Statement::Store {
                target: then_target,
                value: then_value,
            }],
            [Statement::Store {
                target: else_target,
                value: else_value,
            }],
        ) = (then_body, else_body)
        {
            if !nested
                && same_operand(then_target, else_target)
                && matches!(then_target, Expression::Variable(name) if self.globals.contains_key(name.as_str()))
            {
                let select = Expression::Conditional {
                    condition: Box::new(condition.clone()),
                    when_true: Box::new(then_value.clone()),
                    when_false: Box::new(else_value.clone()),
                };
                return self.emit_store(then_target, &select);
            }
        }
        // A no-else block of two-plus REGISTER-VALUED stores: the conditional return, then the
        // stores in source order. mwcc emits them sequentially — the values are already in
        // registers, so there is nothing to materialize or schedule (`cmpwi;beqlr;stw;stw;blr`). A
        // constant/global/computed store value needs the batch value scheduler and falls through.
        if then_body.len() > 1
            && else_body.is_empty()
            && then_body.iter().all(|statement| matches!(statement,
                Statement::Store { value: Expression::Variable(name), .. } if self.locations.contains_key(name.as_str())))
        {
            let (options, condition_bit) = self.condition_branch(condition, reuse_cr0)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            for statement in then_body {
                self.emit_statement(statement)?;
            }
            return Ok(());
        }
        if then_body.len() != 1 {
            return Err(Diagnostic::error(
                "a multi-statement if-body needs the scheduler (roadmap)",
            ));
        }
        // A nested else-if whose comparison REUSES this comparison's condition register
        // (same operand against the same value — `if(c>0) … else if(c<0) …`, which mwcc
        // lowers with ONE `cmpwi` shared by both branches, `ble`/`bge` off the same CR).
        // A SIGNED comparison reuses cr0 (the child branches off the inherited compare);
        // an unsigned operand-vs-zero test folds to an equality idiom, which the raw reuse
        // table would not match, so that stays deferred. A different operand or value
        // re-tests normally and is unaffected.
        let mut child_reuses_cr0 = false;
        if let [Statement::If {
            condition: else_condition,
            ..
        }] = else_body
        {
            if shares_condition_register(condition, else_condition) {
                if self.comparison_operands_signed(condition) {
                    child_reuses_cr0 = true;
                } else {
                    return Err(Diagnostic::error("consecutive else-if comparisons that reuse the condition register are not supported yet (roadmap)"));
                }
            }
        }
        let (options, condition_bit) = self.condition_branch(condition, reuse_cr0)?;
        if else_body.is_empty() {
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options,
                    condition_bit,
                });
            return self.emit_statement(&then_body[0]);
        }
        // An `else if` chain keeps the two-exit form: the then-arm returns (`blr`), then
        // the nested trailing `if` — reusing this cr0 when the child shares the operand.
        if let [Statement::If {
            condition: else_condition,
            then_body: else_then,
            else_body: else_else,
        }] = else_body
        {
            let branch_index = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.emit_statement(&then_body[0])?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_index]
            {
                *target = label;
            }
            return self.emit_trailing_if_inner(
                else_condition,
                else_then,
                else_else,
                true,
                child_reuses_cr0,
            );
        }
        if else_body.len() != 1 {
            return Err(Diagnostic::error(
                "a multi-statement else-body needs the scheduler (roadmap)",
            ));
        }
        // For a truthy condition (a bare register compare) with global-store arms, mwcc
        // uses the re-test idiom: the then-arm falls through to a *re-test* of the
        // condition and a conditional return, then the else — `cmpwi; beq L; A; L: cmpwi;
        // bnelr; B; blr`. A comparison condition re-tests by branchless recomputation (not
        // a second compare), and member/base-register arms keep the two-exit form; both of
        // those route to the two-exit branch below.
        let truthy = !nested
            && (matches!(condition, Expression::Variable(_))
                || matches!(condition, Expression::Unary { operator: UnaryOperator::LogicalNot, operand } if matches!(operand.as_ref(), Expression::Variable(_))));
        let is_global_store = |statement: &Statement| matches!(statement, Statement::Store { target: Expression::Variable(name), .. } if self.globals.contains_key(name.as_str()));
        let use_retest = truthy && is_global_store(&then_body[0]) && is_global_store(&else_body[0]);
        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.emit_statement(&then_body[0])?;
        if use_retest {
            let label = self.output.instructions.len();
            let (retest_options, retest_bit) = self.emit_condition_test(condition)?;
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: retest_options ^ 8,
                    condition_bit: retest_bit,
                });
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_index]
            {
                *target = label;
            }
        } else {
            // Two-exit form: the then-arm returns, the conditional branch lands on the else.
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_index]
            {
                *target = label;
            }
        }
        self.emit_statement(&else_body[0])?;
        Ok(())
    }

    /// A non-trailing `if (c) { body }`: the false path branches forward over the
    /// body to the code that follows.
    pub(crate) fn emit_if_forward(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
    ) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_index]
        {
            *target = label;
        }
        Ok(())
    }

    /// A leaf `if (c) { … return [v]; }` whose then-body ends in an early return:
    /// forward-branch over the body when the condition is false, emit the body
    /// (the `return` materializes the value and runs the epilogue — `blr` for a
    /// leaf), then patch the branch to land on the continuation (the rest of the
    /// function, which supplies the other exit).
    pub(crate) fn emit_if_early_return(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
        return_type: Type,
    ) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        for statement in then_body {
            if let Statement::Return(value) = statement {
                if let Some(value) = value {
                    let result = match return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate_tail(value, return_type, result)?;
                }
                self.emit_epilogue_and_return();
            } else {
                self.emit_statement(statement)?;
            }
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_index]
        {
            *target = label;
        }
        Ok(())
    }

    /// A non-leaf function whose body begins with `if (c) { …calls…; return X; }`
    /// (the if is the first statement) followed by a straight-line continuation
    /// that supplies the other return. mwcc schedules the condition test into the
    /// prologue (between `mflr` and the LR store), the early return materializes X
    /// and branches to a SHARED epilogue, and the continuation falls into that same
    /// epilogue. Returns whether this path took over the whole body.
    pub(crate) fn try_non_leaf_if_first_early_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        // Shape: `if (c) { body…; return; } continuation…`, the if first, non-leaf,
        // no guards/locals, no else. The general/void return type only (a float
        // early return adds the FP result register — deferred).
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }, rest @ ..] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !function_makes_call(function)
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !else_body.is_empty()
            || matches!(function.return_type, Type::Float | Type::Double)
        {
            return Ok(false);
        }
        // The then-body must be straight-line calls/stores ending in the early
        // return; the continuation must likewise be straight-line (no nested
        // control flow, which would need its own branches).
        let Some((early_return, leading)) = then_body.split_last() else {
            return Ok(false);
        };
        let early_value = match early_return {
            Statement::Return(value) => value,
            _ => return Ok(false),
        };
        // Only calls may sit in the then-body or continuation: a call is a
        // scheduling barrier, so the value materialization that follows stays put.
        // A store would let mwcc's scheduler interleave the value into the store
        // sequence (`li r0,5; li r3,2; stw` rather than `li r0,5; stw; li r3,2`),
        // which this straight-line emission cannot reproduce — defer those.
        let call_only = |statement: &Statement| matches!(statement, Statement::Expression(_));
        if !leading.iter().all(call_only) || !rest.iter().all(call_only) {
            return Ok(false);
        }
        // A void function ends after its statements; a value-returning one supplies
        // the other exit through the trailing `return` expression. The early
        // return's value-ness must match (both void or both a value).
        let returns_value = function.return_type != Type::Void;
        if returns_value != early_value.is_some()
            || returns_value != function.return_expression.is_some()
        {
            return Ok(false);
        }
        // The condition test must be schedulable into the prologue: it cannot itself
        // make a call (that would need its own frame-aware sequencing).
        if expression_has_call(condition) {
            return Ok(false);
        }
        // A value computed AFTER a call on its path cannot be read from a
        // caller-saved register (the call clobbers it); mwcc would spill the source
        // to a callee-saved register (r31) and restructure the whole frame — that
        // is the next subsystem and is deferred. So a return value that follows a
        // call on its own path must be a compile-time constant (no register read).
        // The early return follows the then-body's calls; the continuation value
        // follows the continuation's calls (the false path skipped the then-body).
        let then_calls = leading.iter().any(statement_has_call);
        let rest_calls = rest.iter().any(statement_has_call);
        if then_calls
            && early_value
                .as_ref()
                .is_some_and(|value| constant_value(value).is_none())
        {
            return Ok(false);
        }
        if rest_calls
            && function
                .return_expression
                .as_ref()
                .is_some_and(|value| constant_value(value).is_none())
        {
            return Ok(false);
        }

        let result = Eabi::general_result().number;
        self.non_leaf = true;
        self.frame_size = 16;
        // The if's branch labels advance mwcc's anonymous-`@N` counter by 2.
        self.output.anonymous_label_bump = 2;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        // A BARE void early return (`if (a) return; g();`) has no then-body at all:
        // mwcc folds it to a single INVERTED conditional branch straight to the shared
        // epilogue — `bne EPILOGUE; bl g; EPILOGUE:` — rather than a skip over an
        // unconditional branch.
        if leading.is_empty() && early_value.is_none() {
            let epilogue_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: options ^ 8,
                    condition_bit,
                    target: 0,
                });
            for statement in rest {
                self.emit_statement(statement)?;
            }
            let epilogue_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[epilogue_branch]
            {
                *target = epilogue_label;
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        // False path skips the then-body to the continuation.
        let continuation_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        // The then-body: the leading calls/stores, then the early return's value.
        for statement in leading {
            self.emit_statement(statement)?;
        }
        if let Some(value) = early_value {
            self.evaluate_tail(value, function.return_type, result)?;
        }
        // The early return branches to the shared epilogue. Reserve the slot — if
        // the continuation turns out to emit nothing (e.g. `return a` with `a`
        // already in the result register), mwcc lets the early return fall through
        // to the epilogue rather than branch, so the slot is dropped below.
        let branch_slot = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let continuation_label = self.output.instructions.len();
        for statement in rest {
            self.emit_statement(statement)?;
        }
        if let Some(return_expression) = &function.return_expression {
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        if self.output.instructions.len() == continuation_label {
            // The continuation produced no instructions: the early return falls
            // straight through to the epilogue, and the false path targets the
            // epilogue directly. Drop the unnecessary branch.
            self.output.instructions.remove(branch_slot);
            let epilogue_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[continuation_branch]
            {
                *target = epilogue_label;
            }
        } else {
            // A non-empty continuation: the false path lands on it, and the early
            // return branches over it to the shared epilogue.
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[continuation_branch]
            {
                *target = continuation_label;
            }
            let epilogue_label = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[branch_slot] {
                *target = epilogue_label;
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
