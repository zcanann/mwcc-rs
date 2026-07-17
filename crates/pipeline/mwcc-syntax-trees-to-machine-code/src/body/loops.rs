//! Counter, CTR, and rotated loop families.

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
        if !function.guards.is_empty() || !function.locals.is_empty() || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
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
                    Expression::Binary { operator: BinaryOperator::Subtract, left, right },
                ) if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1)) =>
                {
                    name.clone()
                }
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| parameter.name == counter) {
            return Ok(false);
        }
        // The body must be straight-line calls that do not pass the counter as an
        // argument (the first such use would stay in the incoming register — the
        // value-location nuance the callee-saved path also defers).
        if body.iter().any(|statement| !matches!(statement, Statement::Expression(_))) {
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
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) { 6 } else { 4 };
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: SAVED, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::Or { a: SAVED, s: incoming, b: incoming });
        if let Some(location) = self.locations.get_mut(&counter) {
            location.register = SAVED;
        }

        // A while loop tests the condition first: branch down to the
        // decrement-and-test, which falls through into the body on re-entry.
        let skip_to_condition = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
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
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: SAVED, a: SAVED, immediate: -1 });
        // `bne body_top`: branch when CR0[EQ] is clear (BO=4, BI=2). Backward, which
        // the encoder resolves from the instruction indices.
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: body_top });

        // Epilogue, emitted in final order (the loop's branch makes the scheduler and
        // the LR-reload hoist bail): the LR reload comes before the r31 reload.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        self.output.instructions.push(Instruction::LoadWord { d: SAVED, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
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
        if function.return_type != Type::Void || !function.guards.is_empty() || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        if !function.parameters.is_empty() {
            return Ok(false);
        }
        // The counter: one int local, uninitialized at declaration (`int i;`).
        let [counter] = function.locals.as_slice() else {
            return Ok(false);
        };
        if counter.is_static || counter.array_length.is_some() || counter.initializer.is_some() || counter.data_bytes.is_some() {
            return Ok(false);
        }
        if !matches!(counter.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::For, initializer: Some(initializer), condition: Some(condition), step: Some(step), body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = K` … `i < N` … `i++` (parsed as `i = i + 1`). K must be statically
        // below the bound (the dropped pre-test is only valid when the first
        // iteration is certain; a never-running loop is eliminated differently).
        let start = match initializer {
            Expression::Assign { target, value }
                if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name) =>
            {
                match value.as_ref() {
                    Expression::IntegerLiteral(start) if *start >= 0 && *start <= i16::MAX as i64 => *start,
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        let bound = match condition {
            Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name) =>
            {
                match right.as_ref() {
                    Expression::IntegerLiteral(bound) if *bound > 0 && *bound <= i16::MAX as i64 => *bound,
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
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![home]).prologue());
        self.output.instructions.push(Instruction::AddImmediate { d: home, a: 0, immediate: start as i16 });
        let loop_top = self.output.instructions.len();
        if passes_counter {
            self.output.instructions.push(Instruction::Or { a: 3, s: home, b: home });
        }
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchAndLink { target: name.to_string() });
        self.output.instructions.push(Instruction::AddImmediate { d: home, a: home, immediate: 1 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: home, immediate: bound as i16 });
        // `blt loop` — BO 12 (branch-if-true), BI 0 (cr0 LT); backward target.
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: loop_top });
        // The loop's backward branch makes the LR-reload hoist bail, so the epilogue
        // is emitted in final order: LR reload FIRST, then the home (measured — the
        // same order as the do-while counter shape).
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        self.output.instructions.push(Instruction::LoadWord { d: home, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A leaf `void` function whose whole body is an EMPTY-body busy-wait on one
    /// fixed-address array element — the hardware-register poll (`while (__EXIRegs[13] & 1);`,
    /// DebuggerDriver/EXI/SI/DSP spin loops). mwcc materializes the ELEMENT address once
    /// (`lis`/`addi` of the folded `base + index*elem`), then loops load → test → branch
    /// back (the volatile reload is the loop):
    ///
    /// ```text
    ///   lis rB, elem@ha ; addi rB, rB, elem@lo
    ///   loop: lwz r0,0(rB) ; rlwinm. r0,r0,0,mb,me ; bne loop     (`& CONTIGUOUS_MASK`)
    ///                        cmplwi r0,0            ; bne loop     (truthy)
    /// ```
    ///
    /// A `!(…)` wrapper flips `bne` to `beq` (wait-until-set). Element widths: u32 `lwz`,
    /// u16 `lhz`, u8 `lbz`. Measured 1.3.2/2.0/2.7: masks 1/3/0x100/0x8000/0x200-on-u16,
    /// truthy, negated. A non-contiguous mask, non-constant index, or any other condition
    /// shape falls through (the general loop defer). Returns whether this path applied.
    pub(crate) fn try_emit_busy_wait(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !body.is_empty() {
            return Ok(false);
        }
        // Strip a logical-not wrapper: `while (!(R[i] & m));` waits for the bit to SET,
        // so the backward branch re-enters while the test result is ZERO (`beq`).
        let (condition, negated) = match condition {
            Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => (operand.as_ref(), true),
            other => (other, false),
        };
        // The testable forms: `R[c] & mask` (contiguous mask -> one `rlwinm.`) or bare `R[c]`.
        let (element_access, mask) = match condition {
            Expression::Binary { operator: BinaryOperator::BitAnd, left, right } => match (left.as_ref(), right.as_ref()) {
                (access, Expression::IntegerLiteral(mask)) => (access, Some(*mask)),
                (Expression::IntegerLiteral(mask), access) => (access, Some(*mask)),
                _ => return Ok(false),
            },
            access => (access, None),
        };
        let Expression::Index { base, index } = element_access else {
            return Ok(false);
        };
        let (Expression::Variable(name), Expression::IntegerLiteral(index)) = (base.as_ref(), index.as_ref()) else {
            return Ok(false);
        };
        let Some(&(address, element)) = self.fixed_address_arrays.get(name) else {
            return Ok(false);
        };
        // The mask must be one contiguous run of ones (a single `rlwinm.` range).
        let mask_bits = match mask {
            Some(mask) => {
                let bits = mask as u64 as u32;
                if bits == 0 {
                    return Ok(false);
                }
                let low = bits.trailing_zeros();
                let high = 31 - bits.leading_zeros();
                let contiguous = (bits >> low).count_ones() == high - low + 1 && bits >> low == (1u64 << (high - low + 1)) as u32 - 1;
                if !contiguous {
                    return Ok(false);
                }
                Some((31 - high) as u8..=(31 - low) as u8) // PPC bit numbering: mb..=me
            }
            None => None,
        };
        let (load, element_bytes): (fn(u8, u8, i16) -> Instruction, u32) = match element {
            Type::Int | Type::UnsignedInt => (|d, a, offset| Instruction::LoadWord { d, a, offset }, 4),
            Type::Short | Type::UnsignedShort => (|d, a, offset| Instruction::LoadHalfwordZero { d, a, offset }, 2),
            Type::Char | Type::UnsignedChar => (|d, a, offset| Instruction::LoadByteZero { d, a, offset }, 1),
            _ => return Ok(false),
        };

        // The loop-invariant address: element 0's hoisted invariant is just the `lis`
        // half (the `lo` rides the load's displacement, re-read each iteration);
        // a non-zero element hoists the FULL folded address (`lis`+`addi`) and the
        // load runs at displacement 0. Measured: R[0] -> `lis; loop: lwz lo(rB)`,
        // R[13] -> `lis; addi; loop: lwz 0(rB)`.
        let element_address = address as u32 + *index as u32 * element_bytes;
        let base_register = self.lowest_free_general()?;
        let high = ((element_address.wrapping_add(0x8000)) >> 16) as u16;
        let low = element_address as u16 as i16;
        // The loop's internal labels advance mwcc's anonymous-`@N` counter: by 6 for
        // an element-0 poll, by 7 for a non-zero element (the folded full-address
        // temporary adds one) — measured against the no-loop baseline (@9 -> @15/@16).
        self.output.anonymous_label_bump = if *index == 0 { 6 } else { 7 };
        self.output.instructions.push(Instruction::AddImmediateShifted { d: base_register, a: 0, immediate: high as i16 });
        let load_offset = if *index == 0 {
            low
        } else {
            self.output.instructions.push(Instruction::AddImmediate { d: base_register, a: base_register, immediate: low });
            0
        };

        // loop: load; test (sets cr0); branch back while waiting.
        let loop_top = self.output.instructions.len();
        self.output.instructions.push(load(0, base_register, load_offset));
        match mask_bits {
            Some(range) => self.output.instructions.push(Instruction::RotateAndMaskRecord {
                a: 0,
                s: 0,
                shift: 0,
                begin: *range.start(),
                end: *range.end(),
            }),
            None => self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }),
        }
        // `bne loop` re-enters while the bit is SET (wait-for-clear); a negated
        // condition re-enters while ZERO (`beq loop`, wait-for-set).
        let (options, condition_bit) = if negated { (12, 2) } else { (4, 2) };
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: loop_top });
        self.emit_epilogue_and_return();
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
        let [Statement::Loop { kind, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
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
        if let Expression::Binary { operator, left, right } = condition {
            if crate::analysis::is_comparison(*operator) {
                let dereference_vs_constant = (matches!(left.as_ref(), Expression::Dereference { .. }) && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
                    || (matches!(right.as_ref(), Expression::Dereference { .. }) && matches!(left.as_ref(), Expression::IntegerLiteral(_)));
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
            let is_pointer = self.locations.get(name).map_or(false, |location| location.pointee.is_some());
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
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) { 6 } else { 4 };
        // A while tests the condition first: branch down to the test; a do-while falls into the body.
        let skip = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        let body_top = self.output.instructions.len();
        for statement in body {
            if let Statement::Assign { name, value } = statement {
                let register = self.lookup_general(name).expect("gated to a register variable above");
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
        self.output.instructions.push(Instruction::BranchConditionalForward { options: back, condition_bit, target: body_top });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// `T* f(T* p, …) { while (p) { if (p->field CMP x) return p; p = p->next; } return 0; }`
    /// — a linked-list search. mwcc keeps the rotated chase loop and lowers the in-body
    /// early return to a `bclr` (the searched pointer is already in r3, returned unmoved),
    /// followed by the null default after the loop. Leaf; gated to the exact search shape.
    pub(crate) fn try_list_search_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::LoopKind;
        if !function.guards.is_empty() || !self.frame_slots.is_empty() || !function.locals.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if matches!(function.return_type, Type::Float | Type::Double) || function.return_type == Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // A constant default return after the loop (`return 0;`).
        let Some(default_return) = function.return_expression.as_ref() else { return Ok(false) };
        if constant_value(default_return).is_none() {
            return Ok(false);
        }
        // `while (p)` — the searched pointer, which must be the FIRST parameter so it sits
        // in r3 and the in-body `return p` is a bare `bclr` (no move).
        let Expression::Variable(loop_ptr) = condition else { return Ok(false) };
        if function.parameters.first().map(|parameter| &parameter.name) != Some(loop_ptr) {
            return Ok(false);
        }
        let Some(loop_register) = self.lookup_general(loop_ptr) else { return Ok(false) };
        if loop_register != Eabi::general_result().number {
            return Ok(false);
        }
        // Body = [ if (COND) return <p>; , <p> = <chase of p>; ] with an empty else.
        let [Statement::If { condition: if_condition, then_body, else_body }, Statement::Assign { name: chase_name, value: chase_value }] = body.as_slice()
        else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        // The in-body early return: either the loop pointer itself (a bare `bclr`, no
        // move) or a constant flag (materialize + `blr`, reached past a forward branch
        // that skips the found arm when the condition is false).
        let [Statement::Return(Some(return_value))] = then_body.as_slice() else { return Ok(false) };
        let returns_pointer = matches!(return_value, Expression::Variable(other) if other == loop_ptr);
        if (!returns_pointer && constant_value(return_value).is_none()) || chase_name != loop_ptr {
            return Ok(false);
        }
        let is_chase = matches!(chase_value, Expression::Member { base, .. }
                if matches!(base.as_ref(), Expression::Variable(other) if other == loop_ptr))
            || matches!(chase_value, Expression::Dereference { pointer }
                if matches!(pointer.as_ref(), Expression::Variable(other) if other == loop_ptr));
        if !is_chase {
            return Ok(false);
        }

        // -- emit: b test; body{ if-cond, found-arm, chase }; test: cmplwi; bne body; default; blr --
        self.output.anonymous_label_bump = 6; // while (4) + the inner if (2)
        let result = Eabi::general_result().number;
        let skip = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        let (skip_options, if_bit) = self.emit_condition_test(if_condition)?;
        if returns_pointer {
            // Return the searched pointer (already in r3) via `bclr` when TRUE — invert
            // emit_condition_test's SKIP branch; the chase falls through after.
            let return_options = if skip_options == 4 { 12 } else { 4 };
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: return_options, condition_bit: if_bit });
            self.evaluate_general(chase_value, loop_register)?;
        } else {
            // Skip the found arm to the chase when FALSE (the emit_condition_test SKIP
            // branch used directly), else materialize the flag and return.
            let to_chase = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options: skip_options, condition_bit: if_bit, target: 0 });
            self.evaluate_tail(return_value, function.return_type, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let chase_at = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[to_chase] {
                *target = chase_at;
            }
            self.evaluate_general(chase_value, loop_register)?;
        }
        let condition_at = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_at;
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let back = if options == 4 { 12 } else { 4 };
        self.output.instructions.push(Instruction::BranchConditionalForward { options: back, condition_bit, target: body_top });
        self.evaluate_tail(default_return, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A `void` function whose body is a counting `for (i = 0; i < bound; i++)`
    /// loop with a parameter bound: mwcc puts the counter in r31 (callee-saved,
    /// initialised to 0) and the bound in r30, branches to the test, and runs
    /// `BODY: <body>; addi r31,r31,1; cmpw r31,r30; blt BODY`. The body may use the
    /// counter (passed as a call argument). Returns whether this path applied.
    pub(crate) fn try_for_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !self.frame_slots.is_empty() || function.return_type != Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::For, initializer: Some(init), condition: Some(condition), step: Some(step), body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = 0`.
        let counter = match init {
            Expression::Assign { target, value } if matches!(value.as_ref(), Expression::IntegerLiteral(0)) => {
                match target.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i < bound`.
        let bound = match condition {
            Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if *name == counter) =>
            {
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
        if !function.parameters.iter().any(|parameter| parameter.name == bound) {
            return Ok(false);
        }
        // The body must be straight-line calls referencing no register value other
        // than the counter (the bound, and any other parameter, would each need
        // their own callee-saved register — deferred).
        if body.iter().any(|statement| !matches!(statement, Statement::Expression(_))) {
            return Ok(false);
        }
        let reads_other_parameter = body.iter().any(|statement| match statement {
            Statement::Expression(expression) => function
                .parameters
                .iter()
                .any(|parameter| parameter.name != counter && expression_reads_name(expression, &parameter.name)),
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
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: COUNTER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: COUNTER, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: BOUND, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::Or { a: BOUND, s: bound_incoming, b: bound_incoming });
        let signed = self.signed_of(local.declared_type);
        self.locations.insert(
            counter.clone(),
            Location { class: ValueClass::General, register: COUNTER, signed, width: 32, pointee: None, stride: None },
        );
        if let Some(location) = self.locations.get_mut(&bound) {
            location.register = BOUND;
        }

        // Branch to the test; the body falls into the step, then the compare loops.
        let skip = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        self.output.instructions.push(Instruction::AddImmediate { d: COUNTER, a: COUNTER, immediate: 1 });
        let condition_index = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_index;
        }
        self.output.instructions.push(Instruction::CompareWord { a: COUNTER, b: BOUND });
        // `blt body_top` (BO=12 branch-if-true, BI=0 the LT bit).
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: body_top });

        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: COUNTER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: BOUND, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A straight-line non-leaf function whose parameters live across its call(s):
    /// mwcc copies each into a callee-saved register at entry (saved/reloaded around
    /// the frame) so it survives the calls. The registers are assigned by parameter
    /// order — the LAST live parameter gets r31, the next r30, and so on — and the
    /// body/return then read the values from those registers. Returns whether it
    /// applied. (Locals, floats, and values passed to a call still defer.)
    /// The PIPELINED COPY (fire 417, the strcpy idiom): `char *p = dst;
    /// while ((*p++ = *src++)) ;` — the assignment IS the condition, so
    /// there is no separate test block. Measured: mr alias; LOOP: lbz
    /// carry,0(src); addi src,1; extsb. (the test); stb carry,0(p);
    /// addi p,1; bne LOOP; blr — the alias takes params_top+2 (r6) and
    /// the carried char params_top+1 (r5); dst rides r3 to the return.
    pub(crate) fn try_pipelined_copy(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Pointer(Pointee::Char)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [dst_param, src_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if dst_param.parameter_type != Type::Pointer(Pointee::Char)
            || src_param.parameter_type != Type::Pointer(Pointee::Char)
        {
            return Ok(false);
        }
        let dst = dst_param.name.as_str();
        let source = src_param.name.as_str();
        let [alias_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if alias_local.declared_type != Type::Pointer(Pointee::Char)
            || !matches!(&alias_local.initializer, Some(Expression::Variable(v)) if v == dst)
        {
            return Ok(false);
        }
        let alias = alias_local.name.as_str();
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !body.is_empty() {
            return Ok(false);
        }
        // The condition: *p++ = *src++ (both POSTFIX — the old pointers).
        let post_deref = |expression: &Expression| -> Option<String> {
            let Expression::Dereference { pointer } = expression else { return None };
            let Expression::PostStep { target, operator: BinaryOperator::Add } = pointer.as_ref()
            else {
                return None;
            };
            let Expression::Variable(name) = target.as_ref() else { return None };
            Some(name.clone())
        };
        let Expression::Assign { target, value } = condition else {
            return Ok(false);
        };
        if post_deref(target).as_deref() != Some(alias) || post_deref(value).as_deref() != Some(source)
        {
            return Ok(false);
        }
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == dst) {
            return Ok(false);
        }
        let Some(dst_register) = self.lookup_general(dst) else {
            return Ok(false);
        };
        let Some(src_register) = self.lookup_general(source) else {
            return Ok(false);
        };
        let top = dst_register.max(src_register);
        let carry = top + 1;
        let alias_register = top + 2;
        // -- emit --
        self.output.instructions.push(Instruction::move_register(alias_register, dst_register));
        let loop_at = self.fresh_label();
        self.bind_label(loop_at);
        self.output.instructions.push(Instruction::LoadByteZero { d: carry, a: src_register, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: src_register, a: src_register, immediate: 1 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: carry });
        self.output.instructions.push(Instruction::StoreByte { s: carry, a: alias_register, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: alias_register, a: alias_register, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, loop_at); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured after implementation (objprobe) — placeholder 0.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The CTR LOOP (fire 419, e_fmod's `while(n--)` walker): a counted
    /// loop whose body BRANCHES escapes the ×8 unroll entirely — mwcc
    /// emits `mtctr n; cmpwi n,0; beq(lr); BODY; bdnz BODY`. The skip
    /// branch mirrors the entry test exactly: `while(n--)` skips only on
    /// n==0 (a negative n runs 2^32 times, and the unsigned CTR does
    /// too — faithful). Captured micro-shape: `hz = hx - K` fuses into
    /// `addic. r0` (the condition-only computed rides r0 through the
    /// arm), the diamond writes the param home directly in both arms,
    /// and post-loop code takes `beq END` instead of `beqlr`. The
    /// `for(i<n)` variant if-converts its diamond differently (eager
    /// else + `mr` join) and is NOT claimed here; straight-line bodies
    /// take the ×8 unroll machinery (deferred, the counted gate).
    pub(crate) fn try_ctr_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // The condition: a bare `n--` of an int parameter.
        let Expression::PostStep { target, operator: BinaryOperator::Subtract } = condition else {
            return Ok(false);
        };
        let Expression::Variable(count) = target.as_ref() else {
            return Ok(false);
        };
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *count && parameter.parameter_type == Type::Int)
        {
            return Ok(false);
        }
        // The body: `hz = hx - K; if (hz < 0) hx = hx + hx; else hx = hz + hz;`
        let [Statement::Assign { name: hz, value: hz_value }, Statement::If { condition: test, then_body, else_body }] =
            body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = hz_value else {
            return Ok(false);
        };
        let Expression::Variable(hx) = left.as_ref() else {
            return Ok(false);
        };
        // The head: `hx - K` folds into `addic. r0` (fire 419); `hx - hy`
        // (hy an int parameter) into `subf. r0, hy, hx` (fire 420).
        enum Head {
            Immediate(i16),
            Register(String),
        }
        let head = match right.as_ref() {
            Expression::IntegerLiteral(k) => {
                let Ok(negated_k) = i16::try_from(-*k) else {
                    return Ok(false);
                };
                Head::Immediate(negated_k)
            }
            Expression::Variable(hy) if hy != hx && hy != hz && hy != count => {
                if !function
                    .parameters
                    .iter()
                    .any(|parameter| parameter.name == *hy && parameter.parameter_type == Type::Int)
                {
                    return Ok(false);
                }
                Head::Register(hy.clone())
            }
            _ => return Ok(false),
        };
        if hx == hz || hx == count || hz == count {
            return Ok(false);
        }
        // hx: an int parameter sitting in r3 (every capture); hz: an int
        // local or a dead-on-entry int parameter (its home is never
        // touched — the value rides r0).
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *hx && parameter.parameter_type == Type::Int)
        {
            return Ok(false);
        }
        let hz_is_local = function
            .locals
            .iter()
            .any(|local| local.name == *hz && local.declared_type == Type::Int && local.initializer.is_none());
        let hz_is_dead_parameter = function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *hz && parameter.parameter_type == Type::Int);
        if !hz_is_local && !hz_is_dead_parameter {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Less, left: test_left, right: test_right } = test
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(test_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        let doubles_into = |statement: &Statement, target: &str, doubled: &str| -> bool {
            let Statement::Assign { name, value } = statement else {
                return false;
            };
            if name != target {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left, right } = value else {
                return false;
            };
            matches!(left.as_ref(), Expression::Variable(v) if v == doubled)
                && matches!(right.as_ref(), Expression::Variable(v) if v == doubled)
        };
        // The then arm: `hx = hx + hx;` (double, fire 419) or the PAIR
        // CARRY STEP `hx = hx + hx + (lx >> 31); lx = lx + lx;` (fire
        // 420, e_fmod's 2-word left shift — the srwi leads, the LOW
        // doubling schedules between it and the two adds, which
        // associate hx + (hx + carry)). The low word must be UNSIGNED
        // (a signed one would srawi).
        enum ThenArm {
            Double,
            PairStep(String),
        }
        let then_arm = match then_body.as_slice() {
            [single] if doubles_into(single, hx, hx) => ThenArm::Double,
            [Statement::Assign { name: high_name, value: high_value }, low_step] => {
                if high_name != hx {
                    return Ok(false);
                }
                let Expression::Binary { operator: BinaryOperator::Add, left: sum, right: carry } =
                    high_value
                else {
                    return Ok(false);
                };
                let Expression::Binary { operator: BinaryOperator::Add, left: first, right: second } =
                    sum.as_ref()
                else {
                    return Ok(false);
                };
                if !matches!(first.as_ref(), Expression::Variable(v) if v == hx)
                    || !matches!(second.as_ref(), Expression::Variable(v) if v == hx)
                {
                    return Ok(false);
                }
                let Expression::Binary { operator: BinaryOperator::ShiftRight, left: low, right: amount } =
                    carry.as_ref()
                else {
                    return Ok(false);
                };
                let Expression::Variable(lx) = low.as_ref() else {
                    return Ok(false);
                };
                if !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
                    || lx == hx
                    || lx == hz
                    || lx == count
                    || matches!(&head, Head::Register(hy) if lx == hy)
                    || !doubles_into(low_step, lx, lx)
                    || !function
                        .parameters
                        .iter()
                        .any(|parameter| parameter.name == *lx && parameter.parameter_type == Type::UnsignedInt)
                {
                    return Ok(false);
                }
                ThenArm::PairStep(lx.clone())
            }
            _ => return Ok(false),
        };
        let [else_single] = else_body.as_slice() else {
            return Ok(false);
        };
        if !doubles_into(else_single, hx, hz) {
            return Ok(false);
        }
        // The tail: `return hx` (skip = beqlr) or `return hx + K2` (skip =
        // beq END). Both captured with hx in r3.
        enum Tail {
            Home,
            AddImmediate(i16),
        }
        let tail = match &function.return_expression {
            Some(Expression::Variable(v)) if v == hx => Tail::Home,
            Some(Expression::Binary { operator: BinaryOperator::Add, left, right })
                if matches!(left.as_ref(), Expression::Variable(v) if v == hx) =>
            {
                let Expression::IntegerLiteral(k2) = right.as_ref() else {
                    return Ok(false);
                };
                let Ok(k2) = i16::try_from(*k2) else {
                    return Ok(false);
                };
                Tail::AddImmediate(k2)
            }
            _ => return Ok(false),
        };
        let Some(hx_register) = self.lookup_general(hx) else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        let Some(count_register) = self.lookup_general(count) else {
            return Ok(false);
        };
        let head_register = match &head {
            Head::Immediate(_) => None,
            Head::Register(hy) => match self.lookup_general(hy) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
        };
        let pair_low_register = match &then_arm {
            ThenArm::Double => None,
            ThenArm::PairStep(lx) => match self.lookup_general(lx) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
        };
        // -- emit --
        self.output.instructions.push(Instruction::MoveToCountRegister { s: count_register });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: count_register, immediate: 0 });
        let end_label = self.fresh_label();
        match tail {
            Tail::Home => self
                .output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 }),
            Tail::AddImmediate(_) => self.emit_branch_conditional_to(12, 2, end_label),
        }
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        match &head {
            Head::Immediate(negated_k) => self.output.instructions.push(Instruction::AddImmediateCarryingRecord {
                d: 0,
                a: hx_register,
                immediate: *negated_k,
            }),
            Head::Register(_) => self.output.instructions.push(Instruction::SubtractFromRecord {
                d: 0,
                a: head_register.unwrap(),
                b: hx_register,
            }),
        }
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        match &then_arm {
            ThenArm::Double => {
                self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: hx_register });
            }
            ThenArm::PairStep(_) => {
                let low = pair_low_register.unwrap();
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: low, shift: 31 });
                self.output.instructions.push(Instruction::Add { d: low, a: low, b: low });
                self.output.instructions.push(Instruction::Add { d: 0, a: hx_register, b: 0 });
                self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: 0 });
            }
        }
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        self.output.instructions.push(Instruction::Add { d: hx_register, a: 0, b: 0 });
        self.bind_label(join_label);
        self.emit_branch_conditional_to(16, 0, body_label); // bdnz
        if let Tail::AddImmediate(k2) = tail {
            self.bind_label(end_label);
            self.output.instructions.push(Instruction::AddImmediate { d: 3, a: hx_register, immediate: k2 });
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The CTR PAIR LOOP (fire 421): e_fmod's core `while(n--)` captured
    /// whole — the 2-word compare-subtract-and-shift walk:
    ///   hz = hx - hy; lz = lx - ly; if (lx < ly) hz -= 1;
    ///   if (hz < 0) { hx = hx+hx+(lx>>31); lx = lx+lx; }
    ///   else        { hx = hz+hz+(lz>>31); lx = lz+lz; }
    /// Emission facts (all measured): the borrow `cmplw lx,ly` hoists
    /// ABOVE both subtracts (they fill its latency); hz/lz take the
    /// FREED COUNT HOME and the next register up, via plain `subf` (no
    /// record — the `hz -= 1` borrow decrement sits between def and
    /// test, so the diamond re-tests with an explicit cmpwi); the then
    /// arm is the fire-420 pair step verbatim; the else arm's
    /// intermediates land DIRECTLY in r3 (hx is not a source there) and
    /// `lx = lz + lz` writes lx's home from lz. @N +0.
    pub(crate) fn try_ctr_pair_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        // A SCAFFOLD PREFIX may precede the loop (fire 425): the seam is
        // pure concatenation — scaffold ops emit in source order before
        // the mtctr, a loop-crossing sign local takes the next-free
        // register BEFORE the count home frees, and the loop's internal
        // temps allocate around it (hz keeps the freed count home, lz
        // shifts past the sign). Probed forms only: `param &= LOWMASK`
        // (in-place clrlwi), and the sign-extract pair `sign = param &
        // 0x80000000; param ^= sign` (clrrwi + xor).
        let [scaffold @ .., Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Expression::PostStep { target, operator: BinaryOperator::Subtract } = condition else {
            return Ok(false);
        };
        let Expression::Variable(count) = target.as_ref() else {
            return Ok(false);
        };
        // The exact captured signature: (int hx, unsigned lx, int hy,
        // unsigned ly, int n) with n LAST — the freed-count-home rule for
        // hz/lz is only measured in that layout.
        let [p_hx, p_lx, p_hy, p_ly, p_n] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_hy.parameter_type != Type::Int
            || p_ly.parameter_type != Type::UnsignedInt
            || p_n.parameter_type != Type::Int
            || p_n.name != *count
        {
            return Ok(false);
        }
        let (hx, lx, hy, ly) = (p_hx.name.as_str(), p_lx.name.as_str(), p_hy.name.as_str(), p_ly.name.as_str());
        // Parse the scaffold prefix (probed forms only).
        enum ScaffoldOp {
            MaskParam { name: String, clear: u8 },
            SignExtract { source: String },
            XorParam { name: String },
        }
        let is_int_parameter = |name: &str| {
            function
                .parameters
                .iter()
                .any(|parameter| parameter.name == name && matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt))
        };
        let mut scaffold_ops: Vec<ScaffoldOp> = Vec::new();
        let mut sign_local: Option<&str> = None;
        for statement in scaffold {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            let Expression::Binary { operator, left, right } = value else {
                return Ok(false);
            };
            match operator {
                // param &= (1<<n)-1  →  clrlwi param, param, 32-n (in place).
                BinaryOperator::BitAnd
                    if is_int_parameter(name)
                        && matches!(left.as_ref(), Expression::Variable(v) if v == name) =>
                {
                    let Expression::IntegerLiteral(mask) = right.as_ref() else {
                        return Ok(false);
                    };
                    let mask = *mask as u32;
                    if mask == 0 || !(mask as u64 + 1).is_power_of_two() {
                        return Ok(false);
                    }
                    let clear = mask.leading_zeros() as u8;
                    scaffold_ops.push(ScaffoldOp::MaskParam { name: name.clone(), clear });
                }
                // sign = param & 0x80000000  →  clrrwi sign, param, 31.
                BinaryOperator::BitAnd if !is_int_parameter(name) && sign_local.is_none() => {
                    let Expression::Variable(source) = left.as_ref() else {
                        return Ok(false);
                    };
                    if !is_int_parameter(source)
                        || !matches!(right.as_ref(), Expression::IntegerLiteral(m) if *m as u32 == 0x8000_0000)
                        || !function
                            .locals
                            .iter()
                            .any(|local| local.name == *name && local.declared_type == Type::Int && local.initializer.is_none())
                    {
                        return Ok(false);
                    }
                    sign_local = Some(name.as_str());
                    scaffold_ops.push(ScaffoldOp::SignExtract { source: source.clone() });
                }
                // param ^= sign  →  xor param, param, sign (in place).
                BinaryOperator::BitXor
                    if is_int_parameter(name)
                        && matches!(left.as_ref(), Expression::Variable(v) if v == name) =>
                {
                    if !matches!(right.as_ref(), Expression::Variable(v) if Some(v.as_str()) == sign_local) {
                        return Ok(false);
                    }
                    scaffold_ops.push(ScaffoldOp::XorParam { name: name.clone() });
                }
                _ => return Ok(false),
            }
        }
        // Body: [hz = hx - hy][lz = lx - ly][if (lx < ly) hz -= 1][diamond].
        let [Statement::Assign { name: hz, value: hz_value }, Statement::Assign { name: lz, value: lz_value }, Statement::If { condition: borrow_test, then_body: borrow_then, else_body: borrow_else }, Statement::If { condition: test, then_body, else_body }] =
            body.as_slice()
        else {
            return Ok(false);
        };
        let subtracts = |value: &Expression, from: &str, taken: &str| -> bool {
            let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = value else {
                return false;
            };
            matches!(left.as_ref(), Expression::Variable(v) if v == from)
                && matches!(right.as_ref(), Expression::Variable(v) if v == taken)
        };
        if !subtracts(hz_value, hx, hy) || !subtracts(lz_value, lx, ly) {
            return Ok(false);
        }
        let names_distinct = {
            let mut names = [hx, lx, hy, ly, count.as_str(), hz.as_str(), lz.as_str()];
            names.sort_unstable();
            names.windows(2).all(|pair| pair[0] != pair[1])
        };
        if !names_distinct {
            return Ok(false);
        }
        if let Some(sign) = sign_local {
            if [hx, lx, hy, ly, count.as_str(), hz.as_str(), lz.as_str()].contains(&sign) {
                return Ok(false);
            }
        }
        let is_free_local = |name: &str, declared: Type| {
            function
                .locals
                .iter()
                .any(|local| local.name == name && local.declared_type == declared && local.initializer.is_none())
        };
        if !is_free_local(hz, Type::Int) || !is_free_local(lz, Type::UnsignedInt) {
            return Ok(false);
        }
        // The borrow: if (lx < ly) hz -= 1; (unsigned compare, no else).
        let Expression::Binary { operator: BinaryOperator::Less, left: borrow_left, right: borrow_right } =
            borrow_test
        else {
            return Ok(false);
        };
        if !matches!(borrow_left.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(borrow_right.as_ref(), Expression::Variable(v) if v == ly)
            || !borrow_else.is_empty()
        {
            return Ok(false);
        }
        let [Statement::Assign { name: decremented, value: decrement }] = borrow_then.as_slice() else {
            return Ok(false);
        };
        if decremented != hz {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: dec_left, right: dec_right } =
            decrement
        else {
            return Ok(false);
        };
        if !matches!(dec_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(dec_right.as_ref(), Expression::IntegerLiteral(1))
        {
            return Ok(false);
        }
        // The diamond: if (hz < 0) {pair step from lx} else {pair step from hz/lz into hx/lx}.
        let Expression::Binary { operator: BinaryOperator::Less, left: test_left, right: test_right } = test
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(test_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        // An arm: high_target = high+high+(low>>31); lx = low+low;
        let pair_step = |statements: &[Statement], high: &str, low: &str| -> bool {
            let [Statement::Assign { name: high_name, value: high_value }, Statement::Assign { name: low_name, value: low_value }] =
                statements
            else {
                return false;
            };
            if high_name != hx || low_name != lx {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left: sum, right: carry } = high_value
            else {
                return false;
            };
            let Expression::Binary { operator: BinaryOperator::Add, left: first, right: second } = sum.as_ref()
            else {
                return false;
            };
            if !matches!(first.as_ref(), Expression::Variable(v) if v == high)
                || !matches!(second.as_ref(), Expression::Variable(v) if v == high)
            {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shifted, right: amount } =
                carry.as_ref()
            else {
                return false;
            };
            if !matches!(shifted.as_ref(), Expression::Variable(v) if v == low)
                || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
            {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left: low_first, right: low_second } =
                low_value
            else {
                return false;
            };
            matches!(low_first.as_ref(), Expression::Variable(v) if v == low)
                && matches!(low_second.as_ref(), Expression::Variable(v) if v == low)
        };
        if !pair_step(then_body, hx, lx) {
            return Ok(false);
        }
        // The else arm may LEAD with the zero exit `if ((hz | lz) == 0)
        // return K;` — emitted INLINE as `or. r0,hz,lz; bne CONT; li r3,K;
        // blr` (a bare mid-loop return, no exit label; fire 422).
        enum ExitValue {
            Immediate(i16),
            Sign,
        }
        let (early_return, else_step) = match else_body.as_slice() {
            [Statement::If { condition: exit_test, then_body: exit_then, else_body: exit_else }, rest @ ..] => {
                let Expression::Binary { operator: BinaryOperator::Equal, left: or_side, right: zero_side } =
                    exit_test
                else {
                    return Ok(false);
                };
                let Expression::Binary { operator: BinaryOperator::BitOr, left: or_left, right: or_right } =
                    or_side.as_ref()
                else {
                    return Ok(false);
                };
                if !matches!(or_left.as_ref(), Expression::Variable(v) if v == hz)
                    || !matches!(or_right.as_ref(), Expression::Variable(v) if v == lz)
                    || !matches!(zero_side.as_ref(), Expression::IntegerLiteral(0))
                    || !exit_else.is_empty()
                {
                    return Ok(false);
                }
                let exit_value = match exit_then.as_slice() {
                    [Statement::Return(Some(Expression::IntegerLiteral(returned)))] => {
                        let Ok(returned) = i16::try_from(*returned) else {
                            return Ok(false);
                        };
                        ExitValue::Immediate(returned)
                    }
                    [Statement::Return(Some(Expression::Variable(v)))] if Some(v.as_str()) == sign_local => {
                        ExitValue::Sign
                    }
                    _ => return Ok(false),
                };
                (Some(exit_value), rest)
            }
            _ => (None, else_body.as_slice()),
        };
        if !pair_step(else_step, hz, lz) {
            return Ok(false);
        }
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == hx) {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(hy_register), Some(ly_register), Some(count_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(hy),
            self.lookup_general(ly),
            self.lookup_general(count),
        ) else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // The sign local takes the next-free register BEFORE the count home
        // frees; hz keeps the freed count home; lz shifts past the sign.
        let sign_register = if sign_local.is_some() { Some(3 + function.parameters.len() as u8) } else { None };
        let hz_register = count_register;
        let lz_register = 3 + function.parameters.len() as u8 + if sign_local.is_some() { 1 } else { 0 };
        if lz_register > 10 {
            return Ok(false);
        }
        // Resolve every scaffold register before any emission.
        enum ResolvedScaffold {
            Mask { register: u8, clear: u8 },
            Extract { source_register: u8 },
            Xor { register: u8 },
        }
        let mut resolved_scaffold = Vec::new();
        for op in &scaffold_ops {
            match op {
                ScaffoldOp::MaskParam { name, clear } => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Mask { register, clear: *clear });
                }
                ScaffoldOp::SignExtract { source } => {
                    let Some(source_register) = self.lookup_general(source) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Extract { source_register });
                }
                ScaffoldOp::XorParam { name } => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Xor { register });
                }
            }
        }
        // -- emit --
        for op in &resolved_scaffold {
            match op {
                ResolvedScaffold::Mask { register, clear } => self
                    .output
                    .instructions
                    .push(Instruction::AndContiguousMask { a: *register, s: *register, begin: *clear, end: 31 }),
                ResolvedScaffold::Extract { source_register } => self.output.instructions.push(
                    Instruction::AndContiguousMask { a: sign_register.unwrap(), s: *source_register, begin: 0, end: 0 },
                ),
                ResolvedScaffold::Xor { register } => self
                    .output
                    .instructions
                    .push(Instruction::Xor { a: *register, s: *register, b: sign_register.unwrap() }),
            }
        }
        self.output.instructions.push(Instruction::MoveToCountRegister { s: count_register });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: count_register, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: lx_register, b: ly_register });
        self.output.instructions.push(Instruction::SubtractFrom { d: hz_register, a: hy_register, b: hx_register });
        self.output.instructions.push(Instruction::SubtractFrom { d: lz_register, a: ly_register, b: lx_register });
        let no_borrow_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, no_borrow_label); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: hz_register, a: hz_register, immediate: -1 });
        self.bind_label(no_borrow_label);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: hz_register, immediate: 0 });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: lx_register, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: lx_register, a: lx_register, b: lx_register });
        self.output.instructions.push(Instruction::Add { d: 0, a: hx_register, b: 0 });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: 0 });
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        if let Some(exit_value) = &early_return {
            self.output.instructions.push(Instruction::OrRecord { a: 0, s: hz_register, b: lz_register });
            let continue_label = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, continue_label); // bne
            match exit_value {
                ExitValue::Immediate(returned) => {
                    self.output.instructions.push(Instruction::load_immediate(3, *returned));
                }
                ExitValue::Sign => {
                    self.output.instructions.push(Instruction::move_register(3, sign_register.unwrap()));
                }
            }
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            self.bind_label(continue_label);
        }
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: lz_register, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: lx_register, a: lz_register, b: lz_register });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hz_register, b: 0 });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hz_register, b: hx_register });
        self.bind_label(join_label);
        self.emit_branch_conditional_to(16, 0, body_label); // bdnz
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }










    /// The NORMALIZE LOOP (fire 424, e_fmod's tail loop): a NON-counted
    /// `while (hx < BIG) { hx = hx+hx+(lx>>31); lx = lx+lx; iy -= 1; }`
    /// with `return hx + iy` — rotated form with the big bound hoisted
    /// `lis r0, BIG>>16` BEFORE the loop. r0 stays OCCUPIED across the
    /// body, so the carry temp takes the next free register after the
    /// params; the iy decrement schedules INTO the add latency (between
    /// `add rT,hx,rT` and `add hx,hx,rT`). Bound gated to lis-only
    /// constants (low half zero, not a cmpwi immediate).
    pub(crate) fn try_norm_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_iy] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_iy.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (hx, lx, iy) = (p_hx.name.as_str(), p_lx.name.as_str(), p_iy.name.as_str());
        if hx == lx || hx == iy || lx == iy {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // The condition: hx < BIG, BIG a lis-only constant (low half 0).
        let Expression::Binary { operator: BinaryOperator::Less, left: test_left, right: test_right } =
            condition
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(bound) = test_right.as_ref() else {
            return Ok(false);
        };
        let bound = *bound;
        if bound & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(bound_high) = i16::try_from(bound >> 16) else {
            return Ok(false);
        };
        // The body: [hx = hx+hx+(lx>>31)][lx = lx+lx][iy = iy-1].
        let [Statement::Assign { name: high_name, value: high_value }, Statement::Assign { name: low_name, value: low_value }, Statement::Assign { name: dec_name, value: dec_value }] =
            body.as_slice()
        else {
            return Ok(false);
        };
        if high_name != hx || low_name != lx || dec_name != iy {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Add, left: sum, right: carry } = high_value
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Add, left: first, right: second } = sum.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shifted, right: amount } =
            carry.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(first.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(second.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(shifted.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Add, left: low_first, right: low_second } =
            low_value
        else {
            return Ok(false);
        };
        if !matches!(low_first.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(low_second.as_ref(), Expression::Variable(v) if v == lx)
        {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: dec_left, right: dec_right } =
            dec_value
        else {
            return Ok(false);
        };
        if !matches!(dec_left.as_ref(), Expression::Variable(v) if v == iy)
            || !matches!(dec_right.as_ref(), Expression::IntegerLiteral(1))
        {
            return Ok(false);
        }
        // The tail: return hx + iy.
        let Some(Expression::Binary { operator: BinaryOperator::Add, left: ret_left, right: ret_right }) =
            &function.return_expression
        else {
            return Ok(false);
        };
        if !matches!(ret_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(ret_right.as_ref(), Expression::Variable(v) if v == iy)
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(iy_register)) =
            (self.lookup_general(hx), self.lookup_general(lx), self.lookup_general(iy))
        else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // The carry temp: next free past the params (r0 holds the bound).
        let temp = 3 + function.parameters.len() as u8;
        if temp > 10 {
            return Ok(false);
        }
        // -- emit --
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 0, immediate: bound_high });
        let test_label = self.fresh_label();
        self.emit_branch_to(test_label);
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: temp, s: lx_register, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: lx_register, a: lx_register, b: lx_register });
        self.output.instructions.push(Instruction::Add { d: temp, a: hx_register, b: temp });
        self.output.instructions.push(Instruction::AddImmediate { d: iy_register, a: iy_register, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: temp });
        self.bind_label(test_label);
        self.output.instructions.push(Instruction::CompareWord { a: hx_register, b: 0 });
        self.emit_branch_conditional_to(12, 0, body_label); // blt
        self.output.instructions.push(Instruction::Add { d: 3, a: hx_register, b: iy_register });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ROTATED LOOP (fire 413, the e_fmod ilogb family): mwcc emits
    /// non-counted loops as `init; b TEST; BODY: [step][body]; TEST:
    /// cond; b<positive> BODY; [mr]` with NO unrolling (counted loops
    /// take the ctr/unroll machinery instead — deferred). Registers per
    /// the captures: params in place; a condition-only computed value
    /// takes r0 (even across the backward branch); the returned local
    /// takes a param home freed during init, else the next free.
    pub(crate) fn try_rotated_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if !matches!(function.return_type, Type::Int | Type::Void)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer, condition: Some(condition), step, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if function.return_type != Type::Void && !matches!(&function.return_expression, Some(Expression::Variable(_))) {
            return Ok(false);
        }
        // Locals: uninitialized (bound via the comma init), or initialized
        // with a small constant (an init-plan entry).
        let mut local_constant_inits: Vec<(&str, i16)> = Vec::new();
        for local in &function.locals {
            if local.declared_type != Type::Int || local.array_length.is_some() {
                return Ok(false);
            }
            if let Some(init) = &local.initializer {
                let Some(constant) =
                    crate::analysis::constant_value(init).and_then(|k| i16::try_from(k).ok())
                else {
                    return Ok(false);
                };
                local_constant_inits.push((local.name.as_str(), constant));
            }
        }
        // Homes: params in their registers.
        let mut homes: Vec<(String, u8)> = Vec::new();
        let mut char_pointers: Vec<String> = Vec::new();
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::Int => {}
                Type::Pointer(Pointee::Char) => char_pointers.push(parameter.name.clone()),
                _ => return Ok(false),
            }
            let Some(register) = self.lookup_general(&parameter.name) else {
                return Ok(false);
            };
            homes.push((parameter.name.clone(), register));
        }
        let home_of = |homes: &[(String, u8)], name: &str| {
            homes.iter().find(|(n, _)| n == name).map(|&(_, r)| r)
        };
        // The init list: `a = C`, `a = param`, or `a = param << K` — a
        // param read by its LAST init use frees its home.
        enum Init {
            Constant { register: u8, value: i16 },
            ShiftOfParam { register: u8, source: u8, amount: u8 },
        }
        let mut init_plan: Vec<Init> = Vec::new();
        for (name, constant) in &local_constant_inits {
            let top = homes.iter().map(|&(_, r)| r).filter(|&r| r != 0).max().unwrap_or(2);
            let register = top + 1;
            init_plan.push(Init::Constant { register, value: *constant });
            homes.push((name.to_string(), register));
        }
        if let Some(init) = initializer {
            // Flatten the comma list.
            let mut elements: Vec<&Expression> = Vec::new();
            let mut cursor = init;
            loop {
                match cursor {
                    Expression::Comma { left, right } => {
                        elements.push(right.as_ref());
                        cursor = left.as_ref();
                    }
                    other => {
                        elements.push(other);
                        break;
                    }
                }
            }
            elements.reverse();
            // First pass: aliases (i = param) rename in place; param-reads
            // mark freed homes.
            let mut freed: Vec<u8> = Vec::new();
            let mut pending: Vec<(&str, &Expression)> = Vec::new();
            for element in &elements {
                let Expression::Assign { target, value } = element else {
                    return Ok(false);
                };
                let Expression::Variable(name) = target.as_ref() else {
                    return Ok(false);
                };
                match value.as_ref() {
                    Expression::Variable(source) => {
                        // An alias: the local IS the param, renamed.
                        let Some(register) = home_of(&homes, source) else {
                            return Ok(false);
                        };
                        homes.push((name.clone(), register));
                    }
                    Expression::Binary { operator: BinaryOperator::ShiftLeft, left, right } => {
                        let Expression::Variable(source) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(source_register) = home_of(&homes, source) else {
                            return Ok(false);
                        };
                        let Some(amount) =
                            crate::analysis::constant_value(right).and_then(|k| u8::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        // A condition-only computed value lives in r0.
                        init_plan.push(Init::ShiftOfParam {
                            register: 0,
                            source: source_register,
                            amount,
                        });
                        homes.push((name.clone(), 0));
                        freed.push(source_register);
                    }
                    other if crate::analysis::constant_value(other).is_some() => {
                        pending.push((name.as_str(), other));
                    }
                    _ => return Ok(false),
                }
            }
            // Second pass: constants take a freed param home, else the next
            // free register after the params.
            for (name, value) in pending {
                let constant = crate::analysis::constant_value(value).expect("checked");
                let Ok(small) = i16::try_from(constant) else {
                    return Ok(false);
                };
                let register = if let Some(register) = freed.pop() {
                    register
                } else {
                    let top = homes.iter().map(|&(_, r)| r).filter(|&r| r != 0).max().unwrap_or(2);
                    top + 1
                };
                init_plan.push(Init::Constant { register, value: small });
                homes.push((name.to_string(), register));
            }
        }
        // The condition: var OP const, var OP var, or the char-walk
        // truthiness `*p` (lbz + extsb. record test).
        enum LoopTest {
            Constant { register: u8, constant: i64, big: bool },
            Register { left: u8, right: u8 },
            CharLoad { pointer: u8 },
        }
        let (loop_test, back_branch) = match condition {
            Expression::Binary { operator: cond_op, left: cond_left, right: cond_right } => {
                let Expression::Variable(cond_var) = cond_left.as_ref() else {
                    return Ok(false);
                };
                let Some(cond_register) = home_of(&homes, cond_var) else {
                    return Ok(false);
                };
                let back = match cond_op {
                    BinaryOperator::Greater => (12u8, 1u8), // bgt
                    BinaryOperator::Less => (12u8, 0u8),    // blt
                    _ => return Ok(false),
                };
                if let Some(constant) = crate::analysis::constant_value(cond_right) {
                    let big = i16::try_from(constant).is_err();
                    if big && constant & 0xffff != 0 {
                        return Ok(false); // lis-only bounds measured
                    }
                    (LoopTest::Constant { register: cond_register, constant, big }, back)
                } else if let Expression::Variable(right_var) = cond_right.as_ref() {
                    let Some(right_register) = home_of(&homes, right_var) else {
                        return Ok(false);
                    };
                    (LoopTest::Register { left: cond_register, right: right_register }, back)
                } else {
                    return Ok(false);
                }
            }
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else {
                    return Ok(false);
                };
                if !char_pointers.iter().any(|p| p == name) {
                    return Ok(false);
                }
                let Some(register) = home_of(&homes, name) else {
                    return Ok(false);
                };
                (LoopTest::CharLoad { pointer: register }, (4u8, 2u8)) // bne
            }
            _ => return Ok(false),
        };
        let hoists_big_bound = matches!(&loop_test, LoopTest::Constant { big: true, .. });
        // Body + step ops: compound self-ops on homed locals.
        enum LoopOp {
            AddImmediate { register: u8, value: i16 },
            SelfAdd { register: u8 },
            ShiftLeft { register: u8, amount: u8 },
            /// `*dst = *src` where src is the walk's condition pointer —
            /// the char loaded by the TEST carries across the back edge
            /// into this store (measured S2).
            CarriedStore { destination: u8 },
        }
        let condition_pointer: Option<&str> = match condition {
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        };
        let parse_op = |homes: &[(String, u8)], statement: &Statement| -> Option<LoopOp> {
            if let Statement::Store { target, value } = statement {
                // `*dst = *src` with src the condition pointer.
                let Expression::Dereference { pointer: dst } = target else { return None };
                let Expression::Variable(dst_name) = dst.as_ref() else { return None };
                let destination = home_of(homes, dst_name)?;
                let Expression::Dereference { pointer: src } = value else { return None };
                let Expression::Variable(src_name) = src.as_ref() else { return None };
                if condition_pointer != Some(src_name.as_str()) {
                    return None;
                }
                return Some(LoopOp::CarriedStore { destination });
            }
            let Statement::Assign { name, value } = statement else { return None };
            let register = home_of(homes, name)?;
            let Expression::Binary { operator, left, right } = value else { return None };
            if !matches!(left.as_ref(), Expression::Variable(v) if v == name) {
                return None;
            }
            match operator {
                BinaryOperator::Add if matches!(right.as_ref(), Expression::Variable(v) if v == name) => {
                    Some(LoopOp::SelfAdd { register })
                }
                BinaryOperator::Add => {
                    let value = i16::try_from(crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::AddImmediate { register, value })
                }
                BinaryOperator::Subtract => {
                    let value = i16::try_from(-crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::AddImmediate { register, value })
                }
                BinaryOperator::ShiftLeft => {
                    let amount = u8::try_from(crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::ShiftLeft { register, amount })
                }
                _ => None,
            }
        };
        // Loop ops in emission order: the STEP first (it feeds the
        // condition — measured), then the body statements in source order.
        let mut loop_ops: Vec<LoopOp> = Vec::new();
        match kind {
            LoopKind::For => {
                let Some(step) = step else { return Ok(false) };
                let step_statement = Statement::Assign {
                    name: match step {
                        Expression::Assign { target, .. } => match target.as_ref() {
                            Expression::Variable(name) => name.clone(),
                            _ => return Ok(false),
                        },
                        _ => return Ok(false),
                    },
                    value: match step {
                        Expression::Assign { value, .. } => value.as_ref().clone(),
                        _ => return Ok(false),
                    },
                };
                let Some(op) = parse_op(&homes, &step_statement) else {
                    return Ok(false);
                };
                loop_ops.push(op);
            }
            LoopKind::While => {
                if step.is_some() {
                    return Ok(false);
                }
            }
            LoopKind::DoWhile => {
                if step.is_some() {
                    return Ok(false);
                }
            }
        }
        for statement in body {
            let Some(op) = parse_op(&homes, statement) else {
                return Ok(false);
            };
            loop_ops.push(op);
        }
        if loop_ops.is_empty() {
            return Ok(false);
        }
        // COUNTED loops (the condition variable stepped by a constant in a
        // For/While) take mwcc's unroll machinery — claiming them rotated
        // would be WRONG BYTES. Only the do-while keeps constant steps
        // (measured D1: no unroll).
        if !matches!(kind, LoopKind::DoWhile) {
            let condition_variable_register = match &loop_test {
                LoopTest::Constant { register, .. } => Some(*register),
                LoopTest::Register { left, .. } => Some(*left),
                LoopTest::CharLoad { .. } => None,
            };
            if let Some(register) = condition_variable_register {
                let stepped_by_constant = loop_ops.iter().any(|op| {
                    matches!(op, LoopOp::AddImmediate { register: stepped, .. } if *stepped == register)
                });
                if stepped_by_constant {
                    return Ok(false);
                }
            }
        }
        let has_carried_store = loop_ops.iter().any(|op| matches!(op, LoopOp::CarriedStore { .. }));
        // The carried char takes the next free register (S2: r5).
        let carry_register = if has_carried_store {
            let top = homes.iter().map(|&(_, r)| r).filter(|&r| r != 0).max().unwrap_or(2);
            top + 1
        } else {
            0
        };
        let return_register = if function.return_type == Type::Void {
            3 // no move needed
        } else {
            let Some(Expression::Variable(returned)) = &function.return_expression else {
                return Ok(false);
            };
            let Some(register) = home_of(&homes, returned) else {
                return Ok(false);
            };
            register
        };
        // -- emit --
        // Init: param-reading shifts first, then constants (the freed-home
        // order); a big bound hoists to r0 before the loop.
        for init in &init_plan {
            match init {
                Init::ShiftOfParam { register, source, amount } => {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate {
                        a: *register,
                        s: *source,
                        shift: *amount,
                    });
                }
                Init::Constant { register, value } => {
                    self.output.instructions.push(Instruction::load_immediate(*register, *value));
                }
            }
        }
        if let LoopTest::Constant { constant, big: true, .. } = &loop_test {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(0, (constant >> 16) as i16));
        }
        let _ = hoists_big_bound;
        let body_at = self.fresh_label();
        let test_at = self.fresh_label();
        if !matches!(kind, LoopKind::DoWhile) {
            // The rotated entry; a do-while falls straight into its body.
            self.emit_branch_to(test_at);
        }
        self.bind_label(body_at);
        for op in &loop_ops {
            match op {
                LoopOp::AddImmediate { register, value } => {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: *register,
                        a: *register,
                        immediate: *value,
                    });
                }
                LoopOp::SelfAdd { register } => {
                    self.output.instructions.push(Instruction::Add {
                        d: *register,
                        a: *register,
                        b: *register,
                    });
                }
                LoopOp::ShiftLeft { register, amount } => {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate {
                        a: *register,
                        s: *register,
                        shift: *amount,
                    });
                }
                LoopOp::CarriedStore { destination } => {
                    self.output.instructions.push(Instruction::StoreByte {
                        s: carry_register,
                        a: *destination,
                        offset: 0,
                    });
                }
            }
        }
        self.bind_label(test_at);
        match &loop_test {
            LoopTest::Constant { register, constant, big: true } => {
                let _ = constant;
                self.output.instructions.push(Instruction::CompareWord { a: *register, b: 0 });
            }
            LoopTest::Constant { register, constant, big: false } => {
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: *register,
                    immediate: *constant as i16,
                });
            }
            LoopTest::Register { left, right } => {
                self.output.instructions.push(Instruction::CompareWord { a: *left, b: *right });
            }
            LoopTest::CharLoad { pointer } => {
                // A carried store loads into its carry register; a bare
                // walk uses r0.
                let target = if has_carried_store { carry_register } else { 0 };
                self.output.instructions.push(Instruction::LoadByteZero {
                    d: target,
                    a: *pointer,
                    offset: 0,
                });
                self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: target });
            }
        }
        self.emit_branch_conditional_to(back_branch.0, back_branch.1, body_at);
        if function.return_type != Type::Void && return_register != 3 {
            self.output.instructions.push(Instruction::move_register(3, return_register));
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured after implementation (objprobe) — placeholder 0.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// A SMALL constant-trip constant-fill loop UNROLLS COMPLETELY (measured:
    /// `for (i = 0; i < N; i++) A[i] = k;` with N <= 32 emits `li value; lis;
    /// stwu @lo-fold; stw` run — no loop at all; N = 33 begins the peel/ctr
    /// structure and stays deferred). Word arrays past the SDA threshold, full
    /// walks only (a partial fill is unmeasured). The fill value's home is a
    /// virtual with the scratch preference — the allocator derives r0.
    pub(crate) fn try_unrolled_fill_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
        {
            return Ok(false);
        }
        let [counter] = function.locals.as_slice() else {
            return Ok(false);
        };
        if counter.is_static
            || counter.array_length.is_some()
            || counter.initializer.is_some()
            || counter.data_bytes.is_some()
            || !matches!(counter.declared_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::For, initializer: Some(initializer), condition: Some(condition), step: Some(step), body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = 0` (a nonzero start's unroll is unmeasured), `i < N`, `i++`.
        if !matches!(initializer, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::IntegerLiteral(0)))
        {
            return Ok(false);
        }
        let bound = match condition {
            Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name) =>
            {
                match right.as_ref() {
                    Expression::IntegerLiteral(bound) if (3..=32).contains(bound) => *bound as u16,
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
        // The body: `A[i] = k` — a word global array indexed by the counter.
        let [Statement::Store { target: Expression::Index { base, index }, value: Expression::IntegerLiteral(fill) }] = body.as_slice()
        else {
            return Ok(false);
        };
        if !(i16::MIN as i64..=i16::MAX as i64).contains(fill) {
            return Ok(false);
        }
        let Expression::Variable(array) = base.as_ref() else {
            return Ok(false);
        };
        if !matches!(index.as_ref(), Expression::Variable(name) if name == &counter.name)
            || self.locations.contains_key(array.as_str())
            || !matches!(self.globals.get(array.as_str()), Some(Type::Int | Type::UnsignedInt))
        {
            return Ok(false);
        }
        let Some(&size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        if size != bound as u32 * 4 || size <= 8 {
            return Ok(false);
        }
        let array = array.clone();

        // The measured unroll: the fill value greedy-early, the base high half,
        // the offset-0 store FOLDING @lo into `stwu` (which also forms the
        // base), then the run of word stores.
        let value = self.fresh_virtual_general_preferring(0);
        self.output.instructions.push(Instruction::AddImmediate { d: value, a: 0, immediate: *fill as i16 });
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: value, a: 3, offset: 0 });
        for slot in 1..bound {
            self.output.instructions.push(Instruction::StoreWord { s: value, a: 3, offset: (slot as i16) * 4 });
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }


    /// A DYNAMIC-bound constant-zero fill (`for (i = 0; i < n; i++) A[i] = 0;`
    /// with `n` the int parameter) emits mwcc's modulo-scheduled structure,
    /// measured whole: the `n <= 0` early-out (`blelr`); the 8-way block —
    /// `blocks = (n-8+7) >> 3` into ctr, guarded twice (`n <= 8` and `n-8 <= 0`
    /// both skip to the tail), body `stw x8 / addi i,8 / addi base,32 / bdnz`;
    /// then the tail loop — base re-formed at `A + 4i`, `count = n - i` into
    /// ctr, `i >= n` exits (`bgelr`), body `stw / addi base,4 / bdnz`.
    pub(crate) fn try_dynamic_fill_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void || !function.guards.is_empty() || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(parameter.parameter_type, Type::Int) {
            return Ok(false);
        }
        let [counter] = function.locals.as_slice() else {
            return Ok(false);
        };
        if counter.is_static
            || counter.array_length.is_some()
            || counter.initializer.is_some()
            || counter.data_bytes.is_some()
            || !matches!(counter.declared_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::For, initializer: Some(initializer), condition: Some(condition), step: Some(step), body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(initializer, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::IntegerLiteral(0)))
        {
            return Ok(false);
        }
        // `i < n` — the BOUND is the parameter.
        if !matches!(condition, Expression::Binary { operator: BinaryOperator::Less, left, right }
            if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(right.as_ref(), Expression::Variable(name) if name == &parameter.name))
        {
            return Ok(false);
        }
        if !matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                    if matches!(left.as_ref(), Expression::Variable(other) if other == &counter.name)
                        && matches!(right.as_ref(), Expression::IntegerLiteral(1))))
        {
            return Ok(false);
        }
        // The body: `A[i] = 0` (the zero fill is the measured shape).
        let [Statement::Store { target: Expression::Index { base, index }, value: Expression::IntegerLiteral(0) }] = body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Variable(array) = base.as_ref() else {
            return Ok(false);
        };
        if !matches!(index.as_ref(), Expression::Variable(name) if name == &counter.name)
            || self.locations.contains_key(array.as_str())
            || !matches!(self.globals.get(array.as_str()), Some(Type::Int | Type::UnsignedInt))
        {
            return Ok(false);
        }
        let Some(&size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        if size <= 8 {
            return Ok(false);
        }
        // The parameter must sit in r3 (the measured register story).
        if self.locations.get(&parameter.name).map(|location| location.register) != Some(3) {
            return Ok(false);
        }
        let array = array.clone();

        let tail = self.fresh_label();
        let body8 = self.fresh_label();
        let body1 = self.fresh_label();
        // n <= 0: nothing to do.
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 1 });
        // Fewer than nine: straight to the tail loop.
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 3, immediate: -8 });
        self.emit_branch_conditional_to(4, 1, tail); // ble
        // blocks = (n - 8 + 7) >> 3 into ctr; base = A.
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 5, immediate: 7 });
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, shift: 3 });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, tail); // ble
        // The 8-way block: i += 8 rides the first store's latency slot.
        self.bind_label(body8);
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 6, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 7, a: 7, immediate: 8 });
        for slot in 1..8i16 {
            self.output.instructions.push(Instruction::StoreWord { s: 4, a: 6, offset: slot * 4 });
        }
        self.output.instructions.push(Instruction::AddImmediate { d: 6, a: 6, immediate: 32 });
        self.emit_branch_conditional_to(16, 0, body8); // bdnz
        // The tail loop: base = A + 4i, count = n - i.
        self.bind_label(tail);
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 5, s: 7, shift: 2 });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::SubtractFrom { d: 0, a: 7, b: 3 });
        self.output.instructions.push(Instruction::Add { d: 5, a: 4, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::CompareWord { a: 7, b: 3 });
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: 4, condition_bit: 0 });
        self.bind_label(body1);
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 5, immediate: 4 });
        self.emit_branch_conditional_to(16, 0, body1); // bdnz
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

}
