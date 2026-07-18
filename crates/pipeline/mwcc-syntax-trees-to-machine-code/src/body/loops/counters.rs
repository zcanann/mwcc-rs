//! Counter / index loop families (do-while, counted-call, increment-while, for-counter).
//!
//! Split from a single 2795-line `loops.rs` (behavior-identical).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Tear down the stack frame (if one was allocated) and return. A non-leaf
    /// function restores the link register from `frame_size + 4` first.
    /// A `void` function whose whole body is `do { …calls… } while (--counter);`
    /// with the counter a parameter: mwcc keeps the counter in a callee-saved
    /// register (r31), runs the body, then `addic. r31,r31,-1` (decrement, set CR0)
    /// and `bne` back to the loop top. Returns whether this path applied.
    pub(crate) fn try_do_while_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if function.return_type != Type::Void {
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
        let kind = *kind;
        if !matches!(kind, LoopKind::DoWhile | LoopKind::While) {
            return Ok(false);
        }
        // The condition must be `--counter` (a parameter decrement), which the
        // parser lowered to `counter = counter - 1`.
        let counter = match condition {
            Expression::Assign { target, value } => match (target.as_ref(), value.as_ref()) {
                (
                    Expression::Variable(name),
                    Expression::Binary {
                        operator: BinaryOperator::Subtract,
                        left,
                        right,
                    },
                ) if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1)) =>
                {
                    name.clone()
                }
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == counter)
        {
            return Ok(false);
        }
        // The body must be straight-line calls that do not pass the counter as an
        // argument (the first such use would stay in the incoming register — the
        // value-location nuance the callee-saved path also defers).
        if body
            .iter()
            .any(|statement| !matches!(statement, Statement::Expression(_)))
        {
            return Ok(false);
        }
        if body.iter().any(|statement| matches!(statement, Statement::Expression(e) if expression_reads_name(e, &counter))) {
            return Ok(false);
        }
        if !function_makes_call(function) {
            return Ok(false);
        }
        let (class, incoming) = match self.locations.get(&counter) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }

        // Prologue: save the link register and r31, move the counter into r31.
        const SAVED: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![SAVED];
        // The loop's internal labels advance mwcc's anonymous-`@N` counter — by 6
        // for a do-while, by 4 for a while (the extra top branch, fewer labels).
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) {
            // Build 163's older rotated-loop form consumes one fewer internal
            // ordinal even though the final instructions are otherwise the same.
            if self.behavior.frame_convention == FrameConvention::LinkageFirst {
                5
            } else {
                6
            }
        } else {
            4
        };
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: SAVED,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::Or {
            a: SAVED,
            s: incoming,
            b: incoming,
        });
        if let Some(location) = self.locations.get_mut(&counter) {
            location.register = SAVED;
        }

        // A while loop tests the condition first: branch down to the
        // decrement-and-test, which falls through into the body on re-entry.
        let skip_to_condition = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        // The loop body, then the decrement-and-test and the backward branch.
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        if let Some(index) = skip_to_condition {
            let condition = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                *target = condition;
            }
        }
        self.output
            .instructions
            .push(Instruction::AddImmediateCarryingRecord {
                d: SAVED,
                a: SAVED,
                immediate: -1,
            });
        // `bne body_top`: branch when CR0[EQ] is clear (BO=4, BI=2). Backward, which
        // the encoder resolves from the instruction indices.
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: body_top,
            });

        // Epilogue, emitted in final order (the loop's branch makes the scheduler and
        // the LR-reload hoist bail): the LR reload comes before the r31 reload.
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: self.frame_size + 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: SAVED,
            a: 1,
            offset: self.frame_size - 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: self.frame_size,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A `void` function whose whole body is a counted call loop —
    /// `for (i = 0; i < N; i++) g(i);`. The counter lives in the r31 home (it
    /// crosses the call); the loop is bottom-tested (`0 < N` is statically true, so
    /// mwcc drops the pre-test): `li r31,0; loop: mr r3,r31; bl g; addi r31,r31,1;
    /// cmpwi r31,N; blt loop`, then the LR-first epilogue. Measured at 1.3.2/2.6.
    /// A non-zero start, a non-literal bound, a step other than +1, extra
    /// arguments, or any other body defers.
    pub(crate) fn try_counted_call_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        if !function.parameters.is_empty() {
            return Ok(false);
        }
        // The counter: one int local, uninitialized at declaration (`int i;`).
        let [counter] = function.locals.as_slice() else {
            return Ok(false);
        };
        if counter.is_static
            || counter.array_length.is_some()
            || counter.initializer.is_some()
            || counter.data_bytes.is_some()
        {
            return Ok(false);
        }
        if !matches!(counter.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = K` … `i < N` … `i++` (parsed as `i = i + 1`). K must be statically
        // below the bound (the dropped pre-test is only valid when the first
        // iteration is certain; a never-running loop is eliminated differently).
        let start = match initializer {
            Expression::Assign { target, value } if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name) => {
                match value.as_ref() {
                    Expression::IntegerLiteral(start)
                        if *start >= 0 && *start <= i16::MAX as i64 =>
                    {
                        *start
                    }
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        let bound = match condition {
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name) => {
                match right.as_ref() {
                    Expression::IntegerLiteral(bound)
                        if *bound > 0 && *bound <= i16::MAX as i64 =>
                    {
                        *bound
                    }
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        if !matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                    if matches!(left.as_ref(), Expression::Variable(other) if other == &counter.name)
                        && matches!(right.as_ref(), Expression::IntegerLiteral(1))))
        {
            return Ok(false);
        }
        // The body: one direct call whose single argument is the counter.
        let [Statement::Expression(Expression::Call { name, arguments })] = body.as_slice() else {
            return Ok(false);
        };
        let passes_counter = match arguments.as_slice() {
            [] => false,
            [Expression::Variable(variable)] if variable == &counter.name => true,
            _ => return Ok(false),
        };
        if self.locations.contains_key(name.as_str()) || self.globals.contains_key(name.as_str()) {
            return Ok(false);
        }
        // The dropped pre-test requires the first iteration to be statically certain.
        if start >= bound {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 16;
        let home = self.fresh_virtual_general();
        self.callee_saved = vec![home];
        // The counted loop's internal labels advance the anonymous-@N counter by 5
        // (measured: extab @10/@11 vs the unbumped @5/@6).
        self.output.anonymous_label_bump = 5;
        self.output
            .instructions
            .extend(mwcc_vreg::FramePlan::sized_for(vec![home]).prologue());
        self.output.instructions.push(Instruction::AddImmediate {
            d: home,
            a: 0,
            immediate: start as i16,
        });
        let loop_top = self.output.instructions.len();
        if passes_counter {
            self.output.instructions.push(Instruction::Or {
                a: 3,
                s: home,
                b: home,
            });
        }
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: name.to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: home,
            a: home,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: home,
                immediate: bound as i16,
            });
        // `blt loop` — BO 12 (branch-if-true), BI 0 (cr0 LT); backward target.
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: loop_top,
            });
        // The loop's backward branch makes the LR-reload hoist bail, so the epilogue
        // is emitted in final order: LR reload FIRST, then the home (measured — the
        // same order as the do-while counter shape).
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: self.frame_size + 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: home,
            a: 1,
            offset: self.frame_size - 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: self.frame_size,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A leaf `void` function whose body is a single non-counting `while` loop (truthy condition)
    /// whose body is pure in-place increments/decrements of register parameters (`while (*p) p++;`).
    /// mwcc does
    /// NOT unroll these; it emits the rotated form `[b COND;] BODY: <addi>; COND: <test>; <bne BODY>;
    /// blr` with no frame (leaf). The body is emitted directly via `evaluate_general` into each
    /// variable's OWN register, so the loop-carried mutation stays in place rather than being
    /// value-tracked across the back-edge (the linear value tracker has no back-edge). A store in the
    /// body (mwcc hoists loop-invariant store values — not modeled), an empty body (different
    /// structure: the condition is the loop top), a call, a local, a guard, a counted loop (mwcc
    /// unrolls), or a non-void return falls through to defer.
    pub(crate) fn try_emit_increment_while(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
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
        let kind = *kind;
        // A do-while (mwcc fuses the body increment with the next condition load into `lwzu`) or a
        // comparison condition (`while (p < e)` — mwcc computes the trip count and emits a counted
        // CTR loop, `mtctr`/`bdnz`) is deferred; only a `while` with a truthy condition keeps the
        // plain rotated form this models.
        if !matches!(kind, LoopKind::While) || body.is_empty() {
            return Ok(false);
        }
        // A comparison of the loop counter against a loop-invariant bound (`while (p < e)`) lets mwcc
        // compute the trip count and emit a counted CTR loop, so it is deferred. But a comparison of a
        // DATA-DEPENDENT value (`while (*p != 0)`, `while (*p > 0)`) has no computable trip count and
        // stays the rotated form — allow it when one side is a dereference and the other a constant.
        if let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        {
            if crate::analysis::is_comparison(*operator) {
                let dereference_vs_constant =
                    (matches!(left.as_ref(), Expression::Dereference { .. })
                        && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
                        || (matches!(right.as_ref(), Expression::Dereference { .. })
                            && matches!(left.as_ref(), Expression::IntegerLiteral(_)));
                if !dereference_vs_constant {
                    return Ok(false);
                }
            }
        }
        // Every body statement is an in-place update of a register parameter that has no computable
        // trip count, so mwcc keeps the rotated (bottom-test) form: either
        //   (a) a pointer scan `p = p +/- const` — mwcc countifies an INTEGER increment loop but
        //       leaves a pointer scan rotated; or
        //   (b) a pointer CHASE `p = p->field` / `p = *p` — a linked-list walk with no trip count.
        // No stores, calls, or nested control.
        let mut has_chase = false;
        for statement in body {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            if self.lookup_general(name).is_none() {
                return Ok(false);
            }
            let is_pointer = self
                .locations
                .get(name)
                .map_or(false, |location| location.pointee.is_some());
            let is_increment = is_pointer
                && matches!(value, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                    if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                        && matches!(right.as_ref(), Expression::IntegerLiteral(_)));
            // A self-chase reads the next node out of the current one (`p = p->next`, `p = *p`).
            let is_chase = matches!(value, Expression::Member { base, .. }
                    if matches!(base.as_ref(), Expression::Variable(other) if other == name))
                || matches!(value, Expression::Dereference { pointer }
                    if matches!(pointer.as_ref(), Expression::Variable(other) if other == name));
            if !is_increment && !is_chase {
                return Ok(false);
            }
            has_chase |= is_chase;
        }
        // A chase's condition must be the plain chased pointer (`while (p)`), tested once
        // for null. A member/deref condition (`while (p->next)`) reloads the field and
        // schedules differently — deferred rather than mis-emitted.
        if has_chase && !matches!(condition, Expression::Variable(_)) {
            return Ok(false);
        }
        // The loop's labels advance mwcc's anonymous-`@N` counter (4 for a while, 6 for a do-while).
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) {
            6
        } else {
            4
        };
        // A while tests the condition first: branch down to the test; a do-while falls into the body.
        let skip = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        let body_top = self.output.instructions.len();
        for statement in body {
            if let Statement::Assign { name, value } = statement {
                let register = self
                    .lookup_general(name)
                    .expect("gated to a register variable above");
                self.evaluate_general(value, register)?;
            }
        }
        if let Some(index) = skip {
            let condition_at = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                *target = condition_at;
            }
        }
        // emit_condition_test gives the body-SKIP branch (taken when the condition is FALSE); the loop
        // branches BACK to the body when it is TRUE, so invert the BO (branch-if-clear <-> -if-set).
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let back = if options == 4 { 12 } else { 4 };
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: back,
                condition_bit,
                target: body_top,
            });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A `void` function whose body is a counting `for (i = 0; i < bound; i++)`
    /// loop with a parameter bound: mwcc puts the counter in r31 (callee-saved,
    /// initialised to 0) and the bound in r30, branches to the test, and runs
    /// `BODY: <body>; addi r31,r31,1; cmpw r31,r30; blt BODY`. The body may use the
    /// counter (passed as a call argument). Returns whether this path applied.
    pub(crate) fn try_for_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_type != Type::Void
        {
            return Ok(false);
        }
        let [Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(init),
            condition: Some(condition),
            step: Some(step),
            body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = 0`.
        let counter = match init {
            Expression::Assign { target, value }
                if matches!(value.as_ref(), Expression::IntegerLiteral(0)) =>
            {
                match target.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i < bound`.
        let bound = match condition {
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(name) if *name == counter) => {
                match right.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i = i + 1`.
        let step_increments = matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if *name == counter)
            && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if *name == counter)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1))));
        if !step_increments {
            return Ok(false);
        }
        // The counter is the function's only local (uninitialised — the for-clause
        // sets it); the bound is a parameter.
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.name != counter || local.initializer.is_some() {
            return Ok(false);
        }
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == bound)
        {
            return Ok(false);
        }
        // The body must be straight-line calls referencing no register value other
        // than the counter (the bound, and any other parameter, would each need
        // their own callee-saved register — deferred).
        if body
            .iter()
            .any(|statement| !matches!(statement, Statement::Expression(_)))
        {
            return Ok(false);
        }
        let reads_other_parameter = body.iter().any(|statement| match statement {
            Statement::Expression(expression) => function.parameters.iter().any(|parameter| {
                parameter.name != counter && expression_reads_name(expression, &parameter.name)
            }),
            _ => false,
        });
        if reads_other_parameter {
            return Ok(false);
        }
        if !function_makes_call(function) {
            return Ok(false);
        }
        let bound_incoming = match self.locations.get(&bound) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };

        // Prologue: r31 = counter (init 0), r30 = bound, saved at the top of a frame.
        const COUNTER: u8 = 31;
        const BOUND: u8 = 30;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![COUNTER, BOUND];
        self.output.anonymous_label_bump = 5;
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: COUNTER,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: COUNTER,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: BOUND,
            a: 1,
            offset: 8,
        });
        self.emit_integer_materialization_copy(BOUND, bound_incoming);
        let signed = self.signed_of(local.declared_type);
        self.locations.insert(
            counter.clone(),
            Location {
                class: ValueClass::General,
                register: COUNTER,
                signed,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        if let Some(location) = self.locations.get_mut(&bound) {
            location.register = BOUND;
        }

        // Branch to the test; the body falls into the step, then the compare loops.
        let skip = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: COUNTER,
            a: COUNTER,
            immediate: 1,
        });
        let condition_index = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_index;
        }
        self.output.instructions.push(Instruction::CompareWord {
            a: COUNTER,
            b: BOUND,
        });
        // `blt body_top` (BO=12 branch-if-true, BI=0 the LT bit).
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: body_top,
            });

        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: COUNTER,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: BOUND,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
