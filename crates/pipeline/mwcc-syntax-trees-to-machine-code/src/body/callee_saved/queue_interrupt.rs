//! Interrupt-side composition of callback arbitration and verified queue helpers.

use super::fixed_rmw_recognize::peel_casts;
#[allow(unused_imports)]
use super::*;

struct CallbackArm<'a> {
    callback: &'a str,
    pending: &'a str,
}

fn is_null(expression: &Expression) -> bool {
    constant_value(peel_casts(expression)) == Some(0)
}

fn variable_through_casts(expression: &Expression) -> Option<&str> {
    match peel_casts(expression) {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn callback_arm<'a>(condition: &'a Expression, body: &'a [Statement]) -> Option<CallbackArm<'a>> {
    let Expression::Variable(callback) = condition else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: called,
        arguments,
    }), Statement::Store {
        target: Expression::Variable(cleared_pending),
        value: pending_zero,
    }, Statement::Store {
        target: Expression::Variable(cleared_callback),
        value: callback_zero,
    }] = body
    else {
        return None;
    };
    let [argument] = arguments.as_slice() else {
        return None;
    };
    let pending = variable_through_casts(argument)?;
    (called == callback
        && cleared_pending == pending
        && cleared_callback == callback
        && is_null(pending_zero)
        && is_null(callback_zero))
    .then_some(CallbackArm { callback, pending })
}

fn empty_direct_call(statement: &Statement) -> Option<&str> {
    match statement {
        Statement::Expression(Expression::Call { name, arguments }) if arguments.is_empty() => {
            Some(name)
        }
        _ => None,
    }
}

fn null_comparison(expression: &Expression) -> Option<&str> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = expression
    else {
        return None;
    };
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), other) if is_null(other) => Some(name),
        (other, Expression::Variable(name)) if is_null(other) => Some(name),
        _ => None,
    }
}

impl Generator {
    fn emit_indirect_queue_callback_call(&mut self) {
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.output
                    .instructions
                    .push(Instruction::MoveToCountRegister { s: 12 });
                self.output
                    .instructions
                    .push(Instruction::BranchToCountRegisterAndLink);
            }
            FrameConvention::LinkageFirst => {
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 12 });
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegisterAndLink);
            }
        }
    }

    /// Emit a callback arbiter followed by two semantically verified helpers
    /// that mwcc inlines into the same non-leaf schedule.
    pub(crate) fn try_inlined_queue_interrupt_service(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [arbitrate, pop_call, service_guard] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Statement::If {
            condition: high_condition,
            then_body: high_body,
            else_body: high_else,
        } = arbitrate
        else {
            return Ok(false);
        };
        let [Statement::If {
            condition: low_condition,
            then_body: low_body,
            else_body: low_else,
        }] = high_else.as_slice()
        else {
            return Ok(false);
        };
        if !low_else.is_empty() {
            return Ok(false);
        }
        let Some(high) = callback_arm(high_condition, high_body) else {
            return Ok(false);
        };
        let Some(low) = callback_arm(low_condition, low_body) else {
            return Ok(false);
        };
        let Some(pop_name) = empty_direct_call(pop_call) else {
            return Ok(false);
        };
        let Statement::If {
            condition: final_condition,
            then_body: final_body,
            else_body: final_else,
        } = service_guard
        else {
            return Ok(false);
        };
        let [service_call] = final_body.as_slice() else {
            return Ok(false);
        };
        let Some(service_name) = empty_direct_call(service_call) else {
            return Ok(false);
        };
        if !final_else.is_empty() || null_comparison(final_condition) != Some(high.pending) {
            return Ok(false);
        }

        let Some(pop) = self.inline_summaries.queue_pop(pop_name).cloned() else {
            return Ok(false);
        };
        let Some(service) = self.inline_summaries.queue_service(service_name).cloned() else {
            return Ok(false);
        };
        if pop.callback != high.callback
            || pop.pending != high.pending
            || service.callback != low.callback
            || service.pending != low.pending
            || [high.callback, high.pending, low.callback, low.pending]
                .into_iter()
                .any(|name| !self.globals.contains_key(name))
        {
            return Ok(false);
        }

        self.emit_plain_nonleaf_prologue();

        self.emit_global_load(high.callback, 12)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        let low_arm = self.fresh_label();
        let callbacks_done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, low_arm);
        self.emit_global_load(high.pending, 3)?;
        self.emit_indirect_queue_callback_call();
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, high.pending);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, high.callback);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.emit_branch_to(callbacks_done);

        self.bind_label(low_arm);
        self.emit_global_load(low.callback, 12)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        self.emit_branch_conditional_to(12, 2, callbacks_done);
        self.emit_global_load(low.pending, 3)?;
        self.emit_indirect_queue_callback_call();
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, low.pending);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, low.callback);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.bind_label(callbacks_done);

        let pop_done = self.fresh_label();
        self.emit_queue_pop_body(&pop, pop_done);
        self.bind_label(pop_done);

        self.emit_global_load(high.pending, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        let function_end = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, function_end);
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => self.emit_queue_service_body(&service, function_end),
            FrameConvention::LinkageFirst => {
                self.record_relocation(RelocationKind::Rel24, service_name);
                self.output.instructions.push(Instruction::BranchAndLink {
                    target: service_name.to_string(),
                });
            }
        }
        self.bind_label(function_end);

        // Build 163 leaves the service helper out of line, while 2.4.x folds
        // both verified helper CFGs into the interrupt routine.
        self.output.anonymous_label_bump += match self.behavior.frame_convention {
            FrameConvention::Predecrement => 31,
            FrameConvention::LinkageFirst => 13,
        };
        self.pin_queue_helper_post_function_bump();
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
