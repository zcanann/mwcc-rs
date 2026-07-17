//! Callee-saved register families: calls through pointers, park/combine shapes.
//!
//! Split by family (fire 547); behavior-identical.

mod conditional;
mod combine;

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// The FLOAT callee-saved survivor (fire 406, C1): `return g(x) OP x;`
    /// with a double parameter surviving one external call. Measured:
    /// stwu -16; mflr; stw r0,20; stfd f31,8; fmr f31,f1; bl; lwz r0,20
    /// (the LR reload FIRST); the op; lfd f31,8; mtlr; addi; blr. The
    /// fmr copy leaves f1 holding x for the call itself.
    pub(crate) fn try_float_callee_saved(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.return_type != Type::Double
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
        let Some(Expression::Binary { operator, left, right }) = &function.return_expression else {
            return Ok(false);
        };
        // Call OP x, or x OP call. The emitted op reads (f31 = x) and
        // (f1 = the result): mwcc commutes adds to put the saved value
        // first (measured fadd f1,f31,f1 for `g(x) + x`).
        let (call, call_first) = match (left.as_ref(), right.as_ref()) {
            (Expression::Call { name, arguments }, Expression::Variable(v)) if v == x => {
                ((name, arguments), true)
            }
            (Expression::Variable(v), Expression::Call { name, arguments }) if v == x => {
                ((name, arguments), false)
            }
            _ => return Ok(false),
        };
        let (callee, arguments) = call;
        // A single argument: x itself (the fmr leaves f1 intact).
        if !matches!(arguments.as_slice(), [Expression::Variable(v)] if v == x) {
            return Ok(false);
        }
        // The op: fadd (commuted saved-first), fsub per order, fmul
        // (commuted saved-first).
        enum Op {
            Add,
            Mul,
            SubCallMinusX,
            SubXMinusCall,
        }
        let op = match operator {
            BinaryOperator::Add => Op::Add,
            BinaryOperator::Multiply => Op::Mul,
            BinaryOperator::Subtract if call_first => Op::SubCallMinusX,
            BinaryOperator::Subtract => Op::SubXMinusCall,
            _ => return Ok(false),
        };
        // -- emit --
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved_float = 1;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink { target: callee.to_string() });
        // The MULTIPLY schedules ahead of the LR reload (its latency
        // starts early — measured); add/sub follow the reload.
        if matches!(op, Op::Mul) {
            self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 31, c: 1 });
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        } else {
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            match op {
                Op::Add => self
                    .output
                    .instructions
                    .push(Instruction::FloatAddDouble { d: 1, a: 31, b: 1 }),
                Op::SubCallMinusX => self
                    .output
                    .instructions
                    .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 31 }),
                Op::SubXMinusCall => self
                    .output
                    .instructions
                    .push(Instruction::FloatSubtractDouble { d: 1, a: 31, b: 1 }),
                Op::Mul => unreachable!(),
            }
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    pub(crate) fn try_callee_saved(&mut self, function: &Function) -> Compilation<bool> {
        // Address-taken locals are handled by the frame-resident path before this.
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        // This path emits only statements + the return; it does NOT emit local initializers. A local
        // whose initializer has a side effect (`int x = g();`, x otherwise dead) would silently drop
        // that call — a miscompile. Locals are handled by the dedicated local paths (computed_local,
        // result_param_combine) before this; anything reaching here with a local defers.
        if !function.locals.is_empty() {
            return Ok(false);
        }
        // The body is straight-line calls and stores (control flow routes through its own
        // paths). A trailing store sink (`foo(); gi = a;`) saves the live value, runs the
        // calls, then stores it from the callee-saved register; mwcc orders that epilogue's
        // LR reload before the GPR reload (epilogue_lr_first), unlike the return sink.
        if function.statements.iter().any(|statement| !matches!(statement, Statement::Expression(_) | Statement::Store { .. })) {
            return Ok(false);
        }
        let has_store = function.statements.iter().any(|statement| matches!(statement, Statement::Store { .. }));
        if matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        let Some(live) = values_live_across_call(function) else {
            return Ok(false);
        };
        if live.is_empty() {
            return Ok(false);
        }
        // Every live value must be a general-class parameter (locals defer), and none
        // may be passed to a call — the first such argument use stays in the incoming
        // register (mwcc skips the move until a call clobbers it), which needs
        // value-location tracking not modeled here.
        let passed_to_call = function.statements.iter().any(|statement| match statement {
            Statement::Expression(expression) => live.iter().any(|name| expression_reads_name(expression, name)),
            _ => false,
        });
        if passed_to_call {
            return Ok(false);
        }
        // (parameter index, name, incoming register) for each promoted value.
        let mut promoted: Vec<(usize, String, u8)> = Vec::new();
        for name in &live {
            let Some(index) = function.parameters.iter().position(|parameter| &parameter.name == name) else {
                return Ok(false);
            };
            let (class, incoming) = match self.locations.get(name) {
                Some(location) => (location.class, location.register),
                None => return Ok(false),
            };
            if class != ValueClass::General {
                return Ok(false);
            }
            promoted.push((index, name.clone(), incoming));
        }
        // Highest register (r31) to the last parameter, descending toward the first.
        promoted.sort_by_key(|(index, _, _)| *index);

        let count = promoted.len();
        // A store sink takes one or two saved values: the epilogue reloads all-but-the-
        // lowest GPR, then LR, then the lowest, matching mwcc for count 1 and 2 (three or
        // more reschedule the LR reload by register death — `lwz r31; lwz r30; lwz r29; lwz
        // r0` — and defer). A second saved value must be void; a value returned alongside
        // two saved values interleaves the return move with the epilogue and is not modeled.
        if has_store && (count > 2 || (count == 2 && function.return_type != Type::Void)) {
            return Ok(false);
        }
        // A two-value store sink stores the saved values directly (`gi = a; gj = b;`). A
        // computed store (`gi = a + 1;`) reschedules around the two saves/epilogue and is
        // deferred; the single-value sink still allows a computed store.
        if has_store
            && count == 2
            && !function.statements.iter().all(|statement| match statement {
                Statement::Store { value, .. } => matches!(value, Expression::Variable(_)),
                _ => true,
            })
        {
            return Ok(false);
        }
        // A single store that consumes BOTH saved values (`g(); *p=x;` — p is the store base, x the
        // stored value, both live across the call) restores LR BEFORE both GPRs (`stw r31,0(r30);
        // lwz r0(LR); lwz r31; lwz r30`), unlike two independent stores of one value each (`gi=a;
        // gj=b;`) which this path models (`… lwz r31; lwz r0(LR); lwz r30`). Detect it as "fewer store
        // statements than saved values" and defer rather than emit the wrong restore order.
        if has_store
            && count == 2
            && function.statements.iter().filter(|statement| matches!(statement, Statement::Store { .. })).count() < count
        {
            return Ok(false);
        }
        // A store sink whose RETURN is an unrelated CONSTANT (`g(); *p=C; return K;`) uses mwcc's
        // return-value-BEFORE-store schedule with a GPR-first restore (`li r3,K; stw ...; lwz r31;
        // lwz r0`), NOT this store-sink path's LR-first `stw; li r3; lwz r0; lwz r31` — so it emits
        // the wrong order. Defer it (byte-exact-or-defer) until that schedule is modeled. A return of
        // the SAVED value (`foo(); gi=a; return a;`) is unaffected (its return is a variable, not a
        // constant) and stays on the correct LR-first path.
        if has_store
            && matches!(function.statements.last(), Some(Statement::Store { .. }))
            && function.return_type != Type::Void
            && function.return_expression.as_ref().is_some_and(|expression| constant_value(expression).is_some())
        {
            return Ok(false);
        }
        // With more than one saved value RETURNED, mwcc's scheduler interleaves the
        // epilogue restores with the post-call computation by register death — which we
        // don't model yet. It coincides with "all restores after" only when the values
        // combine in a single low-latency instruction (`a+b`, `a-b`, `a&b`); two
        // values through a multiply, or three or more values (multi-step), reschedule
        // the restores. Restrict count > 1 to that one safe shape. (A two-value store sink
        // has its own epilogue order above, so it skips this return-shape gate.)
        if count >= 2 && !has_store {
            let single_op = matches!(
                function.return_expression.as_ref(),
                Some(Expression::Binary { operator, left, right })
                    if count == 2
                        && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                            | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                        && matches!(left.as_ref(), Expression::Variable(_))
                        && matches!(right.as_ref(), Expression::Variable(_))
            );
            if !single_op {
                return Ok(false);
            }
        }
        // A saved value nested >= 2 deep in an arithmetic RETURN (`return (-x)&C` = neg then clrlwi)
        // DIES mid-computation, so mwcc interleaves its callee-saved restore at that death point
        // (restore-by-register-death: `neg r0,r31; lwz r31; clrlwi r3,r0; …`). This all-restores-at-end
        // epilogue does not model that, so it would miscompile — defer. Single-op returns (`x`, `-x`,
        // `x&C`, `x+f()`; depth <= 1) are unaffected. (Store sinks are excluded; count>=2 already
        // restricted to single-op above.)
        if !has_store
            && promoted.iter().any(|(_, name, _)| {
                function.return_expression.as_ref().and_then(|expression| name_nesting_depth(expression, name)).is_some_and(|depth| depth >= 2)
            })
        {
            return Ok(false);
        }
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: the promoted parameters' homes are virtuals, created highest-rank
        // first — id order reproduces r31, r30, … through the callee-saved pool. The
        // interleaved save+move prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        // Only a STORE SINK — a body whose TRAILING statement is the store of the saved value (after
        // all calls) — reloads the saved LR before the GPR reloads, even when a value is also returned
        // afterward (`foo(); gi=a; return a;`). An EARLIER store whose sink is the return (`*p=a; g();
        // return a;`) takes the ordinary return epilogue (GPRs, then LR), where the hoist pass places
        // the LR reload right after the last call. So key on the LAST statement, not merely has_store.
        self.epilogue_lr_first = matches!(function.statements.last(), Some(Statement::Store { .. }));
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = promoted.iter().rev().map(|(_, _, incoming)| *incoming).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        for (rank, (_, name, _)) in promoted.iter().rev().enumerate() {
            if let Some(location) = self.locations.get_mut(name) {
                location.register = homes[rank];
            }
        }

        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        if function.return_type != Type::Void {
            let result = Eabi::general_result().number;
            // A non-void function may FALL OFF THE END (C89; strikers alloc's
            // FORCE_DONT_INLINE stubs) — mwcc emits a bare blr, r3 undefined.
            if let Some(return_expression) = function.return_expression.as_ref() {
                self.evaluate_tail(return_expression, function.return_type, result)?;
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `void s(T *p, …) { *p = g(args); }` — a call's result stored through a pointer
    /// PARAMETER that must survive the call. mwcc saves the pointer in r31 (`mr r31,r3`),
    /// runs the call, then stores the result through r31 (`stw r3,0(r31)`); the store-sink
    /// epilogue reloads LR before r31. Restricted to a general (int/pointer/narrow) pointee,
    /// a general-returning call, and arguments that do not reference the saved pointer.
    pub(crate) fn try_store_call_through_pointer(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        // Void, or a non-void function returning a CONSTANT — materialized in r3 after the
        // store, before the epilogue (`*p=g(); return 0;` -> `stw r3,0(r31); li r3,0; …`). A
        // non-constant return (`return *p` re-reads the saved pointer with an interleaved
        // epilogue; `return x` reads a call-clobbered parameter) defers.
        let returns_constant = function.return_type != Type::Void
            && matches!(function.return_type, Type::Int | Type::UnsignedInt)
            && function.return_expression.as_ref().map_or(false, |expression| constant_value(expression).is_some());
        if function.return_type != Type::Void && !returns_constant {
            return Ok(false);
        }
        let [Statement::Store { target, value }] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Expression::Call { name, arguments } = value else { return Ok(false) };
        // A store target through a pointer PARAMETER: `*p`, `p[const]`, or `p->m` — each
        // resolves to (the pointer variable, a byte offset, the stored width's pointee).
        let (pointer_name, byte_offset, pointee): (&str, i64, Pointee) = match target {
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else { return Ok(false) };
                (name, 0, self.pointee_of(pointer)?)
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                let Some(constant) = constant_value(index) else { return Ok(false) };
                let pointee = self.pointee_of(base)?;
                (name, constant * pointee.size() as i64, pointee)
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                let Some(pointee) = pointee_of_type(*member_type) else { return Ok(false) };
                (name, *offset as i64, pointee)
            }
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| parameter.name == pointer_name) {
            return Ok(false);
        }
        let (class, incoming) = match self.locations.get(pointer_name) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }
        // The call's result must match the store width: a general (int) pointee needs an
        // int-returning call (result in r3, stw/stb/sth); a float/double pointee needs a
        // matching float-returning call (result in f1, stfs/stfd). A mismatch (int call to a
        // float target, or vice versa) would need a conversion — defer.
        let float_store = matches!(pointee, Pointee::Float | Pointee::Double);
        let matched = match pointee {
            Pointee::Float => self.call_return_types.get(name) == Some(&Type::Float),
            Pointee::Double => self.call_return_types.get(name) == Some(&Type::Double),
            _ => !matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)),
        };
        if !matched {
            return Ok(false);
        }
        // The call must NOT pass the saved pointer as an argument (that keeps it in an
        // argument register across the call — a different shape).
        if arguments.iter().any(|argument| expression_reads_name(argument, pointer_name)) {
            return Ok(false);
        }
        let offset = i16::try_from(byte_offset)
            .map_err(|_| Diagnostic::error("store-through-saved-pointer offset out of range (roadmap)"))?;

        // Callee-saved frame: r31 holds the pointer across the call; the store-sink epilogue
        // reloads LR before r31.
        let frame_size: i16 = 16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: the saved pointer's home is a virtual — call-crossing -> r31; the
        // epilogue reload (emit_epilogue_and_return reads callee_saved) renames too.
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        self.epilogue_lr_first = true;
        // The interleaved save+move prologue, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue_interleaved(&[incoming]));
        if let Some(location) = self.locations.get_mut(pointer_name) {
            location.register = saved;
        }
        // A float-returning call leaves its result in f1 (stfs/stfd); an int call in r3.
        let result = if float_store {
            self.emit_call(name, arguments, None, true)?;
            mwcc_target::Eabi::float_result().number
        } else {
            self.emit_call(name, arguments, None, false)?;
            mwcc_target::Eabi::general_result().number
        };
        self.output.instructions.push(displacement_store(pointee, result, saved, offset)?);
        // A non-void function materializes its constant return value in r3 after the store.
        if let Some(return_expression) = function.return_expression.as_ref() {
            self.evaluate_tail(return_expression, function.return_type, mwcc_target::Eabi::general_result().number)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A MEMORY-loaded local carried across calls in r31: `int t = gi; g(); return t;`
    /// loads the global into r31 in the prologue, runs the calls, and returns it —
    /// `stwu; mflr; stw r0; stw r31; lwz r31,gi; bl; lwz r0; mr r3,r31; lwz r31; mtlr;
    /// addi; blr` (the `mr` rides between the LR and r31 reloads). A computed-index
    /// global-array element (`int t = arr[i]; g(); return t;` — the signal.c handler
    /// fetch) interleaves the address build into the prologue: `stwu; mflr; lis r4;
    /// stw r0; slwi r0,i; addi r3,r4; stw r31; lwzx r31,r3,r0; bl; …`. Call arguments
    /// must be constants (a register argument after a call reads clobbered state).
    /// A guarded call through a GLOBAL function pointer held in a local (the signal.c
    /// dispatch tail): `F t = gf; if (!t) return; t();` loads the pointer into r12,
    /// tests it, branches to the shared epilogue when the guard fires, and calls
    /// through — `stwu; mflr; stw r0; lwz r12,gf; cmplwi r12,0; beq EPILOGUE; mtctr;
    /// bctrl; EPILOGUE: lwz r0; mtlr; addi; blr`. Zero-argument, void, single call.
    pub(crate) fn try_guarded_global_pointer_call(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || function.locals.len() != 1
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let local = &function.locals[0];
        if local.is_static {
            return Ok(false);
        }
        let Some(Expression::Variable(global)) = &local.initializer else {
            return Ok(false);
        };
        if !self.globals.contains_key(global.as_str()) || self.global_array_sizes.contains_key(global.as_str()) {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }, Statement::Expression(Expression::Call { name, arguments })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(then_body.as_slice(), [Statement::Return(None)]) || !else_body.is_empty() {
            return Ok(false);
        }
        if name != &local.name {
            return Ok(false);
        }
        // Arguments must ALREADY sit in their argument registers (`t(s)` with `s` the
        // first parameter): nothing to materialize, so the sequence is identical to the
        // zero-argument form. Anything needing placement defers.
        for (position, argument) in arguments.iter().enumerate() {
            let Expression::Variable(argument_name) = argument else {
                return Ok(false);
            };
            let expected = mwcc_target::Eabi::FIRST_GENERAL_ARGUMENT + position as u8;
            match self.locations.get(argument_name) {
                Some(location) if location.class == ValueClass::General && location.register == expected => {}
                _ => return Ok(false),
            }
        }

        // The canonical saveless non-leaf frame, derived by the FRAME BUILDER — the
        // first consumer of the plan-based prologue (its epilogue is the standard
        // emit_epilogue_and_return form, identical to plan.epilogue()).
        let plan = mwcc_vreg::FramePlan::sized_for(Vec::new());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.output.instructions.extend(plan.prologue());
        self.emit_global_load_value(global, 12)?;
        // The pointer local is UNSIGNED (cmplwi) and lives in r12 for the test.
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: 12, signed: false, width: 32, pointee: None, stride: None },
        );
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        // The guard branches ON TRUE straight to the shared epilogue (the bare-void fold).
        // The branch label and the staged pointer load advance the anonymous-`@N` counter.
        self.output.anonymous_label_bump = 3;
        let epilogue_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options: options ^ 8, condition_bit, target: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        let epilogue_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[epilogue_branch] {
            *target = epilogue_label;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn try_callee_saved_memory_local(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty()
            || function.locals.len() != 1
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
            || function.statements.is_empty()
        {
            return Ok(false);
        }
        let local = &function.locals[0];
        if local.is_static || (local.declared_type.width() as u32) < 32 {
            return Ok(false);
        }
        let Some(initializer) = &local.initializer else {
            return Ok(false);
        };
        // The return is the local itself, or a two-leaf expression over the local and
        // ONE parameter — the parameter then survives the calls in r30 alongside the
        // local's r31 (`int t = gi; g(); return t + s;` → `stw r30,8; mr r30,r3;
        // lwz r31; bl; add r3,r31,r30`).
        let paired_parameter: Option<&str> = match function.return_expression.as_ref() {
            Some(Expression::Variable(returned)) if returned == &local.name => None,
            Some(Expression::Binary { left, right, .. }) => {
                let (Expression::Variable(first), Expression::Variable(second)) = (left.as_ref(), right.as_ref()) else {
                    return Ok(false);
                };
                let other = if first == &local.name {
                    second
                } else if second == &local.name {
                    first
                } else {
                    return Ok(false);
                };
                if !function.parameters.iter().any(|parameter| &parameter.name == other) {
                    return Ok(false);
                }
                Some(other.as_str())
            }
            _ => return Ok(false),
        };
        // An optional LEADING guard CHAIN reading the loaded local: `int t = gi; if (!t)
        // return -1; if (t == 1) return 0; g(); return t;` — the raise() shape. Every
        // guard compares the STAGED r0 copy (still valid — no call intervenes), each
        // constant early return branches to the shared epilogue, and only the first
        // compare carries the `mr r31,r0` in its latency slot.
        let mut guard_chain: Vec<(&Expression, i16)> = Vec::new();
        let mut rest = function.statements.as_slice();
        while let Some((Statement::If { condition, then_body, else_body }, tail)) = rest.split_first() {
            if !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Return(Some(_))]) {
                break;
            }
            let [Statement::Return(Some(value))] = then_body.as_slice() else {
                break;
            };
            let Some(constant) = constant_value(value).and_then(|constant| i16::try_from(constant).ok()) else {
                return Ok(false);
            };
            guard_chain.push((condition, constant));
            rest = tail;
        }
        let guard = (!guard_chain.is_empty()).then_some(());
        // An optional CONDITIONAL STORE back into the loaded element (`int t = garr[i];
        // if (t != 1) garr[i] = 0; g(); return t;` — the raise() handler-reset). The
        // scaled index survives in its own register for the store's reuse of the
        // address. Verified without return-guards; the mixed chain defers.
        let (conditional_store, calls) = match rest.split_first() {
            Some((Statement::If { condition, then_body, else_body }, tail))
                if guard_chain.is_empty()
                    && else_body.is_empty()
                    && matches!(then_body.as_slice(), [Statement::Store { .. }]) =>
            {
                let [Statement::Store { target, value }] = then_body.as_slice() else {
                    return Ok(false);
                };
                let Some(constant) = constant_value(value).and_then(|constant| i16::try_from(constant).ok()) else {
                    return Ok(false);
                };
                (Some((condition, target, constant)), tail)
            }
            _ => (None, rest),
        };
        if calls.is_empty() {
            return Ok(false);
        }
        for statement in calls {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            // An ARGUMENT call reshapes the whole sequence: mwcc keeps the array base
            // out of r3 and hoists the argument materialization into the address-build
            // latency (`addi r4,r4; li r3,0; stw r31; lwzx r31,r4,r0`) — a scheduling
            // composition not yet modeled. Zero-argument calls only.
            if !arguments.is_empty() {
                return Ok(false);
            }
        }
        // The two captured load forms: a scalar global, or a plain-index element of a
        // word-sized global array.
        enum MemoryLoad<'e> {
            Scalar,
            Array { name: &'e str, index: &'e Expression },
        }
        let load = match initializer {
            Expression::Variable(name)
                if self.globals.contains_key(name.as_str()) && !self.global_array_sizes.contains_key(name.as_str()) =>
            {
                if pointee_of_type(self.globals[name.as_str()]) != Some(Pointee::Int)
                    && pointee_of_type(self.globals[name.as_str()]) != Some(Pointee::UnsignedInt)
                {
                    return Ok(false);
                }
                MemoryLoad::Scalar
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                if !self.global_array_sizes.contains_key(name.as_str()) || constant_value(index).is_some() {
                    return Ok(false);
                }
                if !matches!(index.as_ref(), Expression::Variable(_)) {
                    return Ok(false);
                }
                if !matches!(pointee_of_type(self.globals[name.as_str()]), Some(Pointee::Int | Pointee::UnsignedInt)) {
                    return Ok(false);
                }
                MemoryLoad::Array { name, index }
            }
            _ => return Ok(false),
        };

        // The PAIRED form is verified for the guard-free scalar load only.
        if paired_parameter.is_some() && (guard.is_some() || matches!(load, MemoryLoad::Array { .. })) {
            return Ok(false);
        }
        // A multi-guard chain over the ARRAY form is unverified — defer.
        if guard_chain.len() > 1 && matches!(load, MemoryLoad::Array { .. }) {
            return Ok(false);
        }
        // The conditional store: verified for the ARRAY load storing back into the SAME
        // element (same array, same index variable), with no paired parameter. The
        // scaled index survives in its own register and the base/index pair is reused:
        // `lis r4; slwi r5,i,2; stw r0; addi r3,r4; stw r31; lwzx r31,r3,r5;
        // cmpwi r31,K; beq SKIP; li r0,C; stwx r0,r3,r5; SKIP: bl; …`.
        if let Some((store_condition, store_target, store_constant)) = conditional_store {
            if paired_parameter.is_some() {
                return Ok(false);
            }
            let MemoryLoad::Array { name: load_name, index: load_index } = load else {
                return Ok(false);
            };
            let Expression::Index { base, index } = store_target else {
                return Ok(false);
            };
            let Expression::Variable(store_name) = base.as_ref() else {
                return Ok(false);
            };
            if store_name != load_name {
                return Ok(false);
            }
            let (Expression::Variable(load_index_name), Expression::Variable(store_index_name)) =
                (load_index, index.as_ref())
            else {
                return Ok(false);
            };
            if load_index_name != store_index_name {
                return Ok(false);
            }

            self.non_leaf = true;
            self.frame_size = 16;
            self.callee_saved = vec![31];
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            let signed = !matches!(local.declared_type, Type::UnsignedInt);
            // The scaled index lands past the (reserved) base-high register so both
            // survive for the store: `lis r4; slwi r5,i,2; stw r0,20; addi r3,r4;
            // stw r31,12; lwzx r31,r3,r5`.
            let index_register = self.general_register_of_leaf(load_index)?;
            let high = self.fresh_virtual_general();
            let scaled = self.fresh_virtual_general();
            self.emit_address_high(high, load_name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: scaled, s: index_register, shift: 2 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
            self.record_relocation(RelocationKind::Addr16Lo, load_name);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
            let saved = self.fresh_virtual_general();
            self.callee_saved = vec![saved];
            self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::LoadWordIndexed { d: saved, a: index_register, b: scaled });
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
            );
            // The conditional store skips on the condition's FALSE side, the value
            // materializes into r0, and the base/scaled pair is reused. (@N: measured
            // against the real extab numbering.)
            self.output.anonymous_label_bump = 3;
            let (options, condition_bit) = self.emit_condition_test(store_condition)?;
            let skip_branch = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: 0, immediate: store_constant });
            self.output.instructions.push(Instruction::StoreWordIndexed { s: GENERAL_SCRATCH, a: index_register, b: scaled });
            let skip_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_branch] {
                *target = skip_label;
            }
            for statement in calls {
                self.emit_statement(statement)?;
            }
            let result = mwcc_target::Eabi::general_result().number;
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::Or { a: result, s: saved, b: saved });
            self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = if paired_parameter.is_some() { vec![31, 30] } else { vec![31] };
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        // Phase D: the callee-saved home is a VIRTUAL in every form — its range
        // crosses the calls, so the allocator assigns it from the callee-saved pool
        // (r31), and apply() rewrites the saves, loads, moves, and restores together.
        // (The paired form allocates its second virtual below; creation order makes
        // the ids deterministic: the local first -> r31, the parameter -> r30.)
        let saved: u8 = self.fresh_virtual_general();
        // The paired parameter saves in r30 between the r31 save and the memory load:
        // `stw r31,12; stw r30,8; mr r30,<param>; lwz r31,<gi>`.
        if let Some(parameter) = paired_parameter {
            let Some(incoming) = self.lookup_general(parameter) else {
                return Ok(false);
            };
            // The parameter's callee-saved home is the SECOND virtual: created after
            // `saved`, both widen to entry, so the scan assigns saved->r31, pair->r30.
            let pair = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::StoreWord { s: pair, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::Or { a: pair, s: incoming, b: incoming });
            if let Some(location) = self.locations.get_mut(parameter) {
                location.register = pair;
            }
            self.evaluate_general(initializer, saved)?;
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
            );
            for statement in calls {
                self.emit_statement(statement)?;
            }
            // The epilogue computes the return expression in the slot after the LR
            // reload: `lwz r0,20; add r3,r31,r30; lwz r31,12; lwz r30,8; mtlr; addi; blr`.
            let result = mwcc_target::Eabi::general_result().number;
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.evaluate_tail(function.return_expression.as_ref().expect("checked above"), function.return_type, result)?;
            self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::LoadWord { d: pair, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        match load {
            MemoryLoad::Scalar => {
                self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
                if guard.is_some() {
                    // With a guard the load STAGES through r0: the compare reads r0 and
                    // the `mr r31,r0` fills its latency slot — `lwz r0,gi; cmpwi r0,0;
                    // mr r31,r0; bne CONT` (the guard emission below issues the branch).
                    self.evaluate_general(initializer, GENERAL_SCRATCH)?;
                    self.locations.insert(
                        local.name.clone(),
                        Location { class: ValueClass::General, register: GENERAL_SCRATCH, signed, width: 32, pointee: None, stride: None },
                    );
                } else {
                    self.evaluate_general(initializer, saved)?;
                }
            }
            MemoryLoad::Array { name, index } => {
                let index_register = self.general_register_of_leaf(index)?;
                let high = self.fresh_virtual_general();
                self.emit_address_high(high, name);
                self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: 2 });
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
                self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
                self.output.instructions.push(Instruction::LoadWordIndexed { d: saved, a: index_register, b: GENERAL_SCRATCH });
            }
        }
        let result = mwcc_target::Eabi::general_result().number;
        if guard.is_some() {
            // Each guard tests the just-loaded value (the staged r0 copy for a scalar —
            // valid across the whole chain, no call intervenes — or r31 for the array
            // form), then `li r3,K; b EPILOGUE`; the next guard or the calls are the
            // fall-through. Only the FIRST compare carries the `mr r31,r0` in its
            // latency slot. A multi-guard array chain is unverified — deferred above.
            // The labels advance mwcc's anonymous-`@N` counter: one per guard's
            // fall-through label plus the shared epilogue; the staged scalar load adds
            // one more (measured against the real extab/extabindex `@N` numbering).
            self.output.anonymous_label_bump =
                2 * guard_chain.len() as u32 + if matches!(load, MemoryLoad::Scalar) { 1 } else { 0 };
            if matches!(load, MemoryLoad::Array { .. }) {
                self.locations.insert(
                    local.name.clone(),
                    Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
                );
            }
            let mut epilogue_branches = Vec::new();
            for (position, (condition, early_constant)) in guard_chain.iter().enumerate() {
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                if position == 0 && matches!(load, MemoryLoad::Scalar) {
                    self.output.instructions.push(Instruction::Or { a: saved, s: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                }
                let skip_branch = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: *early_constant });
                epilogue_branches.push(self.output.instructions.len());
                self.output.instructions.push(Instruction::Branch { target: 0 });
                let fall_through = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_branch] {
                    *target = fall_through;
                }
            }
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
            );
            for statement in calls {
                self.emit_statement(statement)?;
            }
            self.output.instructions.push(Instruction::Or { a: result, s: saved, b: saved });
            let epilogue_label = self.output.instructions.len();
            for branch in epilogue_branches {
                if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                    *target = epilogue_label;
                }
            }
            // With the result already placed on both paths, the epilogue is plain:
            // `lwz r0,20; lwz r31,12; mtlr; addi; blr`.
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        for statement in calls {
            self.emit_statement(statement)?;
        }
        // The epilogue interleaves the result move between the LR and callee-saved
        // reloads. `saved` is the virtual for the guard-free scalar (apply() rewrites
        // the restore's field with the value's home) and the literal r31 otherwise.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::Or { a: result, s: saved, b: saved });
        self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// Decode `*p = call()` / `p[const] = call()` / `p->m = call()` where the call is an
    /// integer-returning, zero-argument call and `p` is a General-class pointer variable —
    /// yielding (pointer name, byte offset, stored pointee, call name). Shared by the
    /// two-output-pointer store sink.
    pub(crate) fn decode_pointer_call_store(&self, statement: &Statement) -> Option<(String, i16, Pointee, String)> {
        let Statement::Store { target, value } = statement else { return None };
        let Expression::Call { name, arguments } = value else { return None };
        if !arguments.is_empty() {
            return None;
        }
        if matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)) {
            return None;
        }
        let (pointer_name, byte_offset, pointee): (&str, i64, Pointee) = match target {
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else { return None };
                (name, 0, self.pointee_of(pointer).ok()?)
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else { return None };
                let constant = constant_value(index)?;
                let pointee = self.pointee_of(base).ok()?;
                (name, constant * pointee.size() as i64, pointee)
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let Expression::Variable(name) = base.as_ref() else { return None };
                let pointee = pointee_of_type(*member_type)?;
                (name, *offset as i64, pointee)
            }
            _ => return None,
        };
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            return None;
        }
        let offset = i16::try_from(byte_offset).ok()?;
        Some((pointer_name.to_string(), offset, pointee, name.clone()))
    }

    /// Two to four output pointers, each receiving a call result: `void s(int*a,int*b){ *a=g();
    /// *b=h(); }`. Every pointer must survive its call, so mwcc parks them in callee-saved
    /// registers — the pointer arriving in the HIGHEST incoming register in r31, the next in r30,
    /// and so on descending (positional, independent of store order) — runs each call, stores its
    /// result, then reloads LR before all the saved GPRs. The single-pointer case is
    /// `try_store_call_through_pointer`.
    pub(crate) fn try_stores_through_pointers(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void || function.return_expression.is_some() {
            return Ok(false);
        }
        let count = function.statements.len();
        if !(2..=4).contains(&count) {
            return Ok(false);
        }
        // Every statement is `*p = call()` through a distinct General-class pointer parameter.
        let mut decoded = Vec::with_capacity(count);
        for statement in &function.statements {
            match self.decode_pointer_call_store(statement) {
                Some(store) => decoded.push(store),
                None => return Ok(false),
            }
        }
        let mut incoming = Vec::with_capacity(count);
        for (pointer_name, _, _, _) in &decoded {
            if !function.parameters.iter().any(|parameter| &parameter.name == pointer_name) {
                return Ok(false);
            }
            match self.locations.get(pointer_name) {
                Some(location) if location.class == ValueClass::General => incoming.push(location.register),
                _ => return Ok(false),
            }
        }
        let mut distinct = incoming.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != count {
            return Ok(false);
        }

        // Assign r31, r30, … to the pointers by DESCENDING incoming register (highest -> r31).
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by(|&i, &j| incoming[j].cmp(&incoming[i]));
        // Phase D: each saved pointer's home is a virtual, created in DESCENDING
        // incoming order — all widen to entry via their saves, so the scan assigns
        // by id: first virtual -> r31, next -> r30, … exactly the positional rule.
        let mut saved_reg = vec![0u8; count];
        let mut callee_saved = Vec::with_capacity(count);
        for &index in order.iter() {
            let register = self.fresh_virtual_general();
            saved_reg[index] = register;
            callee_saved.push(register);
        }

        // The interleaved save+move prologue, from the FRAME BUILDER (each pointer
        // parks in its callee-saved home right after that home's save).
        let plan = mwcc_vreg::FramePlan::sized_for(callee_saved.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = callee_saved;
        self.epilogue_lr_before_gprs = true;
        let incoming_ordered: Vec<u8> = order.iter().map(|&index| incoming[index]).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        for (index, (pointer_name, _, _, _)) in decoded.iter().enumerate() {
            if let Some(location) = self.locations.get_mut(pointer_name) {
                location.register = saved_reg[index];
            }
        }

        // Each call in source order, its result stored through the saved pointer.
        let result = mwcc_target::Eabi::general_result().number;
        for (index, (_, offset, pointee, call)) in decoded.iter().enumerate() {
            self.emit_call(call, &[], None, false)?;
            self.output.instructions.push(displacement_store(*pointee, result, saved_reg[index], *offset)?);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `int x = G; [int y = G';] call(); G2 = x; [G3 = y;]` — one or two register
    /// locals initialized from word globals, live across ONE call, stored back to
    /// word globals in declaration order. The fully general-allocator crossing
    /// shape: each home is a VIRTUAL, the call-crossing makes LinearScan draw from
    /// the callee-saved pool (r31, then r30), and the frame builder supplies the
    /// canonical saves. Measured @2.6/1.3.2: the globals load DIRECTLY into the
    /// callee-saved homes (declaration order), and the epilogue is the
    /// `epilogue_lr_first` register-death schedule (one save: `lwz r0; lwz r31`;
    /// two saves: `lwz r31; lwz r0; lwz r30`).
    pub(crate) fn try_callee_saved_global_round_trip(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !function.parameters.is_empty()
            || !self.frame_slots.is_empty()
            || function.locals.is_empty()
            || function.locals.len() > 2
        {
            return Ok(false);
        }
        // Each local's source: a word-global load, or a no-arg call whose result
        // parks into the home (`bl; mr r31,r3` — measured for the single-local
        // shape; a call producer in the pair is unmeasured and defers below).
        enum Source {
            Global(String),
            Call(String),
        }
        let mut sources = Vec::with_capacity(function.locals.len());
        for local in &function.locals {
            if local.array_length.is_some()
                || local.is_static
                || local.row_bytes.is_some()
                || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            {
                return Ok(false);
            }
            match &local.initializer {
                Some(Expression::Variable(source_global))
                    if !self.locations.contains_key(source_global)
                        && matches!(self.globals.get(source_global.as_str()), Some(Type::Int | Type::UnsignedInt)) =>
                {
                    sources.push(Source::Global(source_global.clone()));
                }
                Some(Expression::Call { name: producer, arguments })
                    if arguments.is_empty()
                        && !matches!(self.call_return_types.get(producer.as_str()), Some(Type::Float | Type::Double | Type::Void)) =>
                {
                    sources.push(Source::Call(producer.clone()));
                }
                _ => return Ok(false),
            }
        }
        let has_call_source = sources.iter().any(|source| matches!(source, Source::Call(_)));
        if has_call_source && function.locals.len() > 1 {
            return Ok(false);
        }
        // Statements: the call, then one store per local IN DECLARATION ORDER
        // (each local written back to a word global exactly once).
        let Some((Statement::Expression(Expression::Call { name: callee, arguments }), store_statements)) =
            function.statements.split_first()
        else {
            return Ok(false);
        };
        if store_statements.len() != function.locals.len() {
            return Ok(false);
        }
        let mut target_globals = Vec::with_capacity(store_statements.len());
        for (local, statement) in function.locals.iter().zip(store_statements) {
            let Statement::Store { target: Expression::Variable(target_global), value: Expression::Variable(stored) } = statement
            else {
                return Ok(false);
            };
            if stored != &local.name
                || self.locations.contains_key(target_global)
                || !matches!(self.globals.get(target_global.as_str()), Some(Type::Int | Type::UnsignedInt))
            {
                return Ok(false);
            }
            target_globals.push(target_global.clone());
        }
        // Arguments are measured only for the SINGLE-local GLOBAL-source shape;
        // the pair — and any call-producer combination — defers with arguments.
        if (function.locals.len() == 2 || has_call_source) && !arguments.is_empty() {
            return Ok(false);
        }
        let local = &function.locals[0];
        // The call's arguments: small-constant ints (their `li`s ride the
        // prologue's latency slots — measured: two after the mflr, the third
        // after the LR store) and/or the crossing local itself, at most once
        // (measured: `mr rPOS,r31` immediately before the `bl`, after the
        // crossing load). The store writes the local back to a word global.
        enum Argument {
            Constant(i16),
            Local,
        }
        let mut decoded_arguments = Vec::with_capacity(arguments.len());
        for argument in arguments {
            match argument {
                Expression::IntegerLiteral(value) if (i16::MIN as i64..=i16::MAX as i64).contains(value) => {
                    decoded_arguments.push(Argument::Constant(*value as i16));
                }
                Expression::Variable(name) if name == &local.name => decoded_arguments.push(Argument::Local),
                _ => return Ok(false),
            }
        }
        let constant_count = decoded_arguments.iter().filter(|argument| matches!(argument, Argument::Constant(_))).count();
        let local_count = decoded_arguments.len() - constant_count;
        if constant_count > 3
            || local_count > 1
            || matches!(self.call_return_types.get(callee.as_str()), Some(Type::Float | Type::Double))
        {
            return Ok(false);
        }
        let callee = callee.clone();
        let constants: Vec<(u8, i16)> = decoded_arguments
            .iter()
            .enumerate()
            .filter_map(|(position, argument)| match argument {
                Argument::Constant(value) => Some((3 + position as u8, *value)),
                Argument::Local => None,
            })
            .collect();
        let local_argument_register: Option<u8> = decoded_arguments
            .iter()
            .position(|argument| matches!(argument, Argument::Local))
            .map(|position| 3 + position as u8);

        let homes: Vec<u8> = (0..function.locals.len()).map(|_| self.fresh_virtual_general()).collect();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes.clone();
        self.epilogue_lr_first = true;
        if constants.is_empty() {
            self.output.instructions.extend(plan.prologue());
        } else {
            // The measured prologue interleave: `stwu; mflr; li r3 [; li r4];
            // stw r0; [li r5;] stw r31` — the argument materializations fill the
            // mflr and LR-store latency slots.
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -plan.frame_size });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            for &(register, value) in constants.iter().take(2) {
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: 0, immediate: value });
            }
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: plan.frame_size + 4 });
            for &(register, value) in constants.iter().skip(2) {
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: 0, immediate: value });
            }
            self.output.instructions.push(Instruction::StoreWord { s: homes[0], a: 1, offset: plan.frame_size - 4 });
        }
        for (source, &home) in sources.iter().zip(&homes) {
            match source {
                Source::Global(source_global) => self.emit_global_load(source_global, home)?,
                Source::Call(producer) => {
                    // The producing call, its result parked into the home.
                    self.emit_call(producer, &[], None, false)?;
                    let result = mwcc_target::Eabi::general_result().number;
                    self.output.instructions.push(Instruction::Or { a: home, s: result, b: result });
                }
            }
        }
        if let Some(register) = local_argument_register {
            self.output.instructions.push(Instruction::Or { a: register, s: homes[0], b: homes[0] });
            // Passing the local burns one extra internal label in mwcc (measured:
            // the extab/extabindex hidden symbols shift @5/@6 -> @6/@7).
            self.output.anonymous_label_bump += 1;
        }
        self.emit_call(&callee, &[], None, false)?;
        for (target_global, &home) in target_globals.iter().zip(&homes) {
            self.emit_global_store(target_global, Pointee::Int, home)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

}
