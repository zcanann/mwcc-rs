//! The pointer-walker call loop: a NULL-terminated function-pointer table walked and each entry
//! called. This is the C++ static ctor/dtor runner mwcc emits into every REL module's `_prolog`
//! and `_epilog` (`while (*p != 0) { (**p)(); p++; }`).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `_prolog`/`_epilog`: a local pointer walks a NULL-terminated global function-pointer table,
    /// calling each entry. `const VoidFunc* p = _ctors; while (*p != 0) { (**p)(); p++; } … return 0;`
    ///
    /// ```text
    ///   stwu; mflr; lis r3,tbl@ha; stw r0,20; addi r0,r3,tbl@lo; stw r31,12; mr r31,r0
    ///   b .cond
    ///   .body: mtctr r12; bctrl; addi r31,r31,4
    ///   .cond: lwz r12,0(r31); cmplwi r12,0; bne .body
    ///   [bl trailing…]; lwz r0,20; [li r3,0;] lwz r31,12; mtlr; addi r1,r1,16; blr
    /// ```
    ///
    /// The keystone is the r12 reuse: the condition load `lwz r12,0(r31)` (the next table entry)
    /// doubles as the indirect callee, so the body is just `mtctr r12; bctrl`. The walker lives in
    /// r31 (callee-saved, it crosses the calls). A `void` function (`_epilog`) drops the trailing
    /// call and the `li r3,0`; an `int` function returns 0. Any trailing statement other than bare
    /// calls, a non-4-byte table element, or a differently-shaped loop defers.
    pub(crate) fn try_pointer_walker_call_loop(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !self.frame_slots.is_empty() || !function.parameters.is_empty() {
            return Ok(false);
        }
        let returns_int = match function.return_type {
            Type::Void => false,
            Type::Int => true,
            _ => return Ok(false),
        };
        // Exactly one local: a pointer initialized to a global (function-pointer) array's address.
        let [local] = function.locals.as_slice() else { return Ok(false) };
        if local.array_length.is_some() || local.is_static {
            return Ok(false);
        }
        // The table is a global function-pointer array (often an unsized `extern T[]`, so it is not
        // in `global_array_sizes`); its address is taken with an ADDR16 relocation regardless. Only
        // a global symbol qualifies — a local/parameter name would not carry the relocation.
        let Some(Expression::Variable(table)) = &local.initializer else { return Ok(false) };
        if self.locations.contains_key(table.as_str()) {
            return Ok(false);
        }
        let table = table.clone();

        // The first statement is the walk loop; the rest are the trailing calls (and, for an `int`
        // function, a `return 0`).
        let Some((Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }, trailing)) =
            function.statements.split_first()
        else {
            return Ok(false);
        };
        // The condition is `*p != 0` — the table entry loaded and tested against null.
        let walks_local = |expression: &Expression| matches!(expression,
            Expression::Dereference { pointer } if matches!(pointer.as_ref(), Expression::Variable(name) if *name == local.name));
        match condition {
            Expression::Binary { operator: BinaryOperator::NotEqual, left, right }
                if walks_local(left) && matches!(right.as_ref(), Expression::IntegerLiteral(0)) => {}
            _ => return Ok(false),
        }
        // The body is exactly `(**p)(); p++;` — an indirect call through the current entry, then the
        // pointer step. `(**p)()` peels to CallThrough { Dereference(p) }; `p++` in statement
        // position is lowered by the parser to the assignment `p = p + 1`.
        let [Statement::Expression(Expression::CallThrough { target, arguments }), step_statement] = body.as_slice() else {
            return Ok(false);
        };
        if !arguments.is_empty() || !walks_local(target) {
            return Ok(false);
        }
        match step_statement {
            Statement::Assign { name, value: Expression::Binary { operator: BinaryOperator::Add, left, right } }
                if *name == local.name
                    && matches!(left.as_ref(), Expression::Variable(other) if *other == local.name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1)) => {}
            _ => return Ok(false),
        }

        // The trailing statements: zero or more bare calls, then (for `int`) an optional `return 0`.
        let mut trailing_calls: Vec<String> = Vec::new();
        let mut saw_return_zero = false;
        for statement in trailing {
            match statement {
                Statement::Expression(Expression::Call { name, arguments }) if arguments.is_empty() => {
                    trailing_calls.push(name.clone());
                }
                Statement::Return(Some(Expression::IntegerLiteral(0))) if returns_int => {
                    saw_return_zero = true;
                }
                _ => return Ok(false),
            }
        }
        // A trailing call must be a genuine external (a file-scope prototype). A name the TU
        // DEFINES instead — e.g. board_executor.c's `static ObjectSetup` — is one mwcc may INLINE
        // into the caller (`lis BoardCreate; lis BoardDestroy; bl BoardObjectSetup`), which this
        // recognizer does not model, so defer rather than emit a plain `bl`.
        if trailing_calls.iter().any(|name| !self.prototyped_names.contains(name)) {
            return Ok(false);
        }
        let returns_zero = returns_int
            && (saw_return_zero || matches!(function.return_expression.as_ref(), Some(Expression::IntegerLiteral(0))));
        if returns_int && !returns_zero {
            return Ok(false);
        }
        if !returns_int && function.return_expression.is_some() {
            return Ok(false);
        }

        const WALKER: u8 = 31;
        // The table entry is a function pointer (4 bytes); the walk steps by that.
        let step = 4i16;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![WALKER];
        self.output.anonymous_label_bump = 5;

        // Prologue with the table-address materialization interleaved: the `lis` fills the mflr->save
        // latency slot, the `addi` (into the scratch r0) lands after it, then the walker is stashed.
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, &table);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.record_relocation(RelocationKind::Addr16Lo, &table);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: WALKER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::Or { a: WALKER, s: 0, b: 0 });

        // Bottom-tested walk: branch to the condition, which loads the next entry into r12 and tests
        // it; the body reuses that r12 as the callee. A branch back on non-null.
        let skip_to_condition = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::AddImmediate { d: WALKER, a: WALKER, immediate: step });
        let condition_top = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip_to_condition] {
            *target = condition_top;
        }
        self.output.instructions.push(Instruction::LoadWord { d: 12, a: WALKER, offset: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 12, immediate: 0 });
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: body_top });

        // The trailing calls, then the epilogue in final order (the loop branch makes the LR-reload
        // hoist bail): LR reload, the `int` return value in its slot, the walker reload, `mtlr`.
        for name in &trailing_calls {
            self.record_relocation(RelocationKind::Rel24, name);
            self.output.instructions.push(Instruction::BranchAndLink { target: name.clone() });
        }
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        if returns_zero {
            self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 0, immediate: 0 });
        }
        self.output.instructions.push(Instruction::LoadWord { d: WALKER, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
