//! Chunked low-priority queue servicing around a conditional DMA call.

use super::fixed_rmw_recognize::peel_casts;
#[allow(unused_imports)]
use super::*;

struct DirectionCall<'a> {
    callee: &'a str,
    direction: u16,
    source: u16,
    dest: u16,
    length: LengthArgument<'a>,
}

enum LengthArgument<'a> {
    Member(u16),
    Global(&'a str),
}

/// Complete semantic facts for an inlineable chunked-service helper.
#[derive(Clone, Debug)]
pub(crate) struct QueueServiceSummary {
    pub(crate) pending: String,
    pub(crate) queue: String,
    pub(crate) chunk: String,
    pub(crate) callback: String,
    pub(crate) callee: String,
    pub(crate) next_offset: i16,
    pub(crate) length_offset: i16,
    pub(crate) direction_offset: i16,
    pub(crate) source_offset: i16,
    pub(crate) dest_offset: i16,
    pub(crate) callback_offset: i16,
}

/// Recognize the whole helper without consulting its name.  Standalone
/// lowering and callers that inline it consume the same verified summary.
pub(crate) fn summarize_queue_service(function: &Function) -> Option<QueueServiceSummary> {
    if !function.parameters.is_empty()
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_type != Type::Void
        || function.return_expression.is_some()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [promote, service] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: promote_condition,
        then_body: promote_body,
        else_body: promote_else,
    } = promote
    else {
        return None;
    };
    if !promote_else.is_empty() {
        return None;
    }
    let (pending, queue) = null_and_global(promote_condition)?;
    let [Statement::Store {
        target: Expression::Variable(promoted_target),
        value: Expression::Variable(promoted_value),
    }, Statement::Store {
        target: Expression::Variable(queue_target),
        value: next_value,
    }] = promote_body.as_slice()
    else {
        return None;
    };
    let next_offset = member_offset(next_value, queue)?;
    if promoted_target != pending || promoted_value != queue || queue_target != queue {
        return None;
    }

    let Statement::If {
        condition: Expression::Variable(service_pending),
        then_body: service_body,
        else_body: service_else,
    } = service
    else {
        return None;
    };
    if service_pending != pending || !service_else.is_empty() {
        return None;
    }
    let [size_choice, length_update, source_update, dest_update] = service_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: size_condition,
        then_body: short_body,
        else_body: long_body,
    } = size_choice
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LessEqual,
        left: size_left,
        right: size_right,
    } = size_condition
    else {
        return None;
    };
    let length_offset = member_offset(size_left, pending)?;
    let Expression::Variable(chunk) = size_right.as_ref() else {
        return None;
    };

    let [short_direction_statement, Statement::Store {
        target: Expression::Variable(callback),
        value: callback_value,
    }] = short_body.as_slice()
    else {
        return None;
    };
    let [long_direction_statement] = long_body.as_slice() else {
        return None;
    };
    let short_direction = direction_call(short_direction_statement, pending)?;
    let long_direction = direction_call(long_direction_statement, pending)?;
    let callback_offset = member_offset(callback_value, pending)?;
    if short_direction.callee != long_direction.callee
        || short_direction.direction != long_direction.direction
        || short_direction.source != long_direction.source
        || short_direction.dest != long_direction.dest
        || !matches!(short_direction.length, LengthArgument::Member(offset) if offset == length_offset)
        || !matches!(long_direction.length, LengthArgument::Global(name) if name == chunk)
    {
        return None;
    }
    let updated_length = member_update(length_update, pending, chunk, BinaryOperator::Subtract)?;
    let updated_source = member_update(source_update, pending, chunk, BinaryOperator::Add)?;
    let updated_dest = member_update(dest_update, pending, chunk, BinaryOperator::Add)?;
    if updated_length != length_offset
        || updated_source != short_direction.source
        || updated_dest != short_direction.dest
    {
        return None;
    }

    Some(QueueServiceSummary {
        pending: pending.to_string(),
        queue: queue.to_string(),
        chunk: chunk.clone(),
        callback: callback.clone(),
        callee: short_direction.callee.to_string(),
        next_offset: i16::try_from(next_offset).ok()?,
        length_offset: i16::try_from(length_offset).ok()?,
        direction_offset: i16::try_from(short_direction.direction).ok()?,
        source_offset: i16::try_from(short_direction.source).ok()?,
        dest_offset: i16::try_from(short_direction.dest).ok()?,
        callback_offset: i16::try_from(callback_offset).ok()?,
    })
}

impl Generator {
    /// Service one chunk from a global pending/queue pair.
    pub(crate) fn try_global_chunked_queue_service(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        let Some(shape) = summarize_queue_service(function) else {
            return Ok(false);
        };
        if [&shape.pending, &shape.queue, &shape.chunk, &shape.callback]
            .into_iter()
            .any(|name| !self.globals.contains_key(name.as_str()))
        {
            return Ok(false);
        }

        self.emit_plain_nonleaf_prologue();
        let outer_end = self.fresh_label();
        self.emit_queue_service_body(&shape, outer_end);
        self.bind_label(outer_end);
        // The compound transaction creates twenty internal control-flow slots.
        self.output.anonymous_label_bump += match self.behavior.frame_convention {
            FrameConvention::Predecrement => 20,
            FrameConvention::LinkageFirst
                if self
                    .inline_summaries
                    .queue_service_has_caller(&function.name) =>
            {
                17
            }
            FrameConvention::LinkageFirst => 20,
        };
        self.pin_queue_helper_post_function_bump();
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.output.symbol_order = [
                shape.pending.as_str(),
                shape.queue.as_str(),
                shape.chunk.as_str(),
                shape.callee.as_str(),
                shape.callback.as_str(),
            ]
            .into_iter()
            .map(String::from)
            .collect();
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Emit only the service transaction, branching to an owner-supplied end.
    pub(crate) fn emit_queue_service_body(
        &mut self,
        shape: &QueueServiceSummary,
        outer_end: mwcc_vreg::Label,
    ) {
        self.record_relocation(RelocationKind::EmbSda21, &shape.pending);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        let promoted = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, promoted);
        self.record_relocation(RelocationKind::EmbSda21, &shape.queue);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, promoted);
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

