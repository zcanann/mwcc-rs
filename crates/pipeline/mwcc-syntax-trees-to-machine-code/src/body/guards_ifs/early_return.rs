//! Ordered early-return branches: guarded global-array stores and if(cond) return v; <store> tails.

#[allow(unused_imports)]
use super::*;

impl Generator {
    fn emit_ordered_early_return_with_tracked_tail(
        &mut self,
        function: &Function,
        condition: &Expression,
        value: &Expression,
        tail: &[Statement],
    ) -> Compilation<()> {
        let result = mwcc_target::Eabi::general_result().number;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if matches!(value, Expression::Variable(name) if self.lookup_general(name) == Some(result))
        {
            // A value already in r3 reduces the source diamond to a conditional return.
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: options ^ 8,
                    condition_bit,
                });
        } else {
            let branch_index = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.evaluate_tail(value, function.return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let continuation = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[branch_index]
            {
                *target = continuation;
            }
        }

        let reduced = Function {
            statements: tail.to_vec(),
            ..function.clone()
        };
        if !self.try_value_tracking(&reduced)? {
            return Err(Diagnostic::error("an ordered early-return continuation outside the value-tracking shape is not supported yet (roadmap)"));
        }
        Ok(())
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
        if let Expression::Binary {
            operator,
            left,
            right,
        } = index
        {
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
        let stored_constant =
            constant_value(stored).and_then(|constant| i16::try_from(constant).ok());
        let stored_register = if stored_constant.is_none() {
            let Expression::Variable(name) = stored else {
                return Ok(false);
            };
            let Some(register) = self.lookup_general(name) else {
                return Ok(false);
            };
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
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.evaluate_tail(guard_value, function.return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let continuation = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_index]
        {
            *target = continuation;
        }

        let index_register = self.general_register_of_leaf(index_leaf)?;
        let shift = size.trailing_zeros() as u8;
        if self.behavior.global_array_index_style
            == mwcc_versions::GlobalArrayIndexStyle::ExplicitAddress
        {
            // Build 163 completes an explicit element address in r3, performs
            // the store sequentially, and only then materializes the constant
            // return. For a zero displacement it scales r3 in place and uses
            // r0 for the low address; a folded displacement keeps the scaled
            // index in r0 and forms the base in r3 before adding it.
            let high = self.fresh_virtual_general();
            self.emit_address_high(high, array);
            if offset == 0 {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: result,
                        s: index_register,
                        shift,
                    });
                self.record_relocation(RelocationKind::Addr16Lo, array);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: GENERAL_SCRATCH,
                    a: high,
                    immediate: 0,
                });
                self.output.instructions.push(Instruction::Add {
                    d: result,
                    a: GENERAL_SCRATCH,
                    b: result,
                });
            } else {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: index_register,
                        shift,
                    });
                self.record_relocation(RelocationKind::Addr16Lo, array);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: result,
                    a: high,
                    immediate: 0,
                });
                self.output.instructions.push(Instruction::Add {
                    d: result,
                    a: result,
                    b: GENERAL_SCRATCH,
                });
            }
            let source = if let Some(register) = stored_register {
                register
            } else {
                self.load_integer_constant(
                    GENERAL_SCRATCH,
                    stored_constant.expect("checked above") as i64,
                );
                GENERAL_SCRATCH
            };
            self.output
                .instructions
                .push(displacement_store(pointee, source, result, offset)?);
            self.load_integer_constant(result, return_constant as i64);
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        if let Some(register) = stored_register {
            // Register value: the base stays OUT of the index register (the return needs
            // r3 live before the store) — `lis B; slwi; addi B,B; li r3,R; stwx v,B,r0`.
            let base = self.fresh_virtual_general();
            self.emit_address_high(base, array);
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index_register,
                    shift,
                });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate {
                d: base,
                a: base,
                immediate: 0,
            });
            self.output.instructions.push(Instruction::AddImmediate {
                d: result,
                a: 0,
                immediate: return_constant,
            });
            self.output
                .instructions
                .push(crate::expressions::indexed_store(
                    pointee,
                    register,
                    base,
                    GENERAL_SCRATCH,
                )?);
        } else {
            let constant = stored_constant.expect("checked above");
            // Phase D: the base-high is a virtual in both forms — a redefined vreg keeps
            // ONE live range spanning the redefinition (the offset≠0 form reuses it for
            // the effective address), so the value's overlapping virtual lands on the
            // next register, matching mwcc's r4/r5 split.
            let high = self.fresh_virtual_general();
            self.emit_address_high(high, array);
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: GENERAL_SCRATCH,
                    s: index_register,
                    shift,
                });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate {
                d: index_register,
                a: high,
                immediate: 0,
            });
            if offset == 0 {
                // The standalone sequence, the return materialized after the store.
                self.output.instructions.push(Instruction::AddImmediate {
                    d: high,
                    a: 0,
                    immediate: constant,
                });
                self.output
                    .instructions
                    .push(crate::expressions::indexed_store(
                        pointee,
                        high,
                        index_register,
                        GENERAL_SCRATCH,
                    )?);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: result,
                    a: 0,
                    immediate: return_constant,
                });
            } else {
                // The value's virtual overlaps the still-live high (which the `add`
                // redefines as the effective address), so it allocates past it.
                let value_register = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::AddImmediate {
                    d: value_register,
                    a: 0,
                    immediate: constant,
                });
                self.output.instructions.push(Instruction::Add {
                    d: high,
                    a: index_register,
                    b: GENERAL_SCRATCH,
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: result,
                    a: 0,
                    immediate: return_constant,
                });
                self.output.instructions.push(displacement_store(
                    pointee,
                    value_register,
                    high,
                    offset,
                )?);
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn try_ordered_early_return_branch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
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
            if let [Statement::If {
                condition,
                then_body,
                else_body,
            }, rest @ ..] = function.statements.as_slice()
            {
                if matches!(then_body.as_slice(), [Statement::Return(None)])
                    && else_body.is_empty()
                    && matches!(rest, [Statement::Store { .. }])
                {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalToLinkRegister {
                            options: options ^ 8,
                            condition_bit,
                        });
                    self.emit_statement(&rest[0])?;
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if !function.guards.is_empty()
            || function.return_type == Type::Void
            || function.return_expression.is_none()
        {
            return Ok(false);
        }
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }, rest @ ..] = function.statements.as_slice()
        else {
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
        if let [Statement::Store {
            target,
            value: stored,
        }] = rest
        {
            if function.guards.is_empty() && function.locals.is_empty() {
                // A computed-index GLOBAL-ARRAY target has its own captured schedules
                // (the address build interleaves with the return) — a dedicated arm.
                if let Expression::Index { base, index } = target {
                    if let Expression::Variable(array) = base.as_ref() {
                        if let Some(&total_size) = self.global_array_sizes.get(array.as_str()) {
                            if constant_value(index).is_none() {
                                return self.try_guarded_global_array_store(
                                    function, condition, value, array, total_size, index, stored,
                                );
                            }
                        }
                    }
                }
                let result = mwcc_target::Eabi::general_result().number;
                let guard_value_in_result = matches!(value, Expression::Variable(name) if self.lookup_general(name) == Some(result));
                // A register-leaf store value (`*p = b`) is already in a register — mwcc stores it
                // DIRECTLY, before the return, with no r0 materialization (`bgtlr; stw r4,0(r5);
                // li r3,0`). Only meaningful when the guard collapses to `bgtlr` (its value already
                // in the result register); otherwise the register-store form stays with value_tracking.
                let stored_register_leaf = if guard_value_in_result {
                    match stored {
                        Expression::Variable(name) => self.lookup_general(name),
                        _ => None,
                    }
                } else {
                    None
                };
                let stored_is_constant = constant_value(stored)
                    .and_then(|constant| i16::try_from(constant).ok())
                    .is_some();
                let stored_is_two_leaf = matches!(stored, Expression::Binary { left, right, .. }
                    if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                        && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
                if stored_register_leaf.is_none() && !stored_is_constant && !stored_is_two_leaf {
                    return Ok(false);
                }
                let (pointer_name, byte_offset, pointee): (&String, i64, Pointee) = match target {
                    Expression::Dereference { pointer } => {
                        let Expression::Variable(name) = pointer.as_ref() else {
                            return Ok(false);
                        };
                        (name, 0, self.pointee_of(pointer)?)
                    }
                    Expression::Index { base, index } => {
                        let Expression::Variable(name) = base.as_ref() else {
                            return Ok(false);
                        };
                        let Some(constant) = constant_value(index) else {
                            return Ok(false);
                        };
                        let pointee = self.pointee_of(base)?;
                        (name, constant * pointee.size() as i64, pointee)
                    }
                    Expression::Member {
                        base,
                        offset,
                        member_type,
                        index_stride: None,
                    } => {
                        let Expression::Variable(name) = base.as_ref() else {
                            return Ok(false);
                        };
                        let Some(pointee) = pointee_of_type(*member_type) else {
                            return Ok(false);
                        };
                        (name, *offset as i64, pointee)
                    }
                    _ => return Ok(false),
                };
                if !function
                    .parameters
                    .iter()
                    .any(|parameter| &parameter.name == pointer_name)
                {
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
                        if let Some(constant) = constant_value(expression)
                            .and_then(|constant| i16::try_from(constant).ok())
                        {
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

                let (options, condition_bit) = self.emit_condition_test(condition)?;
                if guard_value_in_result {
                    // The guard VALUE already occupies the result register (`if(a>0) return a; *p=5;
                    // return 0;`): mwcc collapses the guard to a single conditional branch-to-lr
                    // (`bgtlr`) rather than a forward branch over a no-op value move. `options ^ 8`
                    // inverts the skip-when-false test to return-when-true. The materialized store
                    // value and the return then follow exactly as below (`li r0,5; li r3,0; stw`).
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalToLinkRegister {
                            options: options ^ 8,
                            condition_bit,
                        });
                } else {
                    let branch_index = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalForward {
                            options,
                            condition_bit,
                            target: 0,
                        });
                    self.evaluate_tail(value, function.return_type, result)?;
                    self.output
                        .instructions
                        .push(Instruction::BranchToLinkRegister);
                    let continuation = self.output.instructions.len();
                    if let Instruction::BranchConditionalForward { target, .. } =
                        &mut self.output.instructions[branch_index]
                    {
                        *target = continuation;
                    }
                }
                let emit_return_value = |generator: &mut Self| match return_value {
                    // A constant `li r3,C`; a register `mr r3,reg` (a self-move when the return value
                    // already sits in the result register — coalesced away later).
                    ReturnValue::Constant(constant) => {
                        generator
                            .output
                            .instructions
                            .push(Instruction::AddImmediate {
                                d: result,
                                a: 0,
                                immediate: constant,
                            });
                    }
                    ReturnValue::Register(register) => {
                        generator.output.instructions.push(Instruction::Or {
                            a: result,
                            s: register,
                            b: register,
                        });
                    }
                };
                if let Some(store_register) = stored_register_leaf {
                    // CASE A — the store value is already in a register: store it DIRECTLY (no r0
                    // materialization), then the return (`bgtlr; stw r4,0(r5); li r3,0; blr`).
                    self.output.instructions.push(displacement_store(
                        pointee,
                        store_register,
                        pointer_register,
                        offset,
                    )?);
                    emit_return_value(self);
                } else {
                    // CASE B — materialize the value in r0. Mainline schedules the return
                    // between production and the store; build 163 completes the store first.
                    self.evaluate_general(stored, GENERAL_SCRATCH)?;
                    let store = displacement_store(
                        pointee,
                        GENERAL_SCRATCH,
                        pointer_register,
                        offset,
                    )?;
                    if self.behavior.guard_store_precedes_return_value {
                        self.output.instructions.push(store);
                        emit_return_value(self);
                    } else {
                        emit_return_value(self);
                        self.output.instructions.push(store);
                    }
                }
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            return Ok(false);
        }

        // A single reassignment is the verified continuation shape; longer tails are
        // unverified against mwcc (they may fold or reschedule differently) — defer.
        if !rest
            .iter()
            .all(|statement| matches!(statement, Statement::Assign { .. }))
        {
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
        let reads_written = |expression: &Expression| {
            written
                .iter()
                .any(|name| expression_reads_name(expression, name))
        };
        // A guard VALUE reading a reassigned name is unverified — defer.
        if reads_written(value) {
            return Ok(false);
        }
        let tail_reads_parameter = |name: &str| {
            rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, name),
                _ => false,
            }) || function
                .return_expression
                .as_ref()
                .is_some_and(|ret| expression_reads_name(ret, name))
        };
        let distinct_parameter_reads = function
            .parameters
            .iter()
            .filter(|parameter| tail_reads_parameter(&parameter.name))
            .count();

        // The legacy branch-preserving pipeline keeps an ordered early return ahead
        // of its continuation, including when the condition reads the parameter
        // reassigned by that continuation: the compare precedes the write. If the
        // guarded value is already in r3 this is a conditional return
        // (`b<true>lr`); otherwise it is the literal source diamond followed by
        // the independently compiled value-tracked tail.
        if self.behavior.integer_select_style
            == mwcc_versions::IntegerSelectStyle::BranchPreserving
        {
            self.emit_ordered_early_return_with_tracked_tail(
                function, condition, value, rest,
            )?;
            return Ok(true);
        }

        // The branch form is mwcc's shape for a tail reading TWO-plus distinct parameters
        // (`add r3,r4,r5` after the branch), with a condition reading no reassigned name.
        if distinct_parameter_reads >= 2 && !reads_written(condition) {
            self.emit_ordered_early_return_with_tracked_tail(
                function, condition, value, rest,
            )?;
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
            let [Statement::Assign {
                name: assigned,
                value: assigned_value,
            }] = rest
            else {
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
            let Expression::Variable(value_name) = value else {
                return Ok(false);
            };
            let Some(value_register) = self.lookup_general(value_name) else {
                return Ok(false);
            };
            let result = mwcc_target::Eabi::general_result().number;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.evaluate_tail(assigned_value, function.return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options,
                    condition_bit,
                });
            self.output.instructions.push(Instruction::Or {
                a: result,
                s: value_register,
                b: value_register,
            });
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        Ok(false)
    }
}
