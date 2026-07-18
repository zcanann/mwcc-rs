//! Fill and copy loops (pipelined copy, unrolled/dynamic fills, iota, dynamic-call).
//!
//! Split from a single 2795-line `loops.rs` (behavior-identical).

#[allow(unused_imports)]
use super::*;

impl Generator {
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
        if !matches!(kind, LoopKind::While | LoopKind::DoWhile) {
            return Ok(false);
        }
        if !body.is_empty() {
            return Ok(false);
        }
        // The condition: *p++ = *src++ (both POSTFIX — the old pointers).
        let post_deref = |expression: &Expression| -> Option<String> {
            let Expression::Dereference { pointer } = expression else {
                return None;
            };
            let Expression::PostStep {
                target,
                operator: BinaryOperator::Add,
            } = pointer.as_ref()
            else {
                return None;
            };
            let Expression::Variable(name) = target.as_ref() else {
                return None;
            };
            Some(name.clone())
        };
        let Expression::Assign { target, value } = condition else {
            return Ok(false);
        };
        if post_deref(target).as_deref() != Some(alias)
            || post_deref(value).as_deref() != Some(source)
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
        self.output
            .instructions
            .push(Instruction::move_register(alias_register, dst_register));
        let loop_at = self.fresh_label();
        self.bind_label(loop_at);
        self.output.instructions.push(Instruction::LoadByteZero {
            d: carry,
            a: src_register,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: src_register,
            a: src_register,
            immediate: 1,
        });
        self.output
            .instructions
            .push(Instruction::ExtendSignByteRecord { a: 0, s: carry });
        self.output.instructions.push(Instruction::StoreByte {
            s: carry,
            a: alias_register,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: alias_register,
            a: alias_register,
            immediate: 1,
        });
        self.emit_branch_conditional_to(4, 2, loop_at); // bne
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
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
        // `i = 0` (a nonzero start's unroll is unmeasured), `i < N`, `i++`.
        if !matches!(initializer, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::IntegerLiteral(0)))
        {
            return Ok(false);
        }
        let bound = match condition {
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } if matches!(left.as_ref(), Expression::Variable(name) if name == &counter.name) => {
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
        let [Statement::Store {
            target: Expression::Index { base, index },
            value: Expression::IntegerLiteral(fill),
        }] = body.as_slice()
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
            || !matches!(
                self.globals.get(array.as_str()),
                Some(Type::Int | Type::UnsignedInt)
            )
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
        self.output.instructions.push(Instruction::AddImmediate {
            d: value,
            a: 0,
            immediate: *fill as i16,
        });
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: value,
                a: 3,
                offset: 0,
            });
        for slot in 1..bound {
            self.output.instructions.push(Instruction::StoreWord {
                s: value,
                a: 3,
                offset: (slot as i16) * 4,
            });
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
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
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
        // The body: `A[i] = k` — measured for any i16 constant (the value rides
        // the two `li r4` materialization sites; the structure is unchanged).
        let [Statement::Store {
            target: Expression::Index { base, index },
            value: Expression::IntegerLiteral(fill),
        }] = body.as_slice()
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
            || !matches!(
                self.globals.get(array.as_str()),
                Some(Type::Int | Type::UnsignedInt)
            )
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
        if self
            .locations
            .get(&parameter.name)
            .map(|location| location.register)
            != Some(3)
        {
            return Ok(false);
        }
        let array = array.clone();

        let tail = self.fresh_label();
        let body8 = self.fresh_label();
        let body1 = self.fresh_label();
        // n <= 0: nothing to do.
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 1,
            });
        // Fewer than nine: straight to the tail loop.
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: -8,
        });
        self.emit_branch_conditional_to(4, 1, tail); // ble
                                                     // blocks = (n - 8 + 7) >> 3 into ctr; base = A.
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 7,
        });
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 3,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: *fill as i16,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, tail); // ble
                                                     // The 8-way block: i += 8 rides the first store's latency slot.
        self.bind_label(body8);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 6,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 7,
            immediate: 8,
        });
        for slot in 1..8i16 {
            self.output.instructions.push(Instruction::StoreWord {
                s: 4,
                a: 6,
                offset: slot * 4,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 6,
            immediate: 32,
        });
        self.emit_branch_conditional_to(16, 0, body8); // bdnz
                                                       // The tail loop: base = A + 4i, count = n - i.
        self.bind_label(tail);
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 5,
                s: 7,
                shift: 2,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 7, b: 3 });
        self.output
            .instructions
            .push(Instruction::Add { d: 5, a: 4, b: 5 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: *fill as i16,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 7, b: 3 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            });
        self.bind_label(body1);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 4,
        });
        self.emit_branch_conditional_to(16, 0, body1); // bdnz
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// The DYNAMIC iota fill (`for (i = 0; i < n; i++) A[i] = i;`): the same
    /// modulo-scheduled scaffold as the constant fill, but the counter homes in
    /// r9, the block base in r8, and the 8-way body is SOFTWARE-PIPELINED with
    /// register rotation — i+1..i+7 computed three slots ahead of their stores
    /// in r4,r0,r7,r6,r5,r4,r0 (measured whole @2.6/1.3.2). The tail stores the
    /// counter itself, advancing base and counter together.
    pub(crate) fn try_dynamic_iota_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
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
        if !matches!(initializer, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::IntegerLiteral(0)))
        {
            return Ok(false);
        }
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
        // The body: `A[i] = i`.
        let [Statement::Store {
            target: Expression::Index { base, index },
            value: Expression::Variable(stored),
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Variable(array) = base.as_ref() else {
            return Ok(false);
        };
        if stored != &counter.name
            || !matches!(index.as_ref(), Expression::Variable(name) if name == &counter.name)
            || self.locations.contains_key(array.as_str())
            || !matches!(
                self.globals.get(array.as_str()),
                Some(Type::Int | Type::UnsignedInt)
            )
        {
            return Ok(false);
        }
        let Some(&size) = self.global_array_sizes.get(array.as_str()) else {
            return Ok(false);
        };
        if size <= 8 {
            return Ok(false);
        }
        if self
            .locations
            .get(&parameter.name)
            .map(|location| location.register)
            != Some(3)
        {
            return Ok(false);
        }
        let array = array.clone();

        let tail = self.fresh_label();
        let body8 = self.fresh_label();
        let body1 = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 0,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 1,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 8 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: -8,
        });
        self.emit_branch_conditional_to(4, 1, tail); // ble
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 7,
        });
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: 0,
                shift: 3,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 1, tail); // ble
                                                     // The pipelined 8-way body: values three slots ahead, rotating r4,r0,r7,r6,r5.
        self.bind_label(body8);
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 9,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 9,
            immediate: 2,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 9,
            immediate: 3,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 8,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: 9,
            immediate: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 9,
            immediate: 5,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 9,
            immediate: 6,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 9,
            immediate: 7,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 9,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 8,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 8,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 8,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 8,
            offset: 24,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: 28,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 8,
            immediate: 32,
        });
        self.emit_branch_conditional_to(16, 0, body8); // bdnz
                                                       // The tail: base r4 = A + 4i; count n-i; store the counter itself.
        self.bind_label(tail);
        self.record_relocation(RelocationKind::Addr16Ha, &array);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 5,
                s: 9,
                shift: 2,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 9, b: 3 });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 9, b: 3 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 0,
            });
        self.bind_label(body1);
        self.output.instructions.push(Instruction::StoreWord {
            s: 9,
            a: 4,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 9,
            immediate: 1,
        });
        self.emit_branch_conditional_to(16, 0, body1); // bdnz
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A DYNAMIC-bound bare call loop (`for (i = 0; i < n; i++) h();`): the
    /// bottom-test rotation with a top entry jump (the dynamic bound keeps the
    /// pre-test, unlike the constant-bound counted loop's dropped one).
    /// Measured @2.6/1.3.2: the interleaved prologue parks each home right
    /// after its save (`stw r31,12; li r31,0; stw r30,8; mr r30,r3`), body
    /// `bl; addi i,1`, test `cmpw i,n; blt`, epilogue LR-first. The counter
    /// and bound homes are VIRTUALS — both cross the call, so the allocator's
    /// callee-saved pool derives r31/r30.
    pub(crate) fn try_dynamic_call_loop(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
        {
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
        if !matches!(initializer, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == &counter.name)
                && matches!(value.as_ref(), Expression::IntegerLiteral(0)))
        {
            return Ok(false);
        }
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
        // The body: a run of one to three statements in SOURCE order (measured
        // both orders), each a direct call — bare or passing the counter (its
        // `mr r3,<home>` immediately ahead of its bl) — or a store of the
        // counter to a small word global (`gi = i` -> stw home,@gi SDA).
        // At least one call (a store-only body is a fill loop, not this shape).
        enum BodyStep {
            Call(String, bool),
            StoreCounter(String),
            /// `if (gFlag) h(); [else k();]` — the flag reloads, a forward beq
            /// routes to the else (or past the call); a then-side `b` joins.
            GuardedCall(String, String, Option<String>),
            /// `A[i] = i;` — the store through the WALKING POINTER home, which
            /// advances 4 right after (the base never rematerializes).
            StoreCounterToArray,
        }
        if body.is_empty() || body.len() > 3 {
            return Ok(false);
        }
        let mut body_steps = Vec::with_capacity(body.len());
        let mut call_count = 0usize;
        let mut walking_array: Option<String> = None;
        for statement in body {
            match statement {
                Statement::Expression(Expression::Call {
                    name: callee,
                    arguments,
                }) => {
                    let passes_counter = match arguments.as_slice() {
                        [] => false,
                        [Expression::Variable(variable)] if variable == &counter.name => true,
                        _ => return Ok(false),
                    };
                    if self.locations.contains_key(callee.as_str())
                        || self.globals.contains_key(callee.as_str())
                        || matches!(
                            self.call_return_types.get(callee.as_str()),
                            Some(Type::Float | Type::Double)
                        )
                    {
                        return Ok(false);
                    }
                    call_count += 1;
                    body_steps.push(BodyStep::Call(callee.clone(), passes_counter));
                }
                Statement::Store {
                    target: Expression::Index { base, index },
                    value: Expression::Variable(stored),
                } => {
                    let Expression::Variable(array_name) = base.as_ref() else {
                        return Ok(false);
                    };
                    if stored != &counter.name
                        || !matches!(index.as_ref(), Expression::Variable(name) if name == &counter.name)
                        || self.locations.contains_key(array_name.as_str())
                        || !matches!(
                            self.globals.get(array_name.as_str()),
                            Some(Type::Int | Type::UnsignedInt)
                        )
                        || walking_array.is_some()
                    {
                        return Ok(false);
                    }
                    let Some(&size) = self.global_array_sizes.get(array_name.as_str()) else {
                        return Ok(false);
                    };
                    if size <= 8 {
                        return Ok(false);
                    }
                    walking_array = Some(array_name.clone());
                    body_steps.push(BodyStep::StoreCounterToArray);
                }
                Statement::Store {
                    target: Expression::Variable(global),
                    value: Expression::Variable(stored),
                } => {
                    if stored != &counter.name
                        || self.locations.contains_key(global.as_str())
                        || !matches!(
                            self.globals.get(global.as_str()),
                            Some(Type::Int | Type::UnsignedInt)
                        )
                        || self.global_array_sizes.contains_key(global.as_str())
                    {
                        return Ok(false);
                    }
                    body_steps.push(BodyStep::StoreCounter(global.clone()));
                }
                Statement::If {
                    condition: Expression::Variable(flag),
                    then_body,
                    else_body,
                } => {
                    let [Statement::Expression(Expression::Call {
                        name: callee,
                        arguments,
                    })] = then_body.as_slice()
                    else {
                        return Ok(false);
                    };
                    let bare_call_ok = |generator: &Self, name: &str| {
                        !generator.locations.contains_key(name)
                            && !generator.globals.contains_key(name)
                            && !matches!(
                                generator.call_return_types.get(name),
                                Some(Type::Float | Type::Double)
                            )
                    };
                    let else_callee = match else_body.as_slice() {
                        [] => None,
                        [Statement::Expression(Expression::Call {
                            name: else_name,
                            arguments: else_arguments,
                        })] => {
                            if !else_arguments.is_empty() || !bare_call_ok(self, else_name) {
                                return Ok(false);
                            }
                            Some(else_name.clone())
                        }
                        _ => return Ok(false),
                    };
                    if !arguments.is_empty()
                        || self.locations.contains_key(flag.as_str())
                        || !matches!(
                            self.globals.get(flag.as_str()),
                            Some(Type::Int | Type::UnsignedInt)
                        )
                        || self.global_array_sizes.contains_key(flag.as_str())
                        || !bare_call_ok(self, callee)
                    {
                        return Ok(false);
                    }
                    call_count += 1;
                    body_steps.push(BodyStep::GuardedCall(
                        flag.clone(),
                        callee.clone(),
                        else_callee,
                    ));
                }
                _ => return Ok(false),
            }
        }
        if call_count == 0 {
            return Ok(false);
        }
        // The body's @N advance (measured): a counter-store = flat 5 (one or
        // two alike), a guarded call = flat 7, a pure call body = 0. The
        // store+guard MIX is unmeasured — defer rather than guess the counter.
        let has_store = body_steps.iter().any(|step| {
            matches!(
                step,
                BodyStep::StoreCounter(_) | BodyStep::StoreCounterToArray
            )
        });
        let has_guard = body_steps
            .iter()
            .any(|step| matches!(step, BodyStep::GuardedCall(_, _, None)));
        let has_diamond = body_steps
            .iter()
            .any(|step| matches!(step, BodyStep::GuardedCall(_, _, Some(_))));
        if has_diamond && (has_store || has_guard || body_steps.len() > 1) {
            // Only the LONE if/else body's label count is measured.
            return Err(Diagnostic::error(
                "a loop body mixing an if/else with other steps needs its label count measured (roadmap)",
            ));
        }
        if has_diamond {
            self.output.anonymous_label_bump += 8;
        }
        match (has_store, has_guard) {
            (true, true) => {
                return Err(Diagnostic::error(
                    "a loop body mixing counter-stores and guarded calls needs its label count measured (roadmap)",
                ));
            }
            (true, false) => self.output.anonymous_label_bump += 5,
            (false, true) => self.output.anonymous_label_bump += 7,
            (false, false) => {}
        }
        if self
            .locations
            .get(&parameter.name)
            .map(|location| location.register)
            != Some(3)
        {
            return Ok(false);
        }

        // The walking-pointer home (an array-iota store) is created FIRST so the
        // pool assigns it r31, then the counter, then the bound (measured order).
        let base_home = walking_array.as_ref().map(|_| self.fresh_virtual_general());
        let counter_home = self.fresh_virtual_general();
        let bound_home = self.fresh_virtual_general();
        let homes: Vec<u8> = base_home
            .iter()
            .copied()
            .chain([counter_home, bound_home])
            .collect();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes;
        self.epilogue_lr_before_gprs = true;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -plan.frame_size,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        let mut save_offset = plan.frame_size - 4;
        if let (Some(base_home), Some(array)) = (base_home, walking_array.as_ref()) {
            // The base's high half rides the mflr latency gap; the @lo addi is
            // the base home's park, interleaved after its save.
            self.record_relocation(RelocationKind::Addr16Ha, array);
            self.output
                .instructions
                .push(Instruction::AddImmediateShifted {
                    d: 4,
                    a: 0,
                    immediate: 0,
                });
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: plan.frame_size + 4,
            });
            self.output.instructions.push(Instruction::StoreWord {
                s: base_home,
                a: 1,
                offset: save_offset,
            });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate {
                d: base_home,
                a: 4,
                immediate: 0,
            });
            save_offset -= 4;
        } else {
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: plan.frame_size + 4,
            });
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: counter_home,
            a: 1,
            offset: save_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: counter_home,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: bound_home,
            a: 1,
            offset: save_offset - 4,
        });
        self.output.instructions.push(Instruction::Or {
            a: bound_home,
            s: 3,
            b: 3,
        });
        let test = self.fresh_label();
        let loop_body = self.fresh_label();
        self.emit_branch_to(test);
        self.bind_label(loop_body);
        for step in &body_steps {
            match step {
                BodyStep::Call(callee, passes_counter) => {
                    if *passes_counter {
                        self.output.instructions.push(Instruction::Or {
                            a: 3,
                            s: counter_home,
                            b: counter_home,
                        });
                    }
                    self.emit_call(callee, &[], None, false)?;
                }
                BodyStep::StoreCounter(global) => {
                    self.record_relocation(RelocationKind::EmbSda21, global);
                    self.output.instructions.push(Instruction::StoreWord {
                        s: counter_home,
                        a: 0,
                        offset: 0,
                    });
                }
                BodyStep::StoreCounterToArray => {
                    let base_home = base_home.expect("decode guaranteed the walking pointer");
                    self.output.instructions.push(Instruction::StoreWord {
                        s: counter_home,
                        a: base_home,
                        offset: 0,
                    });
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: base_home,
                        a: base_home,
                        immediate: 4,
                    });
                }
                BodyStep::GuardedCall(flag, callee, else_callee) => {
                    let after_then = self.fresh_label();
                    self.record_relocation(RelocationKind::EmbSda21, flag);
                    self.output.instructions.push(Instruction::LoadWord {
                        d: 0,
                        a: 0,
                        offset: 0,
                    });
                    self.output
                        .instructions
                        .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
                    self.emit_branch_conditional_to(12, 2, after_then); // beq
                    self.emit_call(callee, &[], None, false)?;
                    if let Some(else_callee) = else_callee {
                        let join = self.fresh_label();
                        self.emit_branch_to(join);
                        self.bind_label(after_then);
                        self.emit_call(else_callee, &[], None, false)?;
                        self.bind_label(join);
                    } else {
                        self.bind_label(after_then);
                    }
                }
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: counter_home,
            a: counter_home,
            immediate: 1,
        });
        self.bind_label(test);
        self.output.instructions.push(Instruction::CompareWord {
            a: counter_home,
            b: bound_home,
        });
        self.emit_branch_conditional_to(12, 0, loop_body); // blt
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
