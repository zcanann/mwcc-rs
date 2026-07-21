//! Bounded byte-buffer appends with an inline one-byte fast path.
//!
//! The transaction combines an early empty return, capacity clamping, an
//! inline byte copy versus an external bulk copy, and post-copy cursor stores.
//! Its three cross-call values occupy the dense r31..r29 callee-saved region.

#[allow(unused_imports)]
use super::*;

struct AppendPlan<'a> {
    callee: &'a str,
    capacity: i16,
    overflow: i16,
    length_offset: i16,
    position_offset: i16,
    data_offset: i16,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn member(expression: &Expression, base: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base: found,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    var(found, base).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn member_target<'a>(statement: &'a Statement, base: &str) -> Option<(i16, Type, &'a Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (offset, member_type) = member(target, base)?;
    Some((offset, member_type, value))
}

fn classify(function: &Function) -> Option<AppendPlan<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, length] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || !matches!(data.parameter_type, Type::Pointer(_))
        || length.parameter_type != Type::UnsignedInt
    {
        return None;
    }
    let [error, available] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || constant_value(error.initializer.as_ref()?) != Some(0)
        || available.declared_type != Type::UnsignedInt
        || available.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }
    let [empty_guard, compute_available, clamp, copy, advance, publish] =
        function.statements.as_slice()
    else {
        return None;
    };

    let Statement::If {
        condition: empty_condition,
        then_body: empty_body,
        else_body: empty_else,
    } = empty_guard
    else {
        return None;
    };
    if !empty_else.is_empty()
        || !matches!(empty_condition, Expression::Binary {
            operator: BinaryOperator::Equal, left, right
        } if var(left, &length.name) && constant_value(right) == Some(0))
        || !matches!(empty_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(0))
    {
        return None;
    }

    let Statement::Assign {
        name: available_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: capacity,
                right: position_read,
            },
    } = compute_available
    else {
        return None;
    };
    let capacity = i16::try_from(constant_value(capacity)?).ok()?;
    let (position_offset, position_type) = member(position_read, &buffer.name)?;
    if available_name != &available.name || position_type != Type::UnsignedInt {
        return None;
    }

    let Statement::If {
        condition: clamp_condition,
        then_body: clamp_body,
        else_body: clamp_else,
    } = clamp
    else {
        return None;
    };
    if !clamp_else.is_empty()
        || !matches!(clamp_condition, Expression::Binary {
            operator: BinaryOperator::Less, left, right
        } if var(left, &available.name) && var(right, &length.name))
    {
        return None;
    }
    let [Statement::Assign {
        name: error_name,
        value: overflow,
    }, Statement::Assign {
        name: length_name,
        value: clamped,
    }] = clamp_body.as_slice()
    else {
        return None;
    };
    if error_name != &error.name || length_name != &length.name || !var(clamped, &available.name) {
        return None;
    }
    let overflow = i16::try_from(constant_value(overflow)?).ok()?;

    let Statement::If {
        condition: copy_condition,
        then_body: byte_body,
        else_body: bulk_body,
    } = copy
    else {
        return None;
    };
    if !matches!(copy_condition, Expression::Binary {
        operator: BinaryOperator::Equal, left, right
    } if var(left, &length.name) && constant_value(right) == Some(1))
    {
        return None;
    }
    let [Statement::Store {
        target:
            Expression::Index {
                base: byte_base,
                index: byte_index,
            },
        value: byte_value,
    }] = byte_body.as_slice()
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: byte_buffer,
        offset: data_offset,
        element: Pointee::UnsignedChar,
        index_stride: None,
    } = byte_base.as_ref()
    else {
        return None;
    };
    if !var(byte_buffer, &buffer.name)
        || member(byte_index, &buffer.name) != Some((position_offset, Type::UnsignedInt))
        || !matches!(byte_value, Expression::Index { base, index }
            if constant_value(index) == Some(0)
                && matches!(base.as_ref(), Expression::Cast { operand, .. }
                    if var(operand, &data.name)))
    {
        return None;
    }
    let data_offset = i16::try_from(*data_offset).ok()?;
    let [Statement::Expression(Expression::Call {
        name: callee,
        arguments,
    })] = bulk_body.as_slice()
    else {
        return None;
    };
    if !matches!(arguments.as_slice(), [
        Expression::Binary { operator: BinaryOperator::Add, left, right },
        source,
        count,
    ] if matches!(left.as_ref(), Expression::MemberAddress { base, offset, .. }
            if var(base, &buffer.name) && i16::try_from(*offset).ok() == Some(data_offset))
        && member(right, &buffer.name) == Some((position_offset, Type::UnsignedInt))
        && var(source, &data.name) && var(count, &length.name))
    {
        return None;
    }

    let (advanced_offset, advanced_type, advanced_value) = member_target(advance, &buffer.name)?;
    if advanced_offset != position_offset
        || advanced_type != Type::UnsignedInt
        || !matches!(advanced_value, Expression::Binary {
            operator: BinaryOperator::Add, left, right
        } if member(&left, &buffer.name) == Some((position_offset, Type::UnsignedInt))
            && var(&right, &length.name))
    {
        return None;
    }
    let (length_offset, length_type, published_value) = member_target(publish, &buffer.name)?;
    if length_type != Type::UnsignedInt
        || member(published_value, &buffer.name) != Some((position_offset, Type::UnsignedInt))
    {
        return None;
    }
    Some(AppendPlan {
        callee,
        capacity,
        overflow,
        length_offset,
        position_offset,
        data_offset,
    })
}