        self.bind_label(promoted);
        self.record_relocation(RelocationKind::EmbSda21, &shape.pending);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, outer_end);
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 5,
            offset: shape.length_offset,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.chunk);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        let long = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, long);

        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: shape.direction_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        let short_nonzero = self.fresh_label();
        let short_join = self.fresh_label();
        let update_join = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, short_nonzero);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 5,
            offset: shape.source_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: shape.dest_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });
        self.emit_branch_to(short_join);
        self.bind_label(short_nonzero);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 5,
            offset: shape.dest_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: shape.source_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });
        self.bind_label(short_join);
        self.record_relocation(RelocationKind::EmbSda21, &shape.pending);
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
        self.emit_branch_to(update_join);

        self.bind_label(long);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: shape.direction_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        let long_nonzero = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, long_nonzero);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 5,
            offset: shape.source_offset,
        });
        self.output
            .instructions
            .push(Instruction::move_register(6, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: shape.dest_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });
        self.emit_branch_to(update_join);
        self.bind_label(long_nonzero);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 5,
            offset: shape.dest_offset,
        });
        self.output
            .instructions
            .push(Instruction::move_register(6, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: shape.source_offset,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });

        self.bind_label(update_join);
        self.record_relocation(RelocationKind::EmbSda21, &shape.pending);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, &shape.chunk);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.length_offset,
        });
        self.output
            .instructions
            .push(Instruction::SubtractFrom { d: 0, a: 4, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.length_offset,
        });
        for offset in [shape.source_offset, shape.dest_offset] {
            self.record_relocation(RelocationKind::EmbSda21, &shape.pending);
            self.output.instructions.push(Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            });
            self.record_relocation(RelocationKind::EmbSda21, &shape.chunk);
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 0,
                offset: 0,
            });
            self.output
                .instructions
                .push(Instruction::LoadWord { d: 3, a: 4, offset });
            self.output
                .instructions
                .push(Instruction::Add { d: 0, a: 3, b: 0 });
            self.output
                .instructions
                .push(Instruction::StoreWord { s: 0, a: 4, offset });
        }
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
    matches!(base.as_ref(), Expression::Variable(name) if name == base_name)
        .then(|| u16::try_from(*offset).ok())?
}

fn is_null(expression: &Expression) -> bool {
    constant_value(peel_casts(expression)) == Some(0)
}

fn null_and_global(condition: &Expression) -> Option<(&str, &str)> {
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: null_left,
        right: null_right,
    } = left.as_ref()
    else {
        return None;
    };
    let pending = match (null_left.as_ref(), null_right.as_ref()) {
        (Expression::Variable(name), other) if is_null(other) => name.as_str(),
        (other, Expression::Variable(name)) if is_null(other) => name.as_str(),
        _ => return None,
    };
    let Expression::Variable(queue) = right.as_ref() else {
        return None;
    };
    Some((pending, queue))
}

fn direction_call<'a>(statement: &'a Statement, base_name: &str) -> Option<DirectionCall<'a>> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let direction = member_equal_zero(condition, base_name)?;
    let [Statement::Expression(Expression::Call {
        name: first_name,
        arguments: first,
    })] = then_body.as_slice()
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: second_name,
        arguments: second,
    })] = else_body.as_slice()
    else {
        return None;
    };
    let [first_direction, first_source, first_dest, first_length] = first.as_slice() else {
        return None;
    };
    let [second_direction, second_dest, second_source, second_length] = second.as_slice() else {
        return None;
    };
    let source = member_offset(first_source, base_name)?;
    let dest = member_offset(first_dest, base_name)?;
    if first_name != second_name
        || member_offset(first_direction, base_name)? != direction
        || member_offset(second_direction, base_name)? != direction
        || member_offset(second_source, base_name)? != source
        || member_offset(second_dest, base_name)? != dest
    {
        return None;
    }
    let length = match first_length {
        Expression::Variable(name) => LengthArgument::Global(name),
        expression => LengthArgument::Member(member_offset(expression, base_name)?),
    };
    match (&length, second_length) {
        (LengthArgument::Global(first), Expression::Variable(second)) if first == second => {}
        (LengthArgument::Member(first), second)
            if member_offset(second, base_name) == Some(*first) => {}
        _ => return None,
    }
    Some(DirectionCall {
        callee: first_name,
        direction,
        source,
        dest,
        length,
    })
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
    if is_null(right) {
        member_offset(left, base_name)
    } else if is_null(left) {
        member_offset(right, base_name)
    } else {
        None
    }
}

fn member_update(
    statement: &Statement,
    base_name: &str,
    increment: &str,
    expected_operator: BinaryOperator,
) -> Option<u16> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let offset = member_offset(target, base_name)?;
    let Expression::Binary {
        operator,
        left,
        right,
    } = value
    else {
        return None;
    };
    (*operator == expected_operator
        && member_offset(left, base_name) == Some(offset)
        && matches!(right.as_ref(), Expression::Variable(name) if name == increment))
    .then_some(offset)
}
