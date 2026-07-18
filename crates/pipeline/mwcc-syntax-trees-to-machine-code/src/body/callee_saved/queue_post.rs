//! Interrupt-protected queue posting with verified inline service helpers.

use super::fixed_rmw_recognize::peel_casts;
#[allow(unused_imports)]
use super::*;

fn is_null(expression: &Expression) -> bool {
    constant_value(peel_casts(expression)) == Some(0)
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn member_store<'a>(statement: &'a Statement, base: &str) -> Option<(u16, &'a Expression)> {
    let Statement::Store {
        target:
            Expression::Member {
                base: target_base,
                offset,
                index_stride: None,
                ..
            },
        value,
    } = statement
    else {
        return None;
    };
    matches!(target_base.as_ref(), Expression::Variable(name) if name == base)
        .then_some((*offset, value))
}

fn empty_call(statement: &Statement) -> Option<&str> {
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

fn queue_arm<'a>(
    arm: &'a mwcc_syntax_trees::SwitchArm,
    request: &str,
    next_offset: u16,
) -> Option<(&'a str, &'a str)> {
    if arm.falls_through {
        return None;
    }
    let mwcc_syntax_trees::ArmBody::Statements(statements) = &arm.body else {
        return None;
    };
    let [Statement::If {
        condition: Expression::Variable(queue),
        then_body,
        else_body,
    }, Statement::Store {
        target: Expression::Variable(tail_commit),
        value: Expression::Variable(committed_request),
    }] = statements.as_slice()
    else {
        return None;
    };
    let [Statement::Store {
        target:
            Expression::Member {
                base: tail_base,
                offset,
                index_stride: None,
                ..
            },
        value: Expression::Variable(linked_request),
    }] = then_body.as_slice()
    else {
        return None;
    };
    let Expression::Variable(tail) = tail_base.as_ref() else {
        return None;
    };
    let [Statement::Store {
        target: Expression::Variable(empty_queue),
        value: Expression::Variable(queued_request),
    }] = else_body.as_slice()
    else {
        return None;
    };
    (*offset == next_offset
        && linked_request == request
        && empty_queue == queue
        && queued_request == request
        && tail_commit == tail
        && committed_request == request)
        .then_some((queue.as_str(), tail.as_str()))
}