impl Generator {
    pub(crate) fn try_bounded_buffer_append(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        const ERROR: u8 = 31;
        const LENGTH: u8 = 30;
        const BUFFER: u8 = 29;
        let nonempty = self.fresh_label();
        let unclamped = self.fresh_label();
        let bulk = self.fresh_label();
        let copied = self.fresh_label();
        let epilogue = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![ERROR, LENGTH, BUFFER];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: ERROR,
                a: 1,
                offset: 20,
            },
            Instruction::load_immediate(ERROR, 0),
            Instruction::StoreWord {
                s: LENGTH,
                a: 1,
                offset: 16,
            },
            Instruction::OrRecord {
                a: LENGTH,
                s: 5,
                b: 5,
            },
            Instruction::StoreWord {
                s: BUFFER,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: BUFFER,
                a: 3,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(4, 2, nonempty);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(nonempty);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: BUFFER,
                offset: plan.position_offset,
            },
            Instruction::SubtractFromImmediate {
                d: 0,
                a: 3,
                immediate: plan.capacity,
            },
            Instruction::CompareLogicalWord { a: 0, b: LENGTH },
        ]);
        self.emit_branch_conditional_to(4, 0, unclamped);
        self.output
            .instructions
            .push(Instruction::load_immediate(ERROR, plan.overflow));
        self.output
            .instructions
            .push(Instruction::move_register(LENGTH, 0));
        self.bind_label(unclamped);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: LENGTH,
                immediate: 1,
            });
        self.emit_branch_conditional_to(4, 2, bulk);
        self.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: 4,
                offset: 0,
            },
            Instruction::Add {
                d: 3,
                a: BUFFER,
                b: 3,
            },
            Instruction::StoreByte {
                s: 0,
                a: 3,
                offset: plan.data_offset,
            },
        ]);
        self.emit_branch_to(copied);
        self.bind_label(bulk);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: plan.data_offset,
            },
            Instruction::AddImmediate {
                d: 5,
                a: LENGTH,
                immediate: 0,
            },
            Instruction::Add {
                d: 3,
                a: BUFFER,
                b: 3,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_string(),
        });
        self.bind_label(copied);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: BUFFER,
                offset: plan.position_offset,
            },
            Instruction::AddImmediate {
                d: 3,
                a: ERROR,
                immediate: 0,
            },
            Instruction::Add {
                d: 0,
                a: 0,
                b: LENGTH,
            },
            Instruction::StoreWord {
                s: 0,
                a: BUFFER,
                offset: plan.position_offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: BUFFER,
                offset: plan.position_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: BUFFER,
                offset: plan.length_offset,
            },
        ]);
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: ERROR,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: LENGTH,
                a: 1,
                offset: 16,
            },
            Instruction::LoadWord {
                d: BUFFER,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
