//! Queue-head transactions that surround one conditional external call.

#[allow(unused_imports)]
use super::*;

/// Complete semantic facts for an inlineable queue-pop helper.
#[derive(Clone, Debug)]
pub(crate) struct QueuePopSummary {
    pub(crate) queue: String,
    pub(crate) callback: String,
    pub(crate) pending: String,
    pub(crate) callee: String,
    pub(crate) direction_offset: i16,
    pub(crate) source_offset: i16,
    pub(crate) dest_offset: i16,
    pub(crate) length_offset: i16,
    pub(crate) callback_offset: i16,
    pub(crate) next_offset: i16,
}

/// Recognize the whole helper without consulting its name.  This is shared by
/// standalone lowering and the translation-unit inline-summary pass.
pub(crate) fn summarize_queue_pop(function: &Function) -> Option<QueuePopSummary> {
    if !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_type != Type::Void
        || function.return_expression.is_some()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(queue),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let [Statement::If {
        condition: direction_condition,
        then_body: direction_zero,
        else_body: direction_nonzero,
    }, Statement::Store {
        target: Expression::Variable(callback),
        value: callback_value,
    }, Statement::Store {
        target: Expression::Variable(pending),
        value: Expression::Variable(pending_value),
    }, Statement::Store {
        target: Expression::Variable(queue_target),
        value: next_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if pending_value != queue || queue_target != queue {
        return None;
    }
    let direction_offset = member_equal_zero(direction_condition, queue)?;
    let [Statement::Expression(Expression::Call {
        name: zero_callee,
        arguments: zero_arguments,
    })] = direction_zero.as_slice()
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: nonzero_callee,
        arguments: nonzero_arguments,
    })] = direction_nonzero.as_slice()
    else {
        return None;
    };
    if zero_callee != nonzero_callee {
        return None;
    }
    let [zero_direction, zero_source, zero_dest, zero_length] = zero_arguments.as_slice() else {
        return None;
    };
    let [nonzero_direction, nonzero_dest, nonzero_source, nonzero_length] =
        nonzero_arguments.as_slice()
    else {
        return None;
    };
    let zero_direction_offset = member_offset(zero_direction, queue)?;
    let nonzero_direction_offset = member_offset(nonzero_direction, queue)?;
    let source_offset = member_offset(zero_source, queue)?;
    let dest_offset = member_offset(zero_dest, queue)?;
    let nonzero_dest_offset = member_offset(nonzero_dest, queue)?;
    let nonzero_source_offset = member_offset(nonzero_source, queue)?;
    let length_offset = member_offset(zero_length, queue)?;
    let nonzero_length_offset = member_offset(nonzero_length, queue)?;
    let callback_offset = member_offset(callback_value, queue)?;
    let next_offset = member_offset(next_value, queue)?;
    if direction_offset != zero_direction_offset
        || direction_offset != nonzero_direction_offset
        || source_offset != nonzero_source_offset
        || dest_offset != nonzero_dest_offset
        || length_offset != nonzero_length_offset
        || source_offset == dest_offset
    {
        return None;
    }
    Some(QueuePopSummary {
        queue: queue.clone(),
        callback: callback.clone(),
        pending: pending.clone(),
        callee: zero_callee.clone(),
        direction_offset: i16::try_from(direction_offset).ok()?,
        source_offset: i16::try_from(source_offset).ok()?,
        dest_offset: i16::try_from(dest_offset).ok()?,
        length_offset: i16::try_from(length_offset).ok()?,
        callback_offset: i16::try_from(callback_offset).ok()?,
        next_offset: i16::try_from(next_offset).ok()?,
    })
}

impl Generator {
    /// Pop one request from a global queue and launch its DMA.  The emitted body
    /// is also reusable by a caller in which mwcc inlines this verified helper.
    pub(crate) fn try_global_queue_pop_transaction(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let Some(shape) = summarize_queue_pop(function) else {
            return Ok(false);
        };
        if [&shape.queue, &shape.callback, &shape.pending]
            .into_iter()
            .any(|name| !self.globals.contains_key(name.as_str()))
        {
            return Ok(false);
        }

        self.emit_plain_nonleaf_prologue();
        let outer_end = self.fresh_label();
        self.emit_queue_pop_body(&shape, outer_end);
        self.bind_label(outer_end);
        // Two nested diamonds consume eight anonymous control-flow slots before
        // this non-leaf function's extab pair (whole-object measured).
        self.output.anonymous_label_bump += 8;
        self.pin_queue_helper_post_function_bump();
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.output.symbol_order = [
                shape.queue.as_str(),
                shape.callee.as_str(),
                shape.callback.as_str(),
                shape.pending.as_str(),
            ]
            .into_iter()
            .map(String::from)
            .collect();
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Emit only the transaction body, branching to an owner-supplied end label.
    pub(crate) fn emit_queue_pop_body(
        &mut self,
        shape: &QueuePopSummary,
        outer_end: mwcc_vreg::Label,
    ) {
        self.record_relocation(RelocationKind::EmbSda21, &shape.queue);
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, outer_end);

        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 6,
            offset: shape.direction_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        let nonzero = self.fresh_label();
        let call_join = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, nonzero);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 6,
            offset: shape.source_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 6,
            offset: shape.dest_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 6,
            offset: shape.length_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });
        self.emit_branch_to(call_join);

        self.bind_label(nonzero);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 6,
            offset: shape.dest_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 6,
            offset: shape.source_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 6,
            offset: shape.length_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });

        self.bind_label(call_join);
        self.record_relocation(RelocationKind::EmbSda21, &shape.queue);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.callback_offset,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.callback);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.pending);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.next_offset,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.queue);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
    }
}

fn member_offset(expression: &Expression, base_name: &str) -> Option<u16> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == base_name).then_some(*offset)
}

fn member_equal_zero(condition: &Expression, base_name: &str) -> Option<u16> {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if constant_value(right) == Some(0) {
        member_offset(left, base_name)
    } else if constant_value(left) == Some(0) {
        member_offset(right, base_name)
    } else {
        None
    }
}
