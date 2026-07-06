//! Guard sequences, early-return branches, and if/else emission.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit the whole function body, including its `blr`(s).
    /// A body that continued past its early-return guards parses them into the ordered
    /// statement list as `If { then_body: [Return(Some(v))] }` entries. When every such
    /// leading guard reads only names the remaining statements never assign (and no local,
    /// whose tracked value the guard would need substituted), the guard reads the same
    /// pristine registers whether emitted before or after the (virtual, value-tracked)
    /// reassignments — so it moves back into `guards` for the trailing-guard machinery.
    /// Only shapes where mwcc compiles both orders IDENTICALLY hoist: the guard value must
    /// be a CONSTANT (a register value branches in the ordered source but folds inverted in
    /// the flat one) and the tail must not read the result register's parameter (the fold's
    /// `li r3,V` clobbers it — mwcc branches in the ordered source, folds through a temp in
    /// the flat one). The rest must be pure reassignments (the value-tracking shape).
    pub(crate) fn hoist_order_independent_leading_guards(&self, function: &Function) -> Option<Function> {
        if !matches!(function.statements.first(), Some(Statement::If { .. })) {
            return None;
        }
        let mut hoisted: Vec<GuardedReturn> = Vec::new();
        let mut rest: Vec<Statement> = Vec::new();
        let mut in_prefix = true;
        for statement in &function.statements {
            if in_prefix {
                if let Statement::If { condition, then_body, else_body } = statement {
                    if else_body.is_empty() {
                        if let [Statement::Return(Some(value))] = then_body.as_slice() {
                            hoisted.push(GuardedReturn { condition: condition.clone(), value: value.clone() });
                            continue;
                        }
                    }
                }
                in_prefix = false;
            }
            rest.push(statement.clone());
        }
        if hoisted.is_empty() || !rest.iter().all(|statement| matches!(statement, Statement::Assign { .. })) {
            return None;
        }
        let written: Vec<&str> = rest
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assign { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .chain(function.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let reads_written = |expression: &Expression| written.iter().any(|name| expression_reads_name(expression, name));
        if hoisted.iter().any(|guard| reads_written(&guard.condition) || reads_written(&guard.value)) {
            return None;
        }
        // A guard value must be a constant (the direct fold) or a plain variable (the
        // inverted fold: `cmpwi; addi r3,r4,1; beqlr; mr r3,c` — verified identical in
        // both orders for one-parameter tails). Computed values stay ordered.
        if hoisted
            .iter()
            .any(|guard| constant_value(&guard.value).is_none() && !matches!(&guard.value, Expression::Variable(_)))
        {
            return None;
        }
        // The tail (any reassigned value, or the return expression) must not read the
        // parameter living in the result register — the fold clobbers it. Such bodies
        // stay ordered for the branch-form handler.
        if let Some(occupant) = self.locations.iter().find_map(|(name, location)| {
            (location.register == mwcc_target::Eabi::general_result().number && location.class == ValueClass::General)
                .then_some(name.as_str())
        }) {
            let tail_reads_occupant = rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, occupant),
                _ => false,
            }) || function.return_expression.as_ref().is_some_and(|ret| expression_reads_name(ret, occupant));
            if tail_reads_occupant {
                return None;
            }
        }
        // A tail reading TWO OR MORE distinct parameters does not fold directly: mwcc schedules it
        // into the local's home register ahead of the guard value (`add r0,r4,r5; li r3,5; bnelr; mr
        // r3,r0` flat, a real branch ordered) — an order-dependent form that stays ordered for the
        // branch-form handler. Count over the SUBSTITUTED tail so a reassigned parameter read as its
        // tracked value (`c = b + 1; return c;` -> `b + 1`, one parameter) folds like the reassign-
        // in-place shapes, while a self-referential reassignment (`c = b + c` -> `b + c`, two) bails.
        let mut tracked: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
        for local in &function.locals {
            if let Some(initializer) = &local.initializer {
                let value = crate::value_tracking::substitute(initializer, &tracked);
                tracked.insert(local.name.clone(), value);
            }
        }
        for statement in &rest {
            if let Statement::Assign { name, value } = statement {
                let value = crate::value_tracking::substitute(value, &tracked);
                tracked.insert(name.clone(), value);
            }
        }
        if let Some(return_expression) = &function.return_expression {
            let inlined = crate::value_tracking::substitute(return_expression, &tracked);
            if function.parameters.iter().filter(|parameter| expression_reads_name(&inlined, &parameter.name)).count() > 1 {
                return None;
            }
        }
        let mut guards = hoisted;
        guards.extend(function.guards.iter().cloned());
        Some(Function { statements: rest, guards, ..function.clone() })
    }

    /// The ordered early-return BRANCH form: a single leading `if (c) return v;` whose body
    /// continues with pure reassignments. Where the constant fold does not apply (a register
    /// guard value, or a tail still reading the result register's parameter), mwcc emits a
    /// real forward branch — `<condition>; b<false> CONT; <value into r3>; blr; CONT: <tail>`
    /// (`if (a) return c; b = b + c; return b;` → `cmpwi; beq +8; mr r3,r5; blr; add; blr`).
    /// The guard must read only names the rest never assigns (a guard reading an assigned
    /// name joins through r0 instead — not modeled). The continuation is delegated to value
    /// tracking; a continuation it cannot compile defers the whole body (the guard block is
    /// already emitted, so a bare `Ok(false)` would leave partial output).
    /// A guarded computed-index GLOBAL-ARRAY store with a constant return:
    /// `if (i < 1) return -1; arr[i - 1] = 0; return 0;` (the signal.c shape). The
    /// address build interleaves with the live return value, in three captured forms:
    /// - constant value, offset 0:  `lis r4; slwi r0,i; addi r3,r4; li r4,C; stwx r4,r3,r0; li r3,R`
    /// - constant value, offset ±k: `lis r4; slwi; addi r3,r4; li r5,C; add r4,r3,r0; li r3,R; stw r5,k(r4)`
    /// - register value, offset 0:  `lis r5; slwi; addi r5,r5; li r3,R; stwx v,r5,r0`
    /// A register value with a folded offset is uncaptured; small (SDA21) arrays,
    /// float/byte elements, and non-constant returns fall to the scheduler defer.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_guarded_global_array_store(
        &mut self,
        function: &Function,
        condition: &Expression,
        guard_value: &Expression,
        array: &str,
        total_size: u32,
        index: &Expression,
        stored: &Expression,
    ) -> Compilation<bool> {
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(return_constant) = function
            .return_expression
            .as_ref()
            .and_then(|expression| constant_value(expression))
            .and_then(|constant| i16::try_from(constant).ok())
        else {
            return Ok(false);
        };
        if self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8 {
            return Ok(false);
        }
        let Some(pointee) = pointee_of_type(self.globals[array]) else {
            return Ok(false);
        };
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            return Ok(false);
        }
        let size = pointee.size();
        if size == 1 {
            return Ok(false);
        }
        // `arr[i ± k]` folds the scaled element offset onto the store displacement.
        let mut index_leaf = index;
        let mut element_offset: i64 = 0;
        if let Expression::Binary { operator, left, right } = index {
            if let Some(k) = constant_value(right) {
                match operator {
                    BinaryOperator::Add => {
                        index_leaf = left.as_ref();
                        element_offset = k * size as i64;
                    }
                    BinaryOperator::Subtract => {
                        index_leaf = left.as_ref();
                        element_offset = -k * size as i64;
                    }
                    _ => {}
                }
            }
        }
        if !matches!(index_leaf, Expression::Variable(_)) {
            return Ok(false);
        }
        let Ok(offset) = i16::try_from(element_offset) else {
            return Ok(false);
        };
        let stored_constant = constant_value(stored).and_then(|constant| i16::try_from(constant).ok());
        let stored_register = if stored_constant.is_none() {
            let Expression::Variable(name) = stored else { return Ok(false) };
            let Some(register) = self.lookup_general(name) else { return Ok(false) };
            if offset != 0 {
                return Ok(false);
            }
            Some(register)
        } else {
            None
        };

        let result = mwcc_target::Eabi::general_result().number;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.evaluate_tail(guard_value, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        let continuation = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = continuation;
        }

        let index_register = self.general_register_of_leaf(index_leaf)?;
        let shift = size.trailing_zeros() as u8;
        if let Some(register) = stored_register {
            // Register value: the base stays OUT of the index register (the return needs
            // r3 live before the store) — `lis B; slwi; addi B,B; li r3,R; stwx v,B,r0`.
            let base = self.fresh_virtual_general();
            self.emit_address_high(base, array);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate { d: base, a: base, immediate: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: return_constant });
            self.output.instructions.push(crate::expressions::indexed_store(pointee, register, base, GENERAL_SCRATCH)?);
        } else {
            let constant = stored_constant.expect("checked above");
            // Phase D: the base-high is a virtual in both forms — a redefined vreg keeps
            // ONE live range spanning the redefinition (the offset≠0 form reuses it for
            // the effective address), so the value's overlapping virtual lands on the
            // next register, matching mwcc's r4/r5 split.
            let high = self.fresh_virtual_general();
            self.emit_address_high(high, array);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
            if offset == 0 {
                // The standalone sequence, the return materialized after the store.
                self.output.instructions.push(Instruction::AddImmediate { d: high, a: 0, immediate: constant });
                self.output.instructions.push(crate::expressions::indexed_store(pointee, high, index_register, GENERAL_SCRATCH)?);
                self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: return_constant });
            } else {
                // The value's virtual overlaps the still-live high (which the `add`
                // redefines as the effective address), so it allocates past it.
                let value_register = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::AddImmediate { d: value_register, a: 0, immediate: constant });
                self.output.instructions.push(Instruction::Add { d: high, a: index_register, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: return_constant });
                self.output.instructions.push(displacement_store(pointee, value_register, high, offset)?);
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn try_ordered_early_return_branch(&mut self, function: &Function) -> Compilation<bool> {
        // A VOID early return over a single-store continuation: `if (a) return; *p = 5;`
        // is a conditional RETURN (the void exit needs no value), then the plain store
        // body — `cmpwi; bnelr; li r0,5; stw r0,0(r4); blr`. The store emission is the
        // standalone sequential form (no return value to schedule around).
        if function.return_type == Type::Void
            && function.guards.is_empty()
            && function.return_expression.is_none()
            && function.locals.is_empty()
            && !function_makes_call(function)
        {
            if let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() {
                if matches!(then_body.as_slice(), [Statement::Return(None)])
                    && else_body.is_empty()
                    && matches!(rest, [Statement::Store { .. }])
                {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                    self.emit_statement(&rest[0])?;
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if !function.guards.is_empty() || function.return_type == Type::Void || function.return_expression.is_none() {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() else {
            return Ok(false);
        };
        let [Statement::Return(Some(value))] = then_body.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() || rest.len() != 1 || function_makes_call(function) {
            return Ok(false);
        }

        // A store continuation: `if (a) return -1; *p = 5; return 0;`. A MATERIALIZED store
        // value (a constant, or a simple two-leaf computation) lands in r0 with the return
        // value scheduled BETWEEN the materialization and the store — `li r0,5; li r3,0;
        // stw r0,0(r4); blr` / `addi r0,r5,1; li r3,0; stw r0,0(r4)` (or `mr r3,x` for a
        // register return). Covers `*p`, `p[const]`, and `p->member` targets. A register-
        // valued store needs no materialization and stays with the sequential path (store,
        // then the return move — verified byte-exact there); two or more stores interleave
        // through the batch scheduler and defer.
        if let [Statement::Store { target, value: stored }] = rest {
            if function.guards.is_empty() && function.locals.is_empty() {
                // A computed-index GLOBAL-ARRAY target has its own captured schedules
                // (the address build interleaves with the return) — a dedicated arm.
                if let Expression::Index { base, index } = target {
                    if let Expression::Variable(array) = base.as_ref() {
                        if let Some(&total_size) = self.global_array_sizes.get(array.as_str()) {
                            if constant_value(index).is_none() {
                                return self.try_guarded_global_array_store(function, condition, value, array, total_size, index, stored);
                            }
                        }
                    }
                }
                let stored_is_constant = constant_value(stored).and_then(|constant| i16::try_from(constant).ok()).is_some();
                let stored_is_two_leaf = matches!(stored, Expression::Binary { left, right, .. }
                    if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                        && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
                if !stored_is_constant && !stored_is_two_leaf {
                    return Ok(false);
                }
                let (pointer_name, byte_offset, pointee): (&String, i64, Pointee) = match target {
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
                if !function.parameters.iter().any(|parameter| &parameter.name == pointer_name) {
                    return Ok(false);
                }
                let Some(pointer_register) = self.lookup_general(pointer_name) else {
                    return Ok(false);
                };
                if matches!(pointee, Pointee::Float | Pointee::Double) {
                    return Ok(false);
                }
                let Ok(offset) = i16::try_from(byte_offset) else {
                    return Ok(false);
                };
                // The return value: a constant `li`, or a General register `mr`.
                enum ReturnValue {
                    Constant(i16),
                    Register(u8),
                }
                let return_value = match function.return_expression.as_ref() {
                    Some(expression) => {
                        if let Some(constant) = constant_value(expression).and_then(|constant| i16::try_from(constant).ok()) {
                            ReturnValue::Constant(constant)
                        } else if let Expression::Variable(name) = expression {
                            match self.lookup_general(name) {
                                Some(register) => ReturnValue::Register(register),
                                None => return Ok(false),
                            }
                        } else {
                            return Ok(false);
                        }
                    }
                    None => return Ok(false),
                };
                if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
                    return Ok(false);
                }

                let result = mwcc_target::Eabi::general_result().number;
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.evaluate_tail(value, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                let continuation = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = continuation;
                }
                self.evaluate_general(stored, GENERAL_SCRATCH)?;
                match return_value {
                    ReturnValue::Constant(constant) => {
                        self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: constant });
                    }
                    ReturnValue::Register(register) => {
                        self.output.instructions.push(Instruction::Or { a: result, s: register, b: register });
                    }
                }
                self.output.instructions.push(displacement_store(pointee, GENERAL_SCRATCH, pointer_register, offset)?);
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            return Ok(false);
        }

        // A single reassignment is the verified continuation shape; longer tails are
        // unverified against mwcc (they may fold or reschedule differently) — defer.
        if !rest.iter().all(|statement| matches!(statement, Statement::Assign { .. })) {
            return Ok(false);
        }
        let written: Vec<&str> = rest
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assign { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .chain(function.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let reads_written = |expression: &Expression| written.iter().any(|name| expression_reads_name(expression, name));
        // A guard VALUE reading a reassigned name is unverified — defer.
        if reads_written(value) {
            return Ok(false);
        }
        let tail_reads_parameter = |name: &str| {
            rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, name),
                _ => false,
            }) || function.return_expression.as_ref().is_some_and(|ret| expression_reads_name(ret, name))
        };
        let distinct_parameter_reads =
            function.parameters.iter().filter(|parameter| tail_reads_parameter(&parameter.name)).count();

        // The branch form is mwcc's shape for a tail reading TWO-plus distinct parameters
        // (`add r3,r4,r5` after the branch), with a condition reading no reassigned name.
        if distinct_parameter_reads >= 2 && !reads_written(condition) {
            // The guard block: branch past it when the condition is false, else the value and
            // return. emit_condition_test yields the skip-when-false encoding directly.
            let result = mwcc_target::Eabi::general_result().number;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if matches!(value, Expression::Variable(name) if self.lookup_general(name) == Some(result)) {
                // The guard VALUE already occupies the result register, so mwcc returns it with a
                // single conditional branch-to-lr (`bgtlr`) — the forward branch over a `blr` whose
                // value move would be a no-op is redundant. options^8 turns the skip-when-false
                // encoding into return-when-true.
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
            } else {
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.evaluate_tail(value, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                let continuation = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = continuation;
                }
            }

            // The continuation is a pure value-tracking body; anything it cannot compile must
            // DEFER (the guard block is already in the output).
            let reduced = Function { statements: rest.to_vec(), ..function.clone() };
            if !self.try_value_tracking(&reduced)? {
                return Err(Diagnostic::error("an early-return continuation outside the value-tracking shape is not supported yet (roadmap)"));
            }
            return Ok(true);
        }

        // A ONE-parameter tail with a register guard value takes the INVERTED FOLD even when
        // the condition reads the reassigned name — the compare tests the ORIGINAL value
        // before the tail clobbers it in place: `if (a) return b; a = a + 1; return a;` →
        // `cmpwi r3,0; addi r3,r3,1; beqlr; mr r3,r4`. Kept to the exactly-verified shape: a
        // single `x = <two-leaf expr>; return x;` alias continuation, an unwritten plain-
        // variable guard value. (The order-independent variant without reassigned-name reads
        // is hoisted before this handler runs; a constant guard value here joins through a
        // temp register whose choice needs the register allocator — defer.)
        if distinct_parameter_reads < 2
            && matches!(value, Expression::Variable(_))
            && matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            let [Statement::Assign { name: assigned, value: assigned_value }] = rest else {
                return Ok(false);
            };
            let Some(Expression::Variable(returned)) = function.return_expression.as_ref() else {
                return Ok(false);
            };
            if returned != assigned {
                return Ok(false);
            }
            let two_leaf = matches!(assigned_value, Expression::Binary { left, right, .. }
                if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                    && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
            if !two_leaf {
                return Ok(false);
            }
            let Expression::Variable(value_name) = value else { return Ok(false) };
            let Some(value_register) = self.lookup_general(value_name) else {
                return Ok(false);
            };
            let result = mwcc_target::Eabi::general_result().number;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.evaluate_tail(assigned_value, function.return_type, result)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            self.output.instructions.push(Instruction::Or { a: result, s: value_register, b: value_register });
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        Ok(false)
    }

    /// GUARD-BLOCK MUTATIONS (the s_floor skeleton, fire 377): a chain of
    /// nested no-else ifs whose innermost body only ASSIGNS constants to int
    /// params, followed by a return expression. Every guard branches to ONE
    /// join; the block mutates the params in their own registers; the join
    /// computes the return (measured: `if(c){i0=0;i1=0;} return i0|i1` =
    /// cmpwi; beq J; li; li; J: or; blr — and the nested form re-tests each
    /// guard to the same join).
    pub(crate) fn try_guard_block_mutations(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(return_expression) = &function.return_expression else {
            return Ok(false);
        };
        // Flatten the guard chain: each level is exactly [If{no else}].
        let mut conditions: Vec<&Expression> = Vec::new();
        let mut body: &[Statement] = &function.statements;
        loop {
            match body {
                [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => {
                    conditions.push(condition);
                    body = then_body;
                }
                _ => break,
            }
        }
        if conditions.is_empty() {
            return Ok(false);
        }
        // A MID-CHAIN early return heads the innermost block (measured:
        // `if ((i0|i1)==0) return 7;` = the record-form test, bne PAST the
        // inline return to the mutations).
        let mut early_return: Option<(&Expression, &Expression)> = None;
        if let [Statement::If { condition, then_body, else_body }, rest @ ..] = body {
            if else_body.is_empty() {
                if let [Statement::Return(Some(value))] = then_body.as_slice() {
                    early_return = Some((condition, value));
                    body = rest;
                }
            }
        }
        // The innermost block: assigns to DISTINCT int params — an i16
        // constant (li), a lis-able constant (measured: 0xbff00000), or a
        // leaf-plus-i16 over a param no EARLIER assign in the block already
        // overwrote (measured: i0 = i1 + 1 before i1's own overwrite).
        enum BlockValue {
            Small(i16),
            High(i16),
            LeafAdd(u8, i16),
            Mask(u8, u8),
        }
        let mut assigns: Vec<(u8, BlockValue)> = Vec::new();
        for statement in body {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            let Some(location) = self.locations.get(name.as_str()) else {
                return Ok(false);
            };
            if location.class != ValueClass::General || location.width != 32 {
                return Ok(false);
            }
            let target = location.register;
            let block_value = if let Some(constant) = crate::analysis::constant_value(value) {
                if let Ok(small) = i16::try_from(constant) {
                    BlockValue::Small(small)
                } else if constant & 0xffff == 0 && u32::try_from(constant).is_ok() {
                    BlockValue::High((constant >> 16) as i16)
                } else {
                    return Ok(false);
                }
            } else {
                // Self-masking (`i0 &= C`, desugared): the in-place rlwinm
                // (measured: clrlwi r3,r3,21 in source order).
                if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = value {
                    if let Expression::Variable(read) = left.as_ref() {
                        if read == name {
                            if let Some(mask) = crate::analysis::constant_value(right) {
                                if let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) {
                                    if assigns.iter().any(|&(written, _)| written == target) {
                                        return Ok(false);
                                    }
                                    assigns.push((target, BlockValue::Mask(begin, end)));
                                    continue;
                                }
                            }
                        }
                    }
                    return Ok(false);
                }
                // leaf ± i16 (Add with a possibly-negative constant).
                let (leaf, offset) = match value {
                    Expression::Variable(read) => (read, 0i64),
                    Expression::Binary { operator: BinaryOperator::Add, left, right } => {
                        let Expression::Variable(read) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(offset) = crate::analysis::constant_value(right) else {
                            return Ok(false);
                        };
                        (read, offset)
                    }
                    Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
                        let Expression::Variable(read) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(offset) = crate::analysis::constant_value(right) else {
                            return Ok(false);
                        };
                        (read, -offset)
                    }
                    _ => return Ok(false),
                };
                let Some(read_location) = self.locations.get(leaf.as_str()) else {
                    return Ok(false);
                };
                if read_location.class != ValueClass::General || read_location.width != 32 {
                    return Ok(false);
                }
                let Ok(offset) = i16::try_from(offset) else {
                    return Ok(false);
                };
                if offset == 0 {
                    // A bare register move inside the block is unmeasured.
                    return Ok(false);
                }
                // The read must precede any overwrite of its register — and
                // a SELF-read (i0 = i0 + 5) reorders in mwcc (the
                // independent li hoists above the self-addi; probed) — defer.
                if read_location.register == target
                    || assigns.iter().any(|&(written, _)| written == read_location.register)
                {
                    return Ok(false);
                }
                BlockValue::LeafAdd(read_location.register, offset)
            };
            if assigns.iter().any(|&(register, _)| register == target) {
                return Ok(false);
            }
            assigns.push((target, block_value));
        }
        if early_return.is_none() && assigns.len() < 2 && conditions.len() < 2 {
            // The single-guard single-assign shapes belong to the measured
            // reassign/select arms.
            return Ok(false);
        }
        if assigns.is_empty() {
            return Ok(false);
        }
        // A bare-variable return folds the guards to conditional RETURNS
        // (bclr) instead of branch-to-join — the reassign arms' territory;
        // this arm takes the expression-return join form only (measured).
        if matches!(return_expression, Expression::Variable(_)) {
            return Ok(false);
        }
        // The return must be claimable by the plain tail evaluator: an
        // expression over params (no calls — gated above).
        // -- commit --
        let join = self.fresh_label();
        for condition in conditions {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.emit_branch_conditional_to(options, condition_bit, join);
        }
        if let Some((condition, value)) = early_return {
            let result = Eabi::general_result().number;
            // A bare return of the value already in r3 FOLDS to a
            // conditional return (measured: or.; beqlr).
            if let Expression::Variable(name) = value {
                if self.lookup_general(name) == Some(result) {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                    for (register, block_value) in &assigns {
                        match block_value {
                            BlockValue::Small(constant) => {
                                self.output.instructions.push(Instruction::load_immediate(*register, *constant));
                            }
                            BlockValue::High(high) => {
                                self.output.instructions.push(Instruction::load_immediate_shifted(*register, *high));
                            }
                            BlockValue::LeafAdd(source, offset) => {
                                self.output.instructions.push(Instruction::AddImmediate {
                                    d: *register,
                                    a: *source,
                                    immediate: *offset,
                                });
                            }
                            BlockValue::Mask(begin, end) => {
                                self.output.instructions.push(Instruction::RotateAndMask {
                                    a: *register,
                                    s: *register,
                                    shift: 0,
                                    begin: *begin,
                                    end: *end,
                                });
                            }
                        }
                    }
                    self.bind_label(join);
                    self.evaluate_tail(return_expression, function.return_type, result)?;
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
            }
            // Skip the inline return when the early condition fails; the
            // skip lands on the MUTATIONS, not the join.
            let mutations = self.fresh_label();
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.emit_branch_conditional_to(options, condition_bit, mutations);
            match crate::analysis::constant_value(value) {
                Some(constant) if i16::try_from(constant).is_ok() => {
                    self.output.instructions.push(Instruction::load_immediate(result, constant as i16));
                }
                Some(_) => return Err(Diagnostic::error("early-return constant beyond i16 (roadmap)")),
                None => {
                    self.evaluate_tail(value, function.return_type, result)?;
                }
            }
            self.emit_epilogue_and_return();
            self.bind_label(mutations);
        }
        for (register, value) in &assigns {
            match value {
                BlockValue::Small(constant) => {
                    self.output.instructions.push(Instruction::load_immediate(*register, *constant));
                }
                BlockValue::High(high) => {
                    self.output.instructions.push(Instruction::load_immediate_shifted(*register, *high));
                }
                BlockValue::LeafAdd(source, offset) => {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: *register,
                        a: *source,
                        immediate: *offset,
                    });
                }
                BlockValue::Mask(begin, end) => {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: *register,
                        s: *register,
                        shift: 0,
                        begin: *begin,
                        end: *end,
                    });
                }
            }
        }
        self.bind_label(join);
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expression, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A trailing leaf `if (c) then; [else otherwise | else if …]` in a void
    /// function. With no else, the false path is a conditional return (the body
    /// then falls through to the function `blr`). With an else, branch over the
    /// then-body (and its `blr`) to the else, which is either a single statement
    /// or a nested trailing if (an `else if` chain). Each then-body is a single
    /// statement — multiple statements need the scheduler.
    pub(crate) fn emit_trailing_if(&mut self, condition: &Expression, then_body: &[Statement], else_body: &[Statement], nested: bool) -> Compilation<()> {
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
        if let ([Statement::Store { target: then_target, value: then_value }],
                [Statement::Store { target: else_target, value: else_value }]) = (then_body, else_body)
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
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            for statement in then_body {
                self.emit_statement(statement)?;
            }
            return Ok(());
        }
        if then_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement if-body needs the scheduler (roadmap)"));
        }
        // A nested else-if whose comparison REUSES this comparison's condition register
        // (same operand against the same value — `if(c>0) … else if(c<0) …`, which mwcc
        // lowers with ONE `cmpwi` shared by both branches, `ble`/`bge` off the same CR) is
        // not reproduced yet; defer rather than emit a redundant second `cmpwi` (wrong bytes).
        // A different operand or a different compared value re-tests normally and is unaffected.
        if let [Statement::If { condition: else_condition, .. }] = else_body {
            if shares_condition_register(condition, else_condition) {
                return Err(Diagnostic::error("consecutive else-if comparisons that reuse the condition register are not supported yet (roadmap)"));
            }
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if else_body.is_empty() {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            return self.emit_statement(&then_body[0]);
        }
        // An `else if` chain keeps the two-exit form: the then-arm returns (`blr`), then
        // the nested trailing `if`.
        if let [Statement::If { condition: else_condition, then_body: else_then, else_body: else_else }] = else_body {
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.emit_statement(&then_body[0])?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
            return self.emit_trailing_if(else_condition, else_then, else_else, true);
        }
        if else_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement else-body needs the scheduler (roadmap)"));
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
        let is_global_store = |statement: &Statement| {
            matches!(statement, Statement::Store { target: Expression::Variable(name), .. } if self.globals.contains_key(name.as_str()))
        };
        let use_retest = truthy && is_global_store(&then_body[0]) && is_global_store(&else_body[0]);
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.emit_statement(&then_body[0])?;
        if use_retest {
            let label = self.output.instructions.len();
            let (retest_options, retest_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: retest_options ^ 8, condition_bit: retest_bit });
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
        } else {
            // Two-exit form: the then-arm returns, the conditional branch lands on the else.
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
        }
        self.emit_statement(&else_body[0])?;
        Ok(())
    }

    /// A non-trailing `if (c) { body }`: the false path branches forward over the
    /// body to the code that follows.
    pub(crate) fn emit_if_forward(&mut self, condition: &Expression, then_body: &[Statement]) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        Ok(())
    }

    /// A leaf `if (c) { … return [v]; }` whose then-body ends in an early return:
    /// forward-branch over the body when the condition is false, emit the body
    /// (the `return` materializes the value and runs the epilogue — `blr` for a
    /// leaf), then patch the branch to land on the continuation (the rest of the
    /// function, which supplies the other exit).
    pub(crate) fn emit_if_early_return(&mut self, condition: &Expression, then_body: &[Statement], return_type: Type) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
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
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
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
    pub(crate) fn try_non_leaf_if_first_early_return(&mut self, function: &Function) -> Compilation<bool> {
        // Shape: `if (c) { body…; return; } continuation…`, the if first, non-leaf,
        // no guards/locals, no else. The general/void return type only (a float
        // early return adds the FP result register — deferred).
        let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() else {
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
        if returns_value != early_value.is_some() || returns_value != function.return_expression.is_some() {
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
        if then_calls && early_value.as_ref().is_some_and(|value| constant_value(value).is_none()) {
            return Ok(false);
        }
        if rest_calls && function.return_expression.as_ref().is_some_and(|value| constant_value(value).is_none()) {
            return Ok(false);
        }

        let result = Eabi::general_result().number;
        self.non_leaf = true;
        self.frame_size = 16;
        // The if's branch labels advance mwcc's anonymous-`@N` counter by 2.
        self.output.anonymous_label_bump = 2;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        // A BARE void early return (`if (a) return; g();`) has no then-body at all:
        // mwcc folds it to a single INVERTED conditional branch straight to the shared
        // epilogue — `bne EPILOGUE; bl g; EPILOGUE:` — rather than a skip over an
        // unconditional branch.
        if leading.is_empty() && early_value.is_none() {
            let epilogue_branch = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward {
                options: options ^ 8,
                condition_bit,
                target: 0,
            });
            for statement in rest {
                self.emit_statement(statement)?;
            }
            let epilogue_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[epilogue_branch] {
                *target = epilogue_label;
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        // False path skips the then-body to the continuation.
        let continuation_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
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
        self.output.instructions.push(Instruction::Branch { target: 0 });
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
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[continuation_branch] {
                *target = epilogue_label;
            }
        } else {
            // A non-empty continuation: the false path lands on it, and the early
            // return branches over it to the shared epilogue.
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[continuation_branch] {
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

    /// Emit a sequence of `if (c) return v;` guards followed by the final return.
    /// Each guard is its own block ending in `blr`; the last guard collapses the
    /// final return into a conditional return when the final value already sits in
    /// the result register.
    /// FLOAT PARAM REASSIGNMENT: `if (c) { x = -x; } return <expr of x>;` —
    /// the live float stays IN ITS PARAM REGISTER (an in-place fneg; measured,
    /// and `double t = x; if (c) t = -x;` canonicalizes identically). The
    /// bare-copy local aliases to the param when the param is otherwise dead.
    pub(crate) fn try_float_param_reassign(&mut self, function: &Function) -> Compilation<bool> {
        // The only "calls" allowed are the __fabs INTRINSIC in the arms
        // (a single fabs instruction, not a real call — checked per arm below).
        let has_real_call = function.return_expression.as_ref().is_some_and(crate::analysis::expression_has_call)
            || function.locals.iter().any(|local| local.initializer.as_ref().is_some_and(crate::analysis::expression_has_call));
        if !matches!(function.return_type, Type::Float | Type::Double)
            || function.return_expression.is_none()
            || !function.guards.is_empty()
            || has_real_call
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // An optional single bare-copy local (`double t = x;`) aliases to the
        // param; more locals are outside this slice.
        let mut alias: Option<(&str, &str)> = None;
        match function.locals.as_slice() {
            [] => {}
            [local]
                if matches!(local.declared_type, Type::Float | Type::Double)
                    && !local.is_static
                    && local.array_length.is_none() =>
            {
                let Some(Expression::Variable(source)) = &local.initializer else { return Ok(false) };
                if self.float_register_of(source).is_err() {
                    return Ok(false);
                }
                alias = Some((local.name.as_str(), source.as_str()));
            }
            _ => return Ok(false),
        }
        fn resolve<'a>(alias: Option<(&'a str, &'a str)>, name: &'a str) -> &'a str {
            match alias {
                Some((local, source)) if local == name => source,
                _ => name,
            }
        }
        // Statements: `if (int-param cmp const) { fparam = -fparam; }` runs.
        let mut reassigns: Vec<(&Expression, &str, bool)> = Vec::new();
        for statement in &function.statements {
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() || then_body.len() != 1 {
                return Ok(false);
            }
            let condition_ok = match condition {
                Expression::Variable(name) => self.lookup_general(name).is_some(),
                Expression::Binary { left, right, .. } => {
                    matches!(left.as_ref(), Expression::Variable(name) if self.lookup_general(name).is_some())
                        && constant_value(right).is_some()
                }
                _ => false,
            };
            if !condition_ok {
                return Ok(false);
            }
            let Statement::Assign { name, value } = &then_body[0] else { return Ok(false) };
            let target = resolve(alias, name);
            // `x = -x` (fneg) or `x = __fabs(x)` (the fabs instruction).
            let (source, is_abs) = match value {
                Expression::Unary { operator: UnaryOperator::Negate, operand } => match operand.as_ref() {
                    Expression::Variable(source) => (source, false),
                    _ => return Ok(false),
                },
                Expression::Call { name: callee, arguments } if is_intrinsic_call(callee) => match arguments.as_slice() {
                    [Expression::Variable(source)] => (source, true),
                    _ => return Ok(false),
                },
                _ => return Ok(false),
            };
            if resolve(alias, source) != target || self.float_register_of(target).is_err() {
                return Ok(false);
            }
            reassigns.push((condition, target, is_abs));
        }
        if reassigns.is_empty() {
            return Ok(false);
        }
        // The aliased param must not be read under its own name afterwards
        // (the alias takes the register over).
        let return_expression = function.return_expression.as_ref().expect("gated");
        if let Some((local, source)) = alias {
            if count_name_occurrences(return_expression, source) > 0 {
                return Ok(false);
            }
            let register = self.float_register_of(source).expect("checked");
            self.locations.insert(local.to_string(), crate::generator::Location {
                class: crate::generator::ValueClass::Float,
                register,
                signed: true,
                width: if function.return_type == Type::Float { 32 } else { 64 },
                pointee: None,
                stride: None,
            });
        }
        // Each if's join label advances mwcc's anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2 * reassigns.len() as u32;
        for (condition, target, is_abs) in &reassigns {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            let register = self.float_register_of(target).expect("checked");
            self.output.instructions.push(if *is_abs {
                Instruction::FloatAbsolute { d: register, b: register }
            } else {
                Instruction::FloatNegate { d: register, b: register }
            });
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
        }
        let result = Eabi::float_result().number;
        self.evaluate_tail(return_expression, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// LIVE-ACROSS-BRANCHES: initialized int locals reassigned inside simple
    /// if-blocks, read after the joins (the s_atan `id`/`x` skeleton). The
    /// measured model: the condition's cmpwi leads; the inits compute
    /// SPECULATIVELY before the branch; every definition of one local shares
    /// ONE register home — r0 first unless a later use forbids it (an addi
    /// source), else the condition's DYING register, else a free volatile —
    /// and the trailing return/guards consume the locals as pseudo-params.
    pub(crate) fn try_live_across_branches(&mut self, function: &Function) -> Compilation<bool> {
        let int_return = function.return_type == Type::Int && function.return_expression.is_some();
        let void_stores = function.return_type == Type::Void && function.return_expression.is_none();
        if !(int_return || void_stores)
            || function_makes_call(function)
            || function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        if void_stores && !function.guards.is_empty() {
            return Ok(false);
        }
        // Trailing guards (`if (id < 0) return a;` — the id-tested-later form)
        // are allowed: their conditions/values may read the live locals, which
        // resolve through the registered home locations below.
        for guard in &function.guards {
            if !matches!(&guard.condition, Expression::Variable(_) | Expression::Binary { .. }) {
                return Ok(false);
            }
        }
        // Every local: int, initialized, non-static.
        if function.locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || local.initializer.is_none()
                || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
        }) {
            return Ok(false);
        }
        // The statements: a run of `if (param <cmp> const) { local = value; ... }`
        // blocks (no else), reassigning ONLY the declared locals.
        let local_names: Vec<&str> = function.locals.iter().map(|local| local.name.as_str()).collect();
        let simple_value = |expression: &Expression| -> bool {
            let readable = |name: &str| self.lookup_general(name).is_some() || local_names.contains(&name);
            match expression {
                Expression::IntegerLiteral(value) => i16::try_from(*value).is_ok(),
                Expression::Variable(name) => readable(name.as_str()),
                Expression::Binary { operator, left, right } => {
                    matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply)
                        && matches!(left.as_ref(), Expression::Variable(name) if readable(name.as_str()))
                        && matches!(right.as_ref(), Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok())
                }
                _ => false,
            }
        };
        // A VOID body: a run of ifs then TRAILING STORES to distinct SDA int
        // globals (the tail — DAG-scheduled below with the live locals as
        // pseudo-params).
        let mut tail_stores: Vec<&Statement> = Vec::new();
        let mut branch_conditions: Vec<&Expression> = Vec::new();
        for statement in &function.statements {
            if let Statement::Store { target, value } = statement {
                if !void_stores {
                    return Ok(false);
                }
                let Expression::Variable(global) = target else { return Ok(false) };
                if !matches!(self.globals.get(global.as_str()), Some(Type::Int | Type::UnsignedInt)) {
                    return Ok(false);
                }
                if !simple_value(value) {
                    return Ok(false);
                }
                tail_stores.push(statement);
                continue;
            }
            if !tail_stores.is_empty() {
                // A branch after the tail began — outside this slice.
                return Ok(false);
            }
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() {
                return Ok(false);
            }
            // The condition: a bare param, or param <cmp> constant.
            let condition_param = match condition {
                Expression::Variable(name) => Some(name.as_str()),
                Expression::Binary { left, right, .. } => match (left.as_ref(), constant_value(right)) {
                    (Expression::Variable(name), Some(_)) => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            };
            let Some(condition_param) = condition_param else { return Ok(false) };
            if self.lookup_general(condition_param).is_none() || local_names.contains(&condition_param) {
                return Ok(false);
            }
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { return Ok(false) };
                if !local_names.contains(&name.as_str()) || !simple_value(value) {
                    return Ok(false);
                }
            }
            branch_conditions.push(condition);
        }
        if branch_conditions.is_empty() || (void_stores && tail_stores.is_empty()) {
            return Ok(false);
        }
        // Init values must be simple too.
        for local in &function.locals {
            if !simple_value(local.initializer.as_ref().expect("gated")) {
                return Ok(false);
            }
        }
        // HOME SELECTION. A use as an addi source forbids r0: an init/arm value
        // `local <op> const` reading the local, the return expression adding a
        // constant to it, or a tail store's value doing the same.
        let forbids_r0 = |name: &str| -> bool {
            let reads_as_addi = |expression: &Expression| -> bool {
                matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                    if matches!(left.as_ref(), Expression::Variable(inner) if inner == name) && constant_value(right).is_some())
            };
            if function.return_expression.as_ref().is_some_and(&reads_as_addi) {
                return true;
            }
            if tail_stores.iter().any(|statement| matches!(statement, Statement::Store { value, .. } if reads_as_addi(value))) {
                return true;
            }
            function.statements.iter().any(|statement| {
                let Statement::If { then_body, .. } = statement else { return false };
                then_body.iter().any(|inner| matches!(inner, Statement::Assign { value, .. } if reads_as_addi(value)))
            })
        };
        // Dying condition registers: a condition param never referenced later.
        let mut dying_condition_registers: Vec<u8> = Vec::new();
        for condition in &branch_conditions {
            let param = match condition {
                Expression::Variable(name) => name.as_str(),
                Expression::Binary { left, .. } => match left.as_ref() {
                    Expression::Variable(name) => name.as_str(),
                    _ => continue,
                },
                _ => continue,
            };
            let uses_elsewhere = function.return_expression.as_ref().map_or(0, |expression| count_name_occurrences(expression, param))
                + function
                    .statements
                    .iter()
                    .map(|statement| statement_reads(statement, param))
                    .sum::<usize>()
                > branch_conditions
                    .iter()
                    .filter(|other| {
                        matches!(other, Expression::Variable(name) if name == param)
                            || matches!(other, Expression::Binary { left, .. } if matches!(left.as_ref(), Expression::Variable(name) if name == param))
                    })
                    .count();
            if !uses_elsewhere {
                if let Some(register) = self.lookup_general(param) {
                    dying_condition_registers.push(register);
                }
            }
        }
        let mut homes: Vec<(String, u8)> = Vec::new();
        let mut taken: Vec<u8> = Vec::new();
        for local in &function.locals {
            // In a VOID body, r0 belongs to the LAST tail chain: the local may
            // take it only when it IS that chain's value (stored bare by the
            // final store, read nowhere else in the tail).
            let r0_ok = if void_stores {
                let last_is_bare_self = matches!(
                    tail_stores.last(),
                    Some(Statement::Store { value: Expression::Variable(name), .. }) if *name == local.name
                );
                let tail_reads: usize = tail_stores
                    .iter()
                    .map(|statement| statement_reads(statement, &local.name))
                    .sum();
                last_is_bare_self && tail_reads == 1 && !forbids_r0(&local.name)
            } else {
                !forbids_r0(&local.name)
            };
            let candidates: Vec<u8> = if !r0_ok {
                dying_condition_registers.iter().copied().chain(5..=12).collect()
            } else {
                std::iter::once(0u8).chain(dying_condition_registers.iter().copied()).chain(5..=12).collect()
            };
            let Some(register) = candidates.into_iter().find(|register| !taken.contains(register)) else {
                return Ok(false);
            };
            taken.push(register);
            homes.push((local.name.clone(), register));
        }
        let home_of = |name: &str| homes.iter().find(|(local, _)| local == name).map(|&(_, register)| register);

        // EMISSION. First branch: cmpwi, speculative inits, branch; later
        // branches: cmpwi, branch, arm. Each if's join label advances mwcc's
        // anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2 * branch_conditions.len() as u32;
        for (index, statement) in function.statements.iter().enumerate() {
            // Tail stores emit after the branch structure.
            let Statement::If { condition, then_body, .. } = statement else { break };
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if index == 0 {
                for local in &function.locals {
                    let home = home_of(&local.name).expect("assigned");
                    self.evaluate(local.initializer.as_ref().expect("gated"), Type::Int, home)?;
                }
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { unreachable!() };
                // A reassignment may read the local itself (its home).
                let home = home_of(name).expect("assigned");
                self.evaluate_with_live_locals(value, home, &homes)?;
            }
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
        }
        // The trailing return consumes the locals as pseudo-params.
        for (name, register) in &homes {
            self.locations.insert(name.clone(), crate::generator::Location {
                class: crate::generator::ValueClass::General,
                register: *register,
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            });
        }
        if void_stores {
            // The tail: a single bare-local store emits directly; a richer run
            // routes through the DAG store-fill with the live locals as
            // PSEUDO-PARAMS (their homes registered above resolve through
            // lookup_general like any parameter).
            if let [Statement::Store { target: Expression::Variable(global), value: Expression::Variable(name) }] =
                tail_stores.as_slice()
            {
                let source = self.lookup_general(name).expect("registered home");
                self.record_relocation(RelocationKind::EmbSda21, global);
                self.output.instructions.push(Instruction::StoreWord { s: source, a: 0, offset: 0 });
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            let mut pseudo = function.parameters.clone();
            for (name, _) in &homes {
                pseudo.push(mwcc_syntax_trees::Parameter { parameter_type: Type::Int, name: name.clone() });
            }
            let synthesized = Function {
                return_type: Type::Void,
                section: None,
                asm_body: None, force_active: false,
                name: function.name.clone(),
                is_static: function.is_static,
                is_weak: function.is_weak,
                parameters: pseudo,
                locals: Vec::new(),
                statements: tail_stores.iter().map(|&statement| statement.clone()).collect(),
                guards: Vec::new(),
                return_expression: None,
            };
            if !self.try_dag_store_fill(&synthesized)? {
                return Err(Diagnostic::error("a live-across store tail outside the DAG envelope needs more vocabulary (roadmap)"));
            }
            return Ok(true);
        }
        let return_expression = function.return_expression.as_ref().expect("gated");
        let result = Eabi::general_result().number;
        if function.guards.is_empty() {
            self.evaluate_tail(return_expression, Type::Int, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        } else {
            self.emit_guard_sequence(&function.guards, return_expression, Type::Int, result)?;
        }
        Ok(true)
    }

    pub(crate) fn emit_guard_sequence(
        &mut self,
        guards: &[GuardedReturn],
        final_return: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let final_in_result = match final_return {
            Expression::Variable(name) => self.locations.get(name).map(|location| location.register) == Some(result),
            _ => false,
        };

        // mwcc reuses one `cmpwi` across consecutive guards that test the same operand against the
        // same constant: `if (a < 0) ...; if (a == 0) ...` shares `cmpwi r3,0`, the second guard
        // branching on the same result (`bne`). That cross-guard condition-register reuse is not
        // modeled — each guard here emits its own compare — so a sequence containing such a pair
        // would emit a redundant second `cmpwi` (a byte diff). Defer it rather than ship that.
        let guard_count = guards.len();
        for (pair_index, pair) in guards.windows(2).enumerate() {
            if let (Some(first), Some(second)) =
                (guard_comparison_key(&pair[0].condition), guard_comparison_key(&pair[1].condition))
            {
                if first == second {
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
                    if second_is_last && (!final_in_result || constant_value(&pair[1].value).is_some()) {
                        let select = guard_select(&pair[1].condition, &pair[1].value, final_return);
                        if let Expression::Conditional { condition, when_true, when_false } = &select {
                            if crate::control_flow::select_folds_branchless(condition, when_true, when_false) {
                                continue;
                            }
                        }
                    }
                    return Err(Diagnostic::error("consecutive guards sharing a compare need cross-guard CR reuse (roadmap)"));
                }
            }
        }

        for (index, guard) in guards.iter().enumerate() {
            let is_last = index + 1 == guards.len();

            // A null-guarded dereference `if (!p) return CONST; return *p;` cannot fold branchless
            // (dereferencing null is unsafe); mwcc emits a real branch with the deref in the
            // fall-through and the constant as the cold tail: `cmplwi p,0; beq COLD; <*p>; blr;
            // COLD: li CONST; blr`.
            if is_last {
                if let Some((pointer, hot, cold)) = guarded_null_dereference(&guard.condition, &guard.value, final_return, return_type) {
                    if let Some(pointer_register) = self.lookup_general(pointer) {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: pointer_register, immediate: 0 });
                        let branch_index = self.output.instructions.len();
                        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 });
                        self.evaluate_tail(hot, return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        let cold_label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                            *target = cold_label;
                        }
                        self.evaluate_tail(cold, return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
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
                let select = guard_select(&guard.condition, &guard.value, final_return);
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
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
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

            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            // A (non-last) guard with a CONSTANT value: forward-branch past the return when the
            // condition is false, load the constant into the result, and return —
            // `cmpwi; bge skip; li result, c; blr; skip:`. (A constant has no leaf register, so the
            // leaf paths below would defer at general_register_of_leaf.)
            if let Some(constant) = constant_value(&guard.value) {
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.load_integer_constant(result, constant);
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                let next = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = next;
                }
                continue;
            }
            let value_register = self.general_register_of_leaf(&guard.value)?;

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                if result != value_register {
                    self.output.instructions.push(Instruction::move_register(result, value_register));
                }
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            // A non-last guard whose value already sits in the result register is a
            // conditional return falling through to the next guard (mwcc: `cmpwi; bnelr`),
            // not a forward branch over the return.
            if result == value_register {
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                continue;
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.output.instructions.push(Instruction::move_register(result, value_register));
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let next = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = next;
            }
        }

        // Final fall-through return.
        self.evaluate_tail(final_return, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }

}

/// Whether two conditions are relational comparisons of the SAME operand against the
/// SAME value (`c > 0` and `c < 0`, both `cmpwi r3,0`). mwcc emits ONE compare and reads
/// its condition register from both branches; our per-branch re-compare would emit a
/// redundant second `cmpwi`, so the else-if chain defers when this holds.
fn shares_condition_register(a: &Expression, b: &Expression) -> bool {
    let relational = |operator: &BinaryOperator| {
        matches!(
            operator,
            BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::LessEqual
                | BinaryOperator::GreaterEqual | BinaryOperator::Equal | BinaryOperator::NotEqual
        )
    };
    match (a, b) {
        (
            Expression::Binary { operator: operator_a, left: left_a, right: right_a },
            Expression::Binary { operator: operator_b, left: left_b, right: right_b },
        ) if relational(operator_a) && relational(operator_b) => {
            same_operand(left_a, left_b) && same_operand(right_a, right_b)
        }
        _ => false,
    }
}