impl Generator {
    /// Initialize a request, append it to the selected queue under an interrupt
    /// token, and inline verified queue-pop/service helpers when the engine is idle.
    pub(crate) fn try_inlined_queue_post_transaction(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [request, owner, request_type, priority, source, dest, length, callback] =
            function.parameters.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(request.parameter_type, Type::StructPointer { .. })
            || ![owner, request_type, priority, source, dest, length]
                .iter()
                .all(|parameter| parameter.parameter_type == Type::UnsignedInt)
            || !matches!(
                callback.parameter_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
        {
            return Ok(false);
        }
        let [enabled] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(enabled.declared_type, Type::Int | Type::UnsignedInt)
            || enabled.initializer.is_some()
            || enabled.array_length.is_some()
            || enabled.is_static
        {
            return Ok(false);
        }
        let [next_store, owner_store, type_store, source_store, dest_store, length_store, callback_choice, disable_assign, priority_switch, idle_service, restore_call] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };

        let Some((next_offset, next_value)) = member_store(next_store, &request.name) else {
            return Ok(false);
        };
        let Some((owner_offset, owner_value)) = member_store(owner_store, &request.name) else {
            return Ok(false);
        };
        let Some((type_offset, type_value)) = member_store(type_store, &request.name) else {
            return Ok(false);
        };
        let Some((source_offset, source_value)) = member_store(source_store, &request.name) else {
            return Ok(false);
        };
        let Some((dest_offset, dest_value)) = member_store(dest_store, &request.name) else {
            return Ok(false);
        };
        let Some((length_offset, length_value)) = member_store(length_store, &request.name) else {
            return Ok(false);
        };
        if !is_null(next_value)
            || variable(owner_value) != Some(owner.name.as_str())
            || variable(type_value) != Some(request_type.name.as_str())
            || variable(source_value) != Some(source.name.as_str())
            || variable(dest_value) != Some(dest.name.as_str())
            || variable(length_value) != Some(length.name.as_str())
        {
            return Ok(false);
        }
        let Ok(next_offset_i16) = i16::try_from(next_offset) else {
            return Ok(false);
        };
        let Ok(owner_offset_i16) = i16::try_from(owner_offset) else {
            return Ok(false);
        };
        let Ok(type_offset_i16) = i16::try_from(type_offset) else {
            return Ok(false);
        };
        let Ok(source_offset_i16) = i16::try_from(source_offset) else {
            return Ok(false);
        };
        let Ok(dest_offset_i16) = i16::try_from(dest_offset) else {
            return Ok(false);
        };
        let Ok(length_offset_i16) = i16::try_from(length_offset) else {
            return Ok(false);
        };

        let Statement::If {
            condition: Expression::Variable(callback_condition),
            then_body: callback_then,
            else_body: callback_else,
        } = callback_choice
        else {
            return Ok(false);
        };
        let [then_store] = callback_then.as_slice() else {
            return Ok(false);
        };
        let [else_store] = callback_else.as_slice() else {
            return Ok(false);
        };
        let Some((callback_offset, callback_value)) = member_store(then_store, &request.name)
        else {
            return Ok(false);
        };
        let Some((fallback_offset, fallback_value)) = member_store(else_store, &request.name)
        else {
            return Ok(false);
        };
        let Expression::AddressOf {
            operand: fallback_operand,
        } = peel_casts(fallback_value)
        else {
            return Ok(false);
        };
        let Expression::Variable(fallback) = fallback_operand.as_ref() else {
            return Ok(false);
        };
        if callback_condition != &callback.name
            || callback_offset != fallback_offset
            || variable(callback_value) != Some(callback.name.as_str())
        {
            return Ok(false);
        }
        let Ok(callback_offset_i16) = i16::try_from(callback_offset) else {
            return Ok(false);
        };

        let Statement::Assign {
            name: enabled_name,
            value:
                Expression::Call {
                    name: disable,
                    arguments: disable_arguments,
                },
        } = disable_assign
        else {
            return Ok(false);
        };
        if enabled_name != &enabled.name || !disable_arguments.is_empty() {
            return Ok(false);
        }
        let Statement::Switch {
            scrutinee,
            arms,
            default,
        } = priority_switch
        else {
            return Ok(false);
        };
        if variable(scrutinee) != Some(priority.name.as_str())
            || default.is_some()
            || arms.len() != 2
        {
            return Ok(false);
        }
        let Some(low_arm) = arms.iter().find(|arm| arm.value == 0) else {
            return Ok(false);
        };
        let Some(high_arm) = arms.iter().find(|arm| arm.value == 1) else {
            return Ok(false);
        };
        let Some((queue_lo, tail_lo)) = queue_arm(low_arm, &request.name, next_offset) else {
            return Ok(false);
        };
        let Some((queue_hi, tail_hi)) = queue_arm(high_arm, &request.name, next_offset) else {
            return Ok(false);
        };

        let Statement::If {
            condition: idle_condition,
            then_body: idle_body,
            else_body: idle_else,
        } = idle_service
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: idle_left,
            right: idle_right,
        } = idle_condition
        else {
            return Ok(false);
        };
        let Some(pending_hi) = null_comparison(idle_left) else {
            return Ok(false);
        };
        let Some(pending_lo) = null_comparison(idle_right) else {
            return Ok(false);
        };
        let [pop_call, Statement::If {
            condition: service_condition,
            then_body: service_body,
            else_body: service_else,
        }] = idle_body.as_slice()
        else {
            return Ok(false);
        };
        let Some(pop_name) = empty_call(pop_call) else {
            return Ok(false);
        };
        let [service_call] = service_body.as_slice() else {
            return Ok(false);
        };
        let Some(service_name) = empty_call(service_call) else {
            return Ok(false);
        };
        if !idle_else.is_empty()
            || !service_else.is_empty()
            || null_comparison(service_condition) != Some(pending_hi)
        {
            return Ok(false);
        }
        let Statement::Expression(Expression::Call {
            name: restore,
            arguments: restore_arguments,
        }) = restore_call
        else {
            return Ok(false);
        };
        if !matches!(restore_arguments.as_slice(), [Expression::Variable(name)] if name == &enabled.name)
        {
            return Ok(false);
        }

        let Some(pop) = self.inline_summaries.queue_pop(pop_name).cloned() else {
            return Ok(false);
        };
        let Some(service) = self.inline_summaries.queue_service(service_name).cloned() else {
            return Ok(false);
        };
        if pop.queue != queue_hi
            || pop.pending != pending_hi
            || service.queue != queue_lo
            || service.pending != pending_lo
            || i32::from(pop.next_offset) != i32::from(next_offset)
            || pop.direction_offset != type_offset_i16
            || pop.source_offset != source_offset_i16
            || pop.dest_offset != dest_offset_i16
            || pop.length_offset != length_offset_i16
            || pop.callback_offset != callback_offset_i16
            || service.direction_offset != type_offset_i16
            || service.source_offset != source_offset_i16
            || service.dest_offset != dest_offset_i16
            || service.length_offset != length_offset_i16
            || service.callback_offset != callback_offset_i16
        {
            return Ok(false);
        }
        let globals = [
            queue_lo,
            tail_lo,
            queue_hi,
            tail_hi,
            pending_hi,
            pending_lo,
            service.chunk.as_str(),
            pop.callback.as_str(),
            service.callback.as_str(),
        ];
        if globals.iter().any(|name| !self.globals.contains_key(*name)) {
            return Ok(false);
        }

