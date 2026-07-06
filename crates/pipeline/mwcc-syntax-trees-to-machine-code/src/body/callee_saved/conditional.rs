//! Callee-saved values across CONDITIONAL calls and call-containing loops (the #20/#21 intersection).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Parse an if/guard condition mwcc HOISTS into the prologue as a single
    /// `cmpwi operand, const`: a plain parameter (`if(x)` → implicit `!= 0`) or
    /// `param CMP const` with a small signed constant. Returns the compared operand,
    /// the `cmpwi` immediate, and the SKIP branch (the inverse of the taken condition
    /// — branch past the guarded body when it is false). Ordering comparisons require
    /// a signed `int` operand (`cmpwi` is signed); everything else yields `None`.
    pub(crate) fn parse_hoisted_if_condition<'a>(&self, function: &Function, condition: &'a Expression) -> Option<(&'a String, i16, u8, u8)> {
        match condition {
            Expression::Variable(name) => Some((name, 0, 12, 2)), // if(x): cmpwi x,0; beq
            Expression::Binary { operator, left, right } => {
                let Expression::Variable(name) = left.as_ref() else { return None };
                let immediate = i16::try_from(constant_value(right)?).ok()?;
                let ordering = matches!(operator,
                    BinaryOperator::Less | BinaryOperator::LessEqual
                    | BinaryOperator::Greater | BinaryOperator::GreaterEqual);
                if ordering && function.parameters.iter().find(|parameter| &parameter.name == name).map(|parameter| parameter.parameter_type) != Some(Type::Int) {
                    return None;
                }
                let branch = match operator {
                    BinaryOperator::Equal => (4, 2),         // bne (skip if !=)
                    BinaryOperator::NotEqual => (12, 2),     // beq (skip if ==)
                    BinaryOperator::Less => (4, 0),          // bge (skip if >=)
                    BinaryOperator::LessEqual => (12, 1),    // bgt (skip if >)
                    BinaryOperator::Greater => (4, 1),       // ble (skip if <=)
                    BinaryOperator::GreaterEqual => (12, 0), // blt (skip if <)
                    _ => return None,
                };
                Some((name, immediate, branch.0, branch.1))
            }
            _ => None,
        }
    }

    /// `T f(…, int b, …) { if (b) return call(…); return DEFAULT; }` — an early
    /// return whose value is a CALL, guarding a constant default. mwcc hoists the
    /// condition test into the prologue (`cmpwi b,0` after `mflr`), branches to the
    /// default arm, emits the call return on the taken arm, and both fall into a
    /// shared LR-only epilogue (no callee-saved register — the call's result is
    /// returned directly). Gated narrow: one guard with a call value and a constant
    /// default, no other statements.
    pub(crate) fn try_guarded_call_return(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if matches!(function.return_type, Type::Float | Type::Double) || function.return_type == Type::Void {
            return Ok(false);
        }
        let [guard] = function.guards.as_slice() else { return Ok(false) };
        if !matches!(guard.value, Expression::Call { .. } | Expression::CallThrough { .. }) {
            return Ok(false);
        }
        // The default (fall-through) return must be a constant — a parameter default
        // could be clobbered by the guarded call and needs saving (a later shape).
        let Some(default_return) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if constant_value(default_return).is_none() {
            return Ok(false);
        }
        let Some((cond_name, cmp_constant, skip_bo, skip_bi)) = self.parse_hoisted_if_condition(function, &guard.condition) else {
            return Ok(false);
        };
        let Some(cond_location) = self.locations.get(cond_name) else { return Ok(false) };
        if cond_location.class != ValueClass::General {
            return Ok(false);
        }
        let cond_register = cond_location.register;

        // -- emit -- prologue: stwu; mflr; [cmpwi hoisted]; stw r0 (LR only).
        self.non_leaf = true;
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: cond_register, immediate: cmp_constant });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        let else_label = self.fresh_label();
        let join_label = self.fresh_label();
        self.emit_branch_conditional_to(skip_bo, skip_bi, else_label);
        let result = Eabi::general_result().number;
        // Taken arm: the guard's call, its result in r3, then jump to the epilogue.
        self.evaluate_tail(&guard.value, function.return_type, result)?;
        self.emit_branch_to(join_label);
        // Default arm: materialize the constant.
        self.bind_label(else_label);
        self.evaluate_tail(default_return, function.return_type, result)?;
        self.bind_label(join_label);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }

    /// `void f(int n, …) { while (n) { call(…n…); n = n +/- const; } }` — a loop
    /// counter kept in a callee-saved register (r31) across the body's call, updated
    /// in place each iteration. mwcc saves it in the prologue, runs the rotated loop
    /// (`b test; body; cmpwi r31,0; bne body`), and reloads it after. This composes
    /// the callee-saved prologue with a call-containing loop — the shape in real
    /// counted-work loops. Gated narrow: a single loop-counter parameter, a while over
    /// it, body = call(s) then one in-place `counter +/- const`.
    pub(crate) fn try_callee_saved_call_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::LoopKind;
        if function.return_type != Type::Void || !function.guards.is_empty() || !self.frame_slots.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `while (n)` — the counter, a general parameter, live across the body's calls.
        let Expression::Variable(counter) = condition else { return Ok(false) };
        // Body = call(s), then a single in-place `counter +/- const` update.
        let Some((Statement::Assign { name: update_name, value: update_value }, call_statements)) = body.split_last() else {
            return Ok(false);
        };
        // Exactly one call in the body — two calls reschedule the counter step (deferred).
        if update_name != counter || call_statements.len() != 1 {
            return Ok(false);
        }
        // The in-place counter step `counter +/- const`; emitted directly as `addi`
        // (a reassignment through emit_statement defers in a call context).
        let Expression::Binary { operator, left, right } = update_value else { return Ok(false) };
        if !matches!(left.as_ref(), Expression::Variable(other) if other == counter) {
            return Ok(false);
        }
        let Some(magnitude) = constant_value(right).and_then(|value| i16::try_from(value).ok()) else {
            return Ok(false);
        };
        let step = match operator {
            BinaryOperator::Add => magnitude,
            BinaryOperator::Subtract => magnitude.checked_neg().unwrap_or(0),
            _ => return Ok(false),
        };
        if !call_statements.iter().all(|statement| matches!(statement,
            Statement::Expression(Expression::Call { .. }) | Statement::Expression(Expression::CallThrough { .. })))
        {
            return Ok(false);
        }
        if function.parameters.iter().position(|parameter| &parameter.name == counter).is_none() {
            return Ok(false);
        }
        let Some(location) = self.locations.get(counter) else { return Ok(false) };
        if location.class != ValueClass::General {
            return Ok(false);
        }
        let counter_incoming = location.register;

        // -- emit: callee-saved prologue (no hoisted test — the loop tests at the bottom),
        //    rotated loop, then a manual LR-first epilogue (the LR-reload hoist can't cross
        //    the loop back-edge). --
        self.non_leaf = true;
        self.frame_size = 16;
        let home = self.fresh_virtual_general();
        self.callee_saved = vec![home];
        let plan = mwcc_vreg::FramePlan::sized_for(vec![home]);
        self.output.instructions.extend(plan.prologue_interleaved(&[counter_incoming]));
        if let Some(location) = self.locations.get_mut(counter) {
            location.register = home;
        }
        let skip = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        // The calls (reading the counter from its callee-saved home), then the counter
        // step emitted directly (emit_statement would defer the reassignment).
        for statement in call_statements {
            self.emit_statement(statement)?;
        }
        self.output.instructions.push(Instruction::AddImmediate { d: home, a: home, immediate: step });
        let condition_at = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_at;
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let back = if options == 4 { 12 } else { 4 };
        self.output.instructions.push(Instruction::BranchConditionalForward { options: back, condition_bit, target: body_top });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        self.output.instructions.push(Instruction::LoadWord { d: home, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump = 4; // the while loop's labels
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
        // The if-condition, hoisted into the prologue as `cmpwi operand, const` with an
        // inverse skip branch (shared with the guarded-call-return shape).
        let Some((cond_name, cmp_constant, skip_bo, skip_bi)) = self.parse_hoisted_if_condition(function, condition) else {
            return Ok(false);
        };
        // then-body = plain (void-result) calls only.
        if then_body.is_empty()
            || !then_body.iter().all(|statement| matches!(statement,
                Statement::Expression(Expression::Call { .. }) | Statement::Expression(Expression::CallThrough { .. })))
        {
            return Ok(false);
        }
        // Saved values live across the call: a single returned parameter, or two
        // parameters combined by a low-latency op (`a+b`, `a-b`, `a&b`, …) — the same
        // return-shape the straight-line callee-saved path allows for two saved values.
        let saved_names: Vec<&String> = match function.return_expression.as_ref() {
            Some(Expression::Variable(name)) => vec![name],
            Some(Expression::Binary { operator, left, right })
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor) =>
            {
                match (left.as_ref(), right.as_ref()) {
                    (Expression::Variable(left_name), Expression::Variable(right_name)) if left_name != right_name => vec![left_name, right_name],
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // Each saved value is a distinct general parameter, none the condition operand;
        // a call argument referencing one would keep it in its incoming register.
        let mut promoted: Vec<(usize, String, u8)> = Vec::new();
        for name in &saved_names {
            if *name == cond_name {
                return Ok(false);
            }
            let Some(index) = function.parameters.iter().position(|parameter| &parameter.name == *name) else {
                return Ok(false);
            };
            let Some(location) = self.locations.get(*name) else { return Ok(false) };
            if location.class != ValueClass::General {
                return Ok(false);
            }
            promoted.push((index, (*name).clone(), location.register));
        }
        if then_body.iter().any(|statement| match statement {
            Statement::Expression(Expression::Call { arguments, .. }) => arguments
                .iter()
                .any(|argument| saved_names.iter().any(|name| expression_reads_name(argument, name))),
            _ => false,
        }) {
            return Ok(false);
        }
        let Some(cond_location) = self.locations.get(cond_name) else { return Ok(false) };
        if cond_location.class != ValueClass::General {
            return Ok(false);
        }
        let cond_register = cond_location.register;
        // Highest register (r31) to the last parameter, descending toward the first.
        promoted.sort_by_key(|(index, _, _)| *index);
        let count = promoted.len();

        // -- emit --
        self.non_leaf = true;
        self.frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        let incoming_ordered: Vec<u8> = promoted.iter().rev().map(|(_, _, incoming)| *incoming).collect();
        let mut prologue = plan.prologue_interleaved(&incoming_ordered);
        // mwcc fills the ready slot after `mflr` with the if-condition test.
        prologue.insert(2, Instruction::CompareWordImmediate { a: cond_register, immediate: cmp_constant });
        self.output.instructions.extend(prologue);
        for (rank, (_, name, _)) in promoted.iter().rev().enumerate() {
            if let Some(location) = self.locations.get_mut(name) {
                location.register = homes[rank];
            }
        }
        // Skip past the conditional call when the condition is false.
        let skip = self.fresh_label();
        self.emit_branch_conditional_to(skip_bo, skip_bi, skip);
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        self.bind_label(skip);
        // Epilogue at the join in mwcc's order: the saved LR reloads FIRST (it can only
        // sit after the merge, since the call is one-armed), then the return move/compute,
        // then the saved GPRs (highest first), then `mtlr`/`addi`/`blr`. Hand-emitted —
        // the LR-reload hoist can't cross the branch.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        for (index, &home) in homes.iter().enumerate() {
            self.output.instructions.push(Instruction::LoadWord { d: home, a: 1, offset: self.frame_size - 4 * (index as i16 + 1) });
        }
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // An `if` advances the anonymous `@N` counter by 2 (positional model), which
        // the exception-unwind `@N` entry is numbered against.
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }

}
