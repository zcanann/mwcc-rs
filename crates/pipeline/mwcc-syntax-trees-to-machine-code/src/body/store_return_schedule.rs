//! Cross-statement scheduling between a pointer store and its trailing return.
//!
//! Build 163 moves the first independent comparison instruction ahead of a
//! single `*p = value` store. This pass runs on virtual instructions before
//! allocation, so the allocator observes the extended live range and selects
//! the same non-pointer temporary as mwcc.

use super::*;

pub(super) struct GlobalConstantStoreReturnPlan {
    pub(super) statement_start: usize,
    stores: Vec<(String, Pointee)>,
    stored: i16,
    returned: i16,
}

impl Generator {
    /// Recognize a terminal run of integer-global stores sharing one small
    /// constant, followed by a small constant return. MWCC materializes the
    /// shared store value once. The linkage-first pipeline uses the final
    /// store's latency slot for the independent return value; the predecrement
    /// pipeline hoists that value before the whole store run.
    pub(super) fn global_constant_store_return_plan(
        &self,
        function: &Function,
    ) -> Option<GlobalConstantStoreReturnPlan> {
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return None;
        }
        let returned = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())?;
        let mut statement_start = function.statements.len();
        let mut stores = Vec::new();
        let mut shared_constant = None;
        while let Some(Statement::Store {
            target: Expression::Variable(global),
            value,
        }) = statement_start
            .checked_sub(1)
            .and_then(|index| function.statements.get(index))
        {
            let stored = constant_value(value).and_then(|value| i16::try_from(value).ok())?;
            if shared_constant.is_some_and(|shared| shared != stored) {
                return None;
            }
            let pointee = self
                .globals
                .get(global.as_str())
                .copied()
                .and_then(pointee_of_type)?;
            if matches!(pointee, Pointee::Float | Pointee::Double) {
                return None;
            }
            shared_constant = Some(stored);
            stores.push((global.clone(), pointee));
            statement_start -= 1;
        }
        if stores.is_empty() {
            return None;
        }
        stores.reverse();
        Some(GlobalConstantStoreReturnPlan {
            statement_start,
            stores,
            stored: shared_constant.expect("a nonempty store run has a constant"),
            returned,
        })
    }

    pub(super) fn emit_global_constant_store_return_plan(
        &mut self,
        plan: GlobalConstantStoreReturnPlan,
    ) -> Compilation<()> {
        self.load_integer_constant(GENERAL_SCRATCH, i64::from(plan.stored));
        let result_instruction = Instruction::AddImmediate {
            d: Eabi::general_result().number,
            a: 0,
            immediate: plan.returned,
        };
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output.instructions.push(result_instruction);
            for (global, pointee) in &plan.stores {
                self.emit_global_store(global, *pointee, GENERAL_SCRATCH)?;
            }
        } else {
            let (final_store, preceding_stores) = plan
                .stores
                .split_last()
                .expect("a store-return plan has at least one store");
            for (global, pointee) in preceding_stores {
                self.emit_global_store(global, *pointee, GENERAL_SCRATCH)?;
            }
            self.output.instructions.push(result_instruction);
            self.emit_global_store(&final_store.0, final_store.1, GENERAL_SCRATCH)?;
        }
        Ok(())
    }

    /// `g = enter(); if (g == 0) { a(); b(); } return g = leave();`.
    ///
    /// This is one cross-statement region: legacy 2.3.3 keeps the absolute
    /// address produced for the first store, updates it with `stwu`, and reloads
    /// the condition through that base. The 2.4.x scheduler instead compares the
    /// still-live call result before issuing the store. Selection emits a virtual
    /// address base; ordinary liveness allocation chooses r4 beside the r3 result.
    pub(super) fn try_global_call_store_guard_tail(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
            || self.behavior.global_addressing != GlobalAddressing::Absolute
            || self.behavior.absolute_access_style
                != mwcc_versions::AbsoluteAccessStyle::FoldedDisplacement
        {
            return Ok(false);
        }

        let [Statement::Store {
            target: Expression::Variable(global),
            value:
                Expression::Call {
                    name: enter,
                    arguments: enter_arguments,
                },
        }, Statement::If {
            condition:
                Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left,
                    right,
                },
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some(Expression::Assign {
            target: return_target,
            value: return_value,
        }) = function.return_expression.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(return_global) = return_target.as_ref() else {
            return Ok(false);
        };
        let Expression::Call {
            name: leave,
            arguments: leave_arguments,
        } = return_value.as_ref()
        else {
            return Ok(false);
        };
        if return_global != global
            || !matches!(left.as_ref(), Expression::Variable(name) if name == global)
            || !is_zero_literal(right)
            || !else_body.is_empty()
            || then_body.is_empty()
            || !enter_arguments.is_empty()
            || !leave_arguments.is_empty()
            || then_body.iter().any(|statement| {
                !matches!(statement, Statement::Expression(Expression::Call { arguments, .. }) if arguments.is_empty())
            })
            || self.globals.get(global.as_str()).copied() != Some(function.return_type)
        {
            return Ok(false);
        }
        let Some(pointee @ (Pointee::Int | Pointee::UnsignedInt)) =
            pointee_of_type(function.return_type)
        else {
            return Ok(false);
        };

        self.emit_plain_nonleaf_prologue();
        let result = Eabi::general_result().number;
        self.emit_call(enter, enter_arguments, Some(result), false)?;

        let address = self.fresh_virtual_general_preferring(4);
        self.emit_address_high(address, global);
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => {
                self.record_relocation(RelocationKind::Addr16Lo, global);
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: result,
                        a: address,
                        offset: 0,
                    });
                self.output.instructions.push(Instruction::LoadWord {
                    d: GENERAL_SCRATCH,
                    a: address,
                    offset: 0,
                });
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate {
                        a: GENERAL_SCRATCH,
                        immediate: 0,
                    });
            }
            FrameConvention::Predecrement => {
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate {
                        a: result,
                        immediate: 0,
                    });
                self.record_relocation(RelocationKind::Addr16Lo, global);
                self.output.instructions.push(Instruction::StoreWord {
                    s: result,
                    a: address,
                    offset: 0,
                });
            }
        }

        let after_guard = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, after_guard);
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        self.bind_label(after_guard);

        self.emit_call(leave, leave_arguments, Some(result), false)?;
        self.emit_global_store(global, pointee, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `if (g) return E; g = S; return R;` for integer constants. The guarded
    /// return is emitted as a forward branch, while the continuation schedules
    /// the independent final return value into the constant-store latency slot:
    /// `li r0,S; li r3,R; stw r0,g; blr` on the modern pipeline.
    pub(super) fn try_guarded_global_constant_store_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !function.locals.is_empty()
            || function_makes_call(function)
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [Statement::If {
            condition: Expression::Variable(condition_global),
            then_body,
            else_body,
        }, Statement::Store {
            target: Expression::Variable(store_global),
            value: stored,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Statement::Return(Some(guard_value))] = then_body.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() || condition_global != store_global {
            return Ok(false);
        }
        let Some(global_type) = self.globals.get(store_global.as_str()).copied() else {
            return Ok(false);
        };
        let Some(pointee) = pointee_of_type(global_type) else {
            return Ok(false);
        };
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            return Ok(false);
        }
        let Some(guard_constant) = constant_value(guard_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some(store_constant) = constant_value(stored)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some(return_constant) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };

        let (options, condition_bit) =
            self.emit_condition_test(&Expression::Variable(condition_global.clone()))?;
        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.output
            .instructions
            .push(Instruction::AddImmediate {
                d: Eabi::general_result().number,
                a: 0,
                immediate: guard_constant,
            });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let continuation = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[branch_index]
        {
            *target = continuation;
        }

        self.output
            .instructions
            .push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: 0,
                immediate: store_constant,
            });
        let return_instruction = Instruction::AddImmediate {
            d: Eabi::general_result().number,
            a: 0,
            immediate: return_constant,
        };
        if self.behavior.guard_store_precedes_return_value {
            self.emit_global_store(store_global, pointee, GENERAL_SCRATCH)?;
            self.output.instructions.push(return_instruction);
        } else {
            self.output.instructions.push(return_instruction);
            self.emit_global_store(store_global, pointee, GENERAL_SCRATCH)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(super) fn schedule_legacy_single_pointer_store_return(
        &mut self,
        function: &Function,
        statements_start: usize,
        return_start: usize,
    ) {
        if self.behavior.integer_comparison_value_style
            != IntegerComparisonValueStyle::LegacyCarryChain
            || statements_start >= return_start
            || return_start >= self.output.instructions.len()
        {
            return;
        }

        let [Statement::Store {
            target: Expression::Dereference { .. },
            value: Expression::Variable(stored),
        }] = function.statements.as_slice()
        else {
            return;
        };
        let Some(Expression::Binary {
            operator,
            left,
            right,
        }) = function.return_expression.as_ref()
        else {
            return;
        };
        if !matches!(
            operator,
            BinaryOperator::Less | BinaryOperator::GreaterEqual | BinaryOperator::Equal
        ) || !is_zero_literal(right)
            || !matches!(left.as_ref(), Expression::Variable(name) if name == stored)
        {
            return;
        }

        let first = &self.output.instructions[return_start];
        let independent_preamble = match operator {
            BinaryOperator::Less | BinaryOperator::GreaterEqual => {
                matches!(
                    first,
                    Instruction::AddImmediate {
                        a: 0,
                        immediate: 0,
                        ..
                    }
                )
            }
            BinaryOperator::Equal => matches!(first, Instruction::Negate { .. }),
            _ => false,
        };
        if independent_preamble {
            let instruction = self.output.instructions.remove(return_start);
            self.output
                .instructions
                .insert(statements_start, instruction);
        }
    }
}
