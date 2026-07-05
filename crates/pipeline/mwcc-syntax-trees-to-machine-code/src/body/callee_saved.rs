//! Callee-saved register families: calls through pointers, park/combine shapes.

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
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: the promoted parameters' homes are virtuals, created highest-rank
        // first — id order reproduces r31, r30, … through the callee-saved pool. The
        // interleaved save+move prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        // A store sink reloads the saved LR before the GPR reloads in the epilogue.
        self.epilogue_lr_first = has_store;
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
            let return_expression = function
                .return_expression
                .as_ref()
                .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `T f(T a, int b, …) { if (b) call(…); return a; }` — a parameter live
    /// across a call that runs only on one arm of an `if`. mwcc saves `a` in
    /// r31, HOISTS the if-condition test into the prologue slot after `mflr`
    /// (`cmpwi b,0` between `mflr r0` and `stw r0,20`), branches around the
    /// call, then returns the saved value. This is the #20/#21 intersection in
    /// its simplest form — a value live across a CONDITIONAL call. Gated narrow:
    /// a single saved general parameter, an `if (param) { calls… }` with empty
    /// else, and calls whose arguments do not reference the saved value.
    pub(crate) fn try_callee_saved_conditional_call(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if matches!(function.return_type, Type::Float | Type::Double) || function.return_type == Type::Void {
            return Ok(false);
        }
        // Body = exactly one `if (cond) { … }` with an empty else.
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        // Condition = a plain parameter (implicit `!= 0`), or `param CMP const` with a
        // small signed constant. Yields the compared operand and the SKIP branch (the
        // inverse of the taken condition — branch past the call when it is false).
        // `cmpwi` is signed, so ordering comparisons require a signed `int` operand.
        let (cond_name, cmp_constant, skip_bo, skip_bi): (&String, i16, u8, u8) = match condition {
            Expression::Variable(name) => (name, 0, 12, 2), // if(x): cmpwi x,0; beq
            Expression::Binary { operator, left, right } => {
                let Expression::Variable(name) = left.as_ref() else { return Ok(false) };
                let Some(constant) = constant_value(right) else { return Ok(false) };
                let Ok(immediate) = i16::try_from(constant) else { return Ok(false) };
                let ordering = matches!(operator,
                    BinaryOperator::Less | BinaryOperator::LessEqual
                    | BinaryOperator::Greater | BinaryOperator::GreaterEqual);
                if ordering && function.parameters.iter().find(|parameter| &parameter.name == name).map(|parameter| parameter.parameter_type) != Some(Type::Int) {
                    return Ok(false);
                }
                let (bo, bi) = match operator {
                    BinaryOperator::Equal => (4, 2),         // bne (skip if !=)
                    BinaryOperator::NotEqual => (12, 2),     // beq (skip if ==)
                    BinaryOperator::Less => (4, 0),          // bge (skip if >=)
                    BinaryOperator::LessEqual => (12, 1),    // bgt (skip if >)
                    BinaryOperator::Greater => (4, 1),       // ble (skip if <=)
                    BinaryOperator::GreaterEqual => (12, 0), // blt (skip if <)
                    _ => return Ok(false),
                };
                (name, immediate, bo, bi)
            }
            _ => return Ok(false),
        };
        // then-body = plain (void-result) calls only.
        if then_body.is_empty()
            || !then_body.iter().all(|statement| matches!(statement,
                Statement::Expression(Expression::Call { .. }) | Statement::Expression(Expression::CallThrough { .. })))
        {
            return Ok(false);
        }
        // Return a single parameter, live across the call.
        let Some(Expression::Variable(saved_name)) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if saved_name == cond_name {
            return Ok(false);
        }
        if function.parameters.iter().position(|parameter| &parameter.name == saved_name).is_none() {
            return Ok(false);
        }
        // A call argument that references the saved value would keep it in its
        // incoming register past the save — not this shape.
        if then_body.iter().any(|statement| match statement {
            Statement::Expression(Expression::Call { arguments, .. }) => arguments.iter().any(|argument| expression_reads_name(argument, saved_name)),
            _ => false,
        }) {
            return Ok(false);
        }
        // Both the saved value and the condition must be general-class parameters.
        let (Some(saved_location), Some(cond_location)) = (self.locations.get(saved_name), self.locations.get(cond_name)) else {
            return Ok(false);
        };
        if saved_location.class != ValueClass::General || cond_location.class != ValueClass::General {
            return Ok(false);
        }
        let saved_incoming = saved_location.register;
        let cond_register = cond_location.register;

        // -- emit --
        self.non_leaf = true;
        self.frame_size = 16;
        let home = self.fresh_virtual_general();
        self.callee_saved = vec![home];
        let plan = mwcc_vreg::FramePlan::sized_for(vec![home]);
        let mut prologue = plan.prologue_interleaved(&[saved_incoming]);
        // mwcc fills the ready slot after `mflr` with the if-condition test.
        prologue.insert(2, Instruction::CompareWordImmediate { a: cond_register, immediate: cmp_constant });
        self.output.instructions.extend(prologue);
        if let Some(location) = self.locations.get_mut(saved_name) {
            location.register = home;
        }
        // Skip past the conditional call when the condition is false.
        let skip = self.fresh_label();
        self.emit_branch_conditional_to(skip_bo, skip_bi, skip);
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        self.bind_label(skip);
        // Epilogue at the join, in mwcc's order: the saved LR reloads FIRST (it can
        // only sit after the merge, since the call is one-armed), then the return
        // move, then the saved GPR, then `mtlr`/`addi`/`blr`. Emitted by hand — the
        // LR-reload hoist can't cross the branch, so it leaves this alone.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.output.instructions.push(Instruction::LoadWord { d: home, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // An `if` advances the anonymous `@N` counter by 2 (positional model), which
        // the exception-unwind `@N` entry is numbered against.
        self.output.anonymous_label_bump += 2;
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

    /// A void function whose body is two or more calls that each pass the SAME argument
    /// list — all the parameters, in order — `f(a,b){ g(a,b); h(a,b); }` (the single-
    /// parameter `f(x){ g(x); h(x); }` is the common case). Each parameter is live across
    /// the calls, so mwcc saves them in callee-saved registers up front (r31 to the last
    /// parameter, descending), interleaving each save with its move; the first call uses
    /// the incoming argument registers directly (no moves), and each later call restores
    /// them. One of the most common real shapes (a state handed to several functions).
    pub(crate) fn try_callee_saved_call_args(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.parameters.is_empty() || matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        // A non-void function returns one of the parameters (or a register-only expression
        // over them) after the calls — the return is the post-call use that keeps them
        // live; a void function needs two or more calls for that liveness. A call in the
        // return is a different shape (call-result), so it defers.
        let returns_value = function.return_type != Type::Void;
        if returns_value {
            match &function.return_expression {
                Some(expression) if !expression_has_call(expression) => {}
                _ => return Ok(false),
            }
        } else if function.statements.len() < 2 {
            return Ok(false);
        }
        // Every statement must be a call whose arguments are exactly the parameters in
        // order, so the first call needs no moves and the live set is all the parameters.
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else { return Ok(false) };
            if arguments.len() != function.parameters.len() {
                return Ok(false);
            }
            for (argument, parameter) in arguments.iter().zip(&function.parameters) {
                if !matches!(argument, Expression::Variable(name) if name == &parameter.name) {
                    return Ok(false);
                }
            }
        }
        // Each parameter's incoming register; all must be general-class.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        let count = incoming.len();
        let frame_size = ((8 + 4 * count as i32 + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first; the interleaved save+move
        // prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().map(|(_, register)| *register).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The first call finds the parameters still in their incoming registers (no
        // moves); afterward they live only in their callee-saved registers.
        self.emit_statement(&function.statements[0])?;
        for (rank, (name, _)) in incoming.iter().rev().enumerate() {
            let register = homes[rank];
            if let Some(location) = self.locations.get_mut(name) {
                location.register = register;
            }
        }
        for statement in &function.statements[1..] {
            self.emit_statement(statement)?;
        }
        // A non-void return reads the parameters from their callee-saved registers; the
        // epilogue scheduler hoists the LR reload ahead of this move, matching mwcc.
        if returns_value {
            let result = Eabi::general_result().number;
            let return_expression = function.return_expression.as_ref().unwrap();
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f(...) + x;` — a single general parameter `x` kept live across a call that sits INSIDE
    /// the return expression, then combined with the call's result. mwcc saves `x` in r31 before the
    /// call (`mr r31,r3`), runs the call (whose argument, when it is `x`, is already in the incoming
    /// register, so no move precedes it), reloads LR, then combines from the callee-saved register
    /// (`add r3,r31,r3` — the saved value first). The call is argument-free or forwards exactly the
    /// parameter; a computed/constant argument schedules its materialization differently and defers.
    /// Handles the low-latency ops `+ | & ^` (commutative — `OP r3,r31,r3` on either source side) and
    /// `-` (its `subf` operands chosen by the call's side); heavier ops (e.g. `*`) and multi-parameter
    /// shapes are follow-ups.
    pub(crate) fn try_callee_saved_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 1 {
            return Ok(false);
        }
        let param = &function.parameters[0];
        let (class, param_register) = match self.locations.get(&param.name) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Low-latency ops mwcc issues as a single register op combining the saved parameter (r31)
        // and the call result (r3). The commutative ops (`+ | & ^`) use `OP r3,r31,r3` on either
        // source side; the non-commutative `-` picks its `subf` operands by which side the call is on.
        // `*` combines to a single `mullw r3,r31,r3`; mwcc issues it BEFORE the LR reload (overlapping
        // the multiply latency with the load), which the LR-reload hoist now models.
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let is_param = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == &param.name);
        let (call_name, call_arguments, call_on_left) = match (left.as_ref(), right.as_ref()) {
            (Expression::Call { name, arguments }, other) if is_param(other) => (name, arguments, true),
            (other, Expression::Call { name, arguments }) if is_param(other) => (name, arguments, false),
            _ => return Ok(false),
        };
        // The call takes no arguments or forwards exactly the parameter (already in its incoming
        // register); anything else materializes an argument on a different schedule.
        if !(call_arguments.is_empty() || (call_arguments.len() == 1 && is_param(&call_arguments[0]))) {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the saved parameter.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the saved parameter's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // Save the live parameter before the call clobbers its incoming register.
        self.output.instructions.push(Instruction::Or { a: saved, s: param_register, b: param_register });
        self.emit_call(call_name, call_arguments, None, false)?;
        // Combine the saved parameter with the call result (r3) — the saved value first.
        let result = Eabi::general_result().number;
        self.output.instructions.push(match operator {
            BinaryOperator::Add => Instruction::Add { d: result, a: saved, b: result },
            BinaryOperator::BitOr => Instruction::Or { a: result, s: saved, b: result },
            BinaryOperator::BitAnd => Instruction::And { a: result, s: saved, b: result },
            BinaryOperator::BitXor => Instruction::Xor { a: result, s: saved, b: result },
            // `subf d,a,b` computes `b - a`. `f()-x` (call left) is result-param -> `subf r3,r31,r3`;
            // `x-f()` (call right) is param-result -> `subf r3,r3,r31`.
            BinaryOperator::Subtract if call_on_left => Instruction::SubtractFrom { d: result, a: saved, b: result },
            BinaryOperator::Subtract => Instruction::SubtractFrom { d: result, a: result, b: saved },
            BinaryOperator::Multiply => Instruction::MultiplyLow { d: result, a: saved, b: result },
            _ => unreachable!("operator restricted above"),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `p(x); q(y);` — two single-argument calls passing two distinct parameters in order (a `void`
    /// body). The second parameter is live across the first call, so mwcc saves it in r31 up front
    /// (`mr r31,r4`), runs the first call (the first parameter is still in its incoming register), then
    /// moves the saved second parameter into place for the second call (`mr r3,r31; bl`). The epilogue
    /// reloads LR (hoisted right after the last call) then restores r31. Exactly two parameters/two
    /// calls for now — longer sequences assign further callee-saved registers and are a follow-up.
    pub(crate) fn try_callee_saved_call_sequence(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void || function.return_expression.is_some() {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call { name: name0, arguments: args0 }), Statement::Expression(Expression::Call { name: name1, arguments: args1 })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if args0.len() != 1 || !is_param(&args0[0], 0) || args1.len() != 1 || !is_param(&args1[0], 1) {
            return Ok(false);
        }
        // Both parameters must be general-class (a float parameter is passed/saved differently).
        let mut param_registers = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => param_registers.push(location.register),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and the saved parameter.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the saved parameter's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // Save the second parameter (live across the first call), and record it there so
        // the second call materializes its argument from the saved home (`mr r3,r31`).
        self.output.instructions.push(Instruction::Or { a: saved, s: param_registers[1], b: param_registers[1] });
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = saved;
        }
        // First call: the first parameter is still in its incoming register (no move).
        self.emit_call(name0, args0, None, false)?;
        // Second call: the second parameter now lives in r31.
        self.emit_call(name1, args1, None, false)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f() - g();` — two argument-free calls whose results are subtracted in the return. mwcc
    /// runs the first call, saves its result in r31 (`mr r31,r3`, live across the second call), runs
    /// the second call (its result in r3), reloads LR, then `subf r3,r3,r31` (= r31 - r3 = f() - g()).
    /// Only `-` for now: a COMMUTATIVE op evaluates its operands right-first in mwcc, reordering the
    /// symbol table, which the left-first `symbol_order` does not reproduce (a `referenced_names`
    /// change — deferred). Argument-bearing calls are a follow-up.
    pub(crate) fn try_callee_saved_two_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // Only `-` for now. A commutative `+`/`|`/… evaluates its operands RIGHT-first in mwcc (so the
        // symbol/relocation order is right-then-left), which our left-first `symbol_order` does not
        // reproduce — that needs a `referenced_names` change and defers. `-` is natural left-then-right.
        let Some(Expression::Binary { operator: BinaryOperator::Subtract, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Both operands are argument-free calls (an argument would interleave its materialization with
        // the saves on a schedule not modeled here).
        let (Expression::Call { name: first_name, arguments: first_arguments }, Expression::Call { name: second_name, arguments: second_arguments }) = (left.as_ref(), right.as_ref()) else {
            return Ok(false);
        };
        if !first_arguments.is_empty() || !second_arguments.is_empty() {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the saved result.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the first result's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // First call; its result is saved across the second call.
        self.emit_call(first_name, first_arguments, None, false)?;
        self.output.instructions.push(Instruction::Or { a: saved, s: 3, b: 3 });
        // Second call; its result lands in r3.
        self.emit_call(second_name, second_arguments, None, false)?;
        // `subf d,a,b` = `b - a`; first result saved, second in r3, so `subf r3,r3,<saved>` =
        // first - second (`f() - g()`).
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: saved });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `g(x); return x OP y;` — TWO parameters both live across a single call (the first is passed to
    /// it, the second is only used in the return), combined by a low-latency op (`+ | & ^`, or `-`
    /// whose `subf` order `evaluate_tail` reproduces). mwcc
    /// preserves BOTH in callee-saved registers — the last parameter in r31, the first in r30 —
    /// saving them interleaved up front (`stw r31; mr r31,y; stw r30; mr r30,x`); the return combines
    /// from the saved registers (`add r3,r30,r31`). The call may pass EITHER parameter: the first stays
    /// in its incoming register (no move); the second is materialized from its saved r31 (`mr r3,r31`).
    pub(crate) fn try_callee_saved_param_pair_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call { name, arguments })] = function.statements.as_slice() else {
            return Ok(false);
        };
        // The call passes exactly one of the two parameters (the first stays in its incoming register;
        // the second is materialized from its callee-saved register — see the save/location logic).
        if arguments.len() != 1 || !matches!(&arguments[0], Expression::Variable(argument) if argument == &function.parameters[0].name || argument == &function.parameters[1].name) {
            return Ok(false);
        }
        // The return is `p OP q` reading both parameters, combined by a low-latency op whose operand
        // order `evaluate_tail` reproduces (source order for the commutative ops; the correct `subf`
        // for `-`). `*` is excluded here: with TWO saved GPRs mwcc interleaves the LR reload between
        // the register restores (`mullw; lwz r31; lwz r0; lwz r30`), a register-death epilogue schedule
        // this path does not model — the single-saved-GPR combine handles multiply.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if !((is_param(left, 0) && is_param(right, 1)) || (is_param(left, 1) && is_param(right, 0))) {
            return Ok(false);
        }
        // Both parameters general-class; keep them in incoming (parameter) order for the save loop.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and r31 + r30.
        let frame_size = 16i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first (id order -> r31, r30); the
        // interleaved save+move prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..incoming.len()).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().map(|(_, register)| *register).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The second parameter is now read from its callee-saved home (its incoming register
        // is dead), so a call passing it materializes `mr r3,r31`. The first parameter stays in its
        // incoming register for the call (no move) and moves to its home only afterward.
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = homes[0];
        }
        self.emit_call(name, arguments, None, false)?;
        if let Some(location) = self.locations.get_mut(&function.parameters[0].name) {
            location.register = homes[1];
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A single local initialized by a call whose argument forwards the (single)
    /// parameter, returned combined with that parameter:
    /// `int x = g(a); return x + a;`. The parameter crosses the call: mwcc saves
    /// it in r31 (interleaved save+move prologue), the call reads the STILL-LIVE
    /// incoming register (no argument move), and the combine reads the result
    /// from r3 and the parameter from r31 (measured: the fire-498 probe).
    pub(crate) fn try_callee_saved_result_param_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 1 || function.locals.len() != 1 {
            return Ok(false);
        }
        let parameter = &function.parameters[0];
        let local = &function.locals[0];
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // The initializer is a call forwarding the parameter in its natural
        // register position (or no arguments) — no argument moves to schedule.
        let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
            return Ok(false);
        };
        match arguments.as_slice() {
            [] => {}
            [Expression::Variable(argument)] if argument == &parameter.name => {}
            _ => return Ok(false),
        }
        // The return combines the local and the parameter with one low-latency op
        // (either operand order; evaluate_tail reproduces it). Multiply is excluded:
        // its latency reschedules the epilogue restores.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let reads = |expression: &Expression, name: &str| matches!(expression, Expression::Variable(variable) if variable == name);
        if !((reads(left, &local.name) && reads(right, &parameter.name))
            || (reads(left, &parameter.name) && reads(right, &local.name)))
        {
            return Ok(false);
        }
        let incoming = match self.locations.get(&parameter.name) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };
        // Prologue: a 16-byte frame saving the link register and r31, the
        // parameter's save+move interleaved (stw r31; mr r31,r3).
        let frame_size = 16i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        let homes: Vec<u8> = vec![self.fresh_virtual_general()];
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        self.output.instructions.extend(plan.prologue_interleaved(&[incoming]));
        // The call reads the parameter from its STILL-LIVE incoming register (no
        // move); only afterwards does the parameter read from its saved home.
        self.emit_call(name, arguments, None, false)?;
        if let Some(location) = self.locations.get_mut(&parameter.name) {
            location.register = homes[0];
        }
        // The local IS the call result: it lives (and dies) in r3.
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: 3, signed, width: 32, pointee: None, stride: None },
        );
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A call-result local added to TWO call-crossing saved values (measured
    /// fire 498/499 probes): `int x = g(a); return x + a + b;` (both parameters
    /// cross) and `int x = g(a); int y = g(x); return y + a + x;` (the first
    /// result crosses the second call). mwcc REASSOCIATES `(res + s1) + s2` into
    /// `res + (s1 + s2)`: the fresh result parks in r0 (`mr r0,r3`), the two
    /// callee-saved homes combine first (`add r3,r30,r31` — creation order), and
    /// the parked value adds last. Adds only — the reassociation is unmeasured
    /// for the other operators.
    pub(crate) fn try_callee_saved_result_park_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // The return is ((result + saved1) + saved2), left-associated adds.
        let Some(Expression::Binary { operator: BinaryOperator::Add, left: outer_left, right: outer_right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Add, left: inner_left, right: inner_right } = outer_left.as_ref() else {
            return Ok(false);
        };
        let (Expression::Variable(result_name), Expression::Variable(saved1), Expression::Variable(saved2)) =
            (inner_left.as_ref(), inner_right.as_ref(), outer_right.as_ref())
        else {
            return Ok(false);
        };
        let all_int = |declared: Type| matches!(declared, Type::Int | Type::UnsignedInt);
        match (function.parameters.len(), function.locals.len()) {
            // `int x = g(a); return x + a + b;` — saved1 = a (param 0 -> r30),
            // saved2 = b (param 1 -> r31), the result local dies in r3.
            (2, 1) => {
                let local = &function.locals[0];
                if !all_int(local.declared_type)
                    || result_name != &local.name
                    || saved1 != &function.parameters[0].name
                    || saved2 != &function.parameters[1].name
                {
                    return Ok(false);
                }
                let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                    return Ok(false);
                };
                match arguments.as_slice() {
                    [] => {}
                    [Expression::Variable(argument)] if argument == &function.parameters[0].name => {}
                    _ => return Ok(false),
                }
                let mut incoming = Vec::new();
                for parameter in &function.parameters {
                    match self.locations.get(&parameter.name) {
                        Some(location) if location.class == ValueClass::General => incoming.push(location.register),
                        _ => return Ok(false),
                    }
                }
                self.non_leaf = true;
                self.frame_size = 16;
                // Reverse creation order: the LAST-created saved value takes r31.
                let homes: Vec<u8> = (0..2).map(|_| self.fresh_virtual_general()).collect();
                self.callee_saved = homes.clone();
                let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
                debug_assert_eq!(plan.frame_size, 16);
                // Interleaved save+fill pairs, r31 (param 1) first.
                let incoming_ordered: Vec<u8> = incoming.iter().rev().copied().collect();
                self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
                // The call reads the STILL-LIVE incoming register (no move).
                self.emit_call(name, arguments, None, false)?;
                self.emit_park_and_combine(homes[1], homes[0]);
                self.emit_epilogue_and_return();
                Ok(true)
            }
            // `int x = g(a); int y = g(x); return y + a + x;` — saved1 = a
            // (created first -> r30), saved2 = x (-> r31), y dies in r3.
            (1, 2) => {
                let (first, second) = (&function.locals[0], &function.locals[1]);
                if !all_int(first.declared_type)
                    || !all_int(second.declared_type)
                    || result_name != &second.name
                    || saved1 != &function.parameters[0].name
                    || saved2 != &first.name
                {
                    return Ok(false);
                }
                let Some(Expression::Call { name: call1, arguments: arguments1 }) = first.initializer.as_ref() else {
                    return Ok(false);
                };
                let Some(Expression::Call { name: call2, arguments: arguments2 }) = second.initializer.as_ref() else {
                    return Ok(false);
                };
                // Call 1 forwards the parameter (or nothing); call 2 passes the
                // FRESH first result — both read a still-live r3, no moves.
                match arguments1.as_slice() {
                    [] => {}
                    [Expression::Variable(argument)] if argument == &function.parameters[0].name => {}
                    _ => return Ok(false),
                }
                if !matches!(arguments2.as_slice(), [Expression::Variable(argument)] if argument == &first.name) {
                    return Ok(false);
                }
                let incoming = match self.locations.get(&function.parameters[0].name) {
                    Some(location) if location.class == ValueClass::General => location.register,
                    _ => return Ok(false),
                };
                self.non_leaf = true;
                self.frame_size = 16;
                let homes: Vec<u8> = (0..2).map(|_| self.fresh_virtual_general()).collect();
                self.callee_saved = homes.clone();
                let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
                debug_assert_eq!(plan.frame_size, 16);
                // Batched saves; the parameter's fill follows (its def is the
                // prologue), the first result's fill lands after its call.
                self.output.instructions.extend(plan.prologue());
                self.output.instructions.push(Instruction::Or { a: homes[1], s: incoming, b: incoming });
                self.emit_call(call1, arguments1, None, false)?;
                self.output.instructions.push(Instruction::Or { a: homes[0], s: 3, b: 3 });
                // The second call reads the first result from the STILL-LIVE r3
                // (its home copy exists but no argument move is emitted).
                let signed = !matches!(first.declared_type, Type::UnsignedInt);
                self.locations.insert(
                    first.name.clone(),
                    Location { class: ValueClass::General, register: 3, signed, width: 32, pointee: None, stride: None },
                );
                self.emit_call(call2, arguments2, None, false)?;
                self.emit_park_and_combine(homes[1], homes[0]);
                self.emit_epilogue_and_return();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// A local COMPUTED from the parameters, consumed by a call, and combined
    /// with that call's result: `int x = a*5+2; int y = g(x); return y + x;`
    /// (the fire-498 probe). The computation lands directly in the callee-saved
    /// home (intermediates in place, the scheduler hoists them into the
    /// prologue's latency gaps), the argument move `mr r3,r31` IS emitted (the
    /// home is not the argument register), and the combine reads r3 and r31.
    pub(crate) fn try_callee_saved_computed_then_call(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [computed, result] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(computed.declared_type, Type::Int | Type::UnsignedInt)
            || !matches!(result.declared_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        // The first local computes WITHOUT calling (its expression evaluates into
        // the home); the second is a call passing exactly the first.
        let Some(initializer) = computed.initializer.as_ref() else {
            return Ok(false);
        };
        if crate::analysis::expression_has_call(initializer) {
            return Ok(false);
        }
        let Some(Expression::Call { name, arguments }) = result.initializer.as_ref() else {
            return Ok(false);
        };
        if !matches!(arguments.as_slice(), [Expression::Variable(argument)] if argument == &computed.name) {
            return Ok(false);
        }
        // The return combines the result and the computed local with one
        // low-latency op, either order (evaluate_tail reproduces it).
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let reads = |expression: &Expression, name: &str| matches!(expression, Expression::Variable(variable) if variable == name);
        if !((reads(left, &result.name) && reads(right, &computed.name))
            || (reads(left, &computed.name) && reads(right, &result.name)))
        {
            return Ok(false);
        }
        self.non_leaf = true;
        self.frame_size = 16;
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The computation's INTERMEDIATES stay in place on the dying source
        // register; only the ROOT op lands in the callee-saved home (measured:
        // `mulli r3,r3,5; addi r31,r3,2` — the in-place mulli carries no
        // anti-dependence against the r31 save, so the scheduler may hoist it
        // into the prologue's latency gap). Only the measured root shape —
        // `<single-param subexpression> + <i16 literal>` — is emitted; other
        // roots decline to the honest defer.
        let (sub, literal) = match initializer {
            Expression::Binary { operator: BinaryOperator::Add, left, right } => match right.as_ref() {
                Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok() => (left.as_ref(), *value as i16),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        // The subexpression must be the measured ONE-instruction in-place shape
        // (`param * literal` -> mulli): the scheduler slot it fills — between
        // mflr and the LR store — is only measured for a single instruction.
        let (in_place_source, factor) = match sub {
            Expression::Binary { operator: BinaryOperator::Multiply, left, right } => match (left.as_ref(), right.as_ref()) {
                (Expression::Variable(source), Expression::IntegerLiteral(value)) if i16::try_from(*value).is_ok() => (source, *value as i16),
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| &parameter.name == in_place_source) {
            return Ok(false);
        }
        let in_place = match self.locations.get(in_place_source.as_str()) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };
        // The prologue with the in-place multiply spliced into the mflr latency
        // gap (measured: stwu; mflr; MULLI; stw r0; stw r31), then the root op
        // landing in the home.
        let prologue = mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue();
        self.output.instructions.extend(prologue[..2].iter().cloned());
        self.output.instructions.push(Instruction::MultiplyImmediate { d: in_place, a: in_place, immediate: factor });
        self.output.instructions.extend(prologue[2..].iter().cloned());
        self.output.instructions.push(Instruction::AddImmediate { d: saved, a: in_place, immediate: literal });
        let signed = !matches!(computed.declared_type, Type::UnsignedInt);
        self.locations.insert(
            computed.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        self.emit_call(name, arguments, None, false)?;
        let result_signed = !matches!(result.declared_type, Type::UnsignedInt);
        self.locations.insert(
            result.name.clone(),
            Location { class: ValueClass::General, register: 3, signed: result_signed, width: 32, pointee: None, stride: None },
        );
        let destination = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, destination)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The measured reassociation tail: park the fresh call result in r0,
    /// combine the two callee-saved homes (creation order: lower home first),
    /// then add the parked value into the return register.
    pub(crate) fn emit_park_and_combine(&mut self, home_low: u8, home_high: u8) {
        self.output.instructions.push(Instruction::Or { a: 0, s: 3, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 3, a: home_low, b: home_high });
        self.output.instructions.push(Instruction::Add { d: 3, a: 0, b: 3 });
    }

    /// One or two locals that are CALL RESULTS, live across later calls, then returned:
    /// `int z = g(); h(); return z;` or `int a = g1(); int b = g2(); h(); return a+b;`.
    /// mwcc preserves them in r31 (and r30) across the later calls — each producing call
    /// is followed by a move into its callee-saved register, all saved up front. The
    /// single-local return may post-process z (`z + 1`); the two-local return must be a
    /// single low-latency op of both (`a + b`), as in [`Self::try_callee_saved`].
    /// (Parameters live across calls go through that path.) Narrowly shaped.
    pub(crate) fn try_callee_saved_call_result(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // One or two general-int locals, each initialized by an argument-free call.
        let count = function.locals.len();
        if count == 0 || count > 2 {
            return Ok(false);
        }
        let mut init_calls: Vec<(String, Vec<Expression>)> = Vec::new();
        for local in &function.locals {
            if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
                return Ok(false);
            }
            let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                return Ok(false);
            };
            init_calls.push((name.clone(), arguments.clone()));
        }
        // A producing call's arguments are allowed only in the single-local case, and
        // only when they forward parameters in their natural register positions (arg i
        // is parameter i, all parameters general) — then the parameters are already in
        // place, no moves are emitted, and the sequence matches the argument-free shape.
        // A constant/reordered argument would schedule its materialization differently,
        // and a second producing call's parameter would be call-clobbered; both defer.
        let params_all_general = !function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double));
        for (index, (_, arguments)) in init_calls.iter().enumerate() {
            if arguments.is_empty() {
                continue;
            }
            let forwards_parameters = count == 1
                && index == 0
                && params_all_general
                && arguments.len() <= function.parameters.len()
                && arguments
                    .iter()
                    .enumerate()
                    .all(|(position, argument)| matches!(argument, Expression::Variable(name) if name == &function.parameters[position].name));
            if !forwards_parameters {
                return Ok(false);
            }
        }
        // The return reads no parameter (it would be a call-clobbered register) and no
        // global (its load reschedules against the epilogue). A single local may be
        // post-processed (`z + 1`); two locals must combine in one low-latency op
        // (`a + b`), the only shape whose restores aren't rescheduled.
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if function.parameters.iter().any(|parameter| expression_reads_name(return_expr, &parameter.name)) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(return_expr, name)) {
            return Ok(false);
        }
        if count == 1 {
            if !expression_reads_name(return_expr, &function.locals[0].name) {
                return Ok(false);
            }
        } else {
            let single_op = matches!(return_expr, Expression::Binary { operator, left, right }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                    && matches!(left.as_ref(), Expression::Variable(_))
                    && matches!(right.as_ref(), Expression::Variable(_)));
            if !single_op || !function.locals.iter().all(|local| expression_reads_name(return_expr, &local.name)) {
                return Ok(false);
            }
        }
        // The body is one or more straight-line argument-free calls (so the locals are
        // genuinely live across a call).
        if function.statements.is_empty() {
            return Ok(false);
        }
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if !arguments.is_empty() {
                return Ok(false);
            }
        }

        // Prologue: a frame saving the link register and the callee-saved registers
        // (r31, then r30), all up front, highest at the top of the frame.
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, created highest-rank first (id order -> r31, r30, …),
        // framed by the FRAME BUILDER (all saves consecutive — the canonical schedule).
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        self.output.instructions.extend(plan.prologue());

        // Each local: its producing call, then move r3 into the local's callee-saved
        // register — the first local takes the lowest (r30 when there are two), the last
        // takes r31, matching mwcc's `bl g1; mr r30,r3; bl g2; mr r31,r3`.
        for (index, local) in function.locals.iter().enumerate() {
            let (init_name, init_arguments) = &init_calls[index];
            self.emit_call(init_name, init_arguments, None, false)?;
            // The first local takes the LOWEST home (homes are highest-first).
            let register = homes[count - 1 - index];
            self.output.instructions.push(Instruction::Or { a: register, s: 3, b: 3 });
            let signed = !matches!(local.declared_type, Type::UnsignedInt);
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register, signed, width: 32, pointee: None, stride: None },
            );
        }

        // The later calls, then the return. The LR-reload hoist places the saved-LR
        // reload right after the last call, matching mwcc's epilogue order.
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A single local COMPUTED from parameters (no call in its initializer) that is live
    /// across a call — passed to it and/or post-processed in the return:
    /// `int z = x + 1; g(z); return z;`. z is computed into r31 before the call, used
    /// from r31 (as a call argument and/or the return), then reloaded. Argument calls may
    /// pass only z and constants (a parameter would be call-clobbered).
    pub(crate) fn try_callee_saved_computed_local(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.locals.len() != 1 {
            return Ok(false);
        }
        let local = &function.locals[0];
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(initializer) = local.initializer.as_ref() else {
            return Ok(false);
        };
        // A genuinely computed initializer: not a bare variable (that keeps its source
        // register), not a call (the call-result path), reading no global.
        if matches!(initializer, Expression::Variable(_)) || expression_has_call(initializer) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(initializer, name)) {
            return Ok(false);
        }
        // One or more argument calls whose arguments read only z (preserved in r31) and
        // constants; the return reads z and no parameter/global. (A parameter in either
        // would be read from a call-clobbered register.)
        if function.statements.is_empty() {
            return Ok(false);
        }
        let reads_param_or_global = |this: &Self, expression: &Expression| {
            function.parameters.iter().any(|parameter| expression_reads_name(expression, &parameter.name))
                || this.globals.keys().any(|name| expression_reads_name(expression, name))
        };
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if arguments.iter().any(|argument| reads_param_or_global(self, argument)) {
                return Ok(false);
            }
        }
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !expression_reads_name(return_expr, &local.name) || reads_param_or_global(self, return_expr) {
            return Ok(false);
        }

        // Prologue, then compute z into r31, then the argument calls, then the return.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the computed local's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        self.evaluate_general(initializer, saved)?;
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

}