        // The request pointer, priority, and interrupt token occupy r29, r30,
        // and r31 respectively. Their save/restore order is part of this
        // transaction's schedule. Build 163 also reserves the complete outgoing
        // argument area, producing its measured 56-byte linkage-first frame.
        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29];
        self.epilogue_lr_before_gprs = true;
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.frame_size = 32;
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -32,
                    });
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: 10,
                        immediate: 0,
                    });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 36,
                });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, 0));
                self.output.instructions.push(Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 28,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset: 24,
                });
                self.emit_callee_saved_home_copy(30, 6);
                self.output.instructions.push(Instruction::StoreWord {
                    s: 29,
                    a: 1,
                    offset: 20,
                });
                self.emit_callee_saved_home_copy(29, 3);
            }
            FrameConvention::LinkageFirst => {
                self.frame_size = 56;
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: 10,
                        immediate: 0,
                    });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 4,
                });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, 0));
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -56,
                    });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 52,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset: 48,
                });
                self.emit_callee_saved_home_copy(30, 6);
                self.output.instructions.push(Instruction::StoreWord {
                    s: 29,
                    a: 1,
                    offset: 44,
                });
                self.emit_callee_saved_home_copy(29, 3);
            }
        }

        for (register, offset) in [
            (0, next_offset_i16),
            (4, owner_offset_i16),
            (5, type_offset_i16),
            (7, source_offset_i16),
            (8, dest_offset_i16),
            (9, length_offset_i16),
        ] {
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 3,
                offset,
            });
        }
        let callback_fallback = self.fresh_label();
        let callback_done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, callback_fallback);
        self.output.instructions.push(Instruction::StoreWord {
            s: 10,
            a: 29,
            offset: callback_offset_i16,
        });
        self.emit_branch_to(callback_done);
        self.bind_label(callback_fallback);
        self.record_relocation(RelocationKind::Addr16Ha, fallback);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, fallback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 29,
            offset: callback_offset_i16,
        });
        self.bind_label(callback_done);

        self.record_relocation(RelocationKind::Rel24, disable);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: disable.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 1,
            });
        self.emit_callee_saved_home_copy(31, 3);
        let high = self.fresh_label();
        let switch_end = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, high);
        self.emit_branch_conditional_to(4, 0, switch_end);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: 0,
            });
        let low = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, low);
        self.emit_branch_to(switch_end);

        self.bind_label(low);
        self.emit_queue_append(queue_lo, tail_lo, next_offset_i16)?;
        self.emit_branch_to(switch_end);
        self.bind_label(high);
        self.emit_queue_append(queue_hi, tail_hi, next_offset_i16)?;
        self.bind_label(switch_end);

        let restore_point = self.fresh_label();
        self.emit_global_load(pending_hi, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, restore_point);
        self.emit_global_load(pending_lo, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, restore_point);
        let pop_done = self.fresh_label();
        self.emit_queue_pop_body(&pop, pop_done);
        self.bind_label(pop_done);
        self.emit_global_load(pending_hi, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, restore_point);
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => self.emit_queue_service_body(&service, restore_point),
            FrameConvention::LinkageFirst => {
                self.record_relocation(RelocationKind::Rel24, service_name);
                self.output.instructions.push(Instruction::BranchAndLink {
                    target: service_name.to_string(),
                });
            }
        }

        self.bind_label(restore_point);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, restore);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: restore.clone(),
        });
        // Build 163 keeps the service helper out of line. The resulting label
        // walk is smaller than the 2.4.x schedule that inlines both helpers.
        self.output.anonymous_label_bump += match self.behavior.frame_convention {
            FrameConvention::Predecrement => 39,
            FrameConvention::LinkageFirst => 21,
        };
        self.pin_queue_helper_post_function_bump();
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            // The disable prototype is cataloged before the queue-tail globals
            // in build 163's source walk; the 2.4.x AST order differs.
            self.output.symbol_order = [disable, tail_lo, tail_hi, restore]
                .into_iter()
                .map(String::from)
                .collect();
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_queue_append(&mut self, queue: &str, tail: &str, next_offset: i16) -> Compilation<()> {
        self.emit_global_load(queue, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        let empty = self.fresh_label();
        let joined = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, empty);
        self.emit_global_load(tail, 3)?;
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 3,
            offset: next_offset,
        });
        self.emit_branch_to(joined);
        self.bind_label(empty);
        self.record_relocation(RelocationKind::EmbSda21, queue);
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 0,
            offset: 0,
        });
        self.bind_label(joined);
        self.record_relocation(RelocationKind::EmbSda21, tail);
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 0,
            offset: 0,
        });
        Ok(())
    }
}
