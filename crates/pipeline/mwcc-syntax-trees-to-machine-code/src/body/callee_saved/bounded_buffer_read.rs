//! Bounded byte-buffer reads with capacity clamping.
//!
//! This is the read-side sibling of the append transaction: error, requested
//! length, and buffer base survive the bulk-copy call in r31..r29 while the
//! destination remains in its incoming volatile register until argument setup.

#[allow(unused_imports)]
use super::*;

struct ReadPlan<'a> {
    callee: &'a str,
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

fn classify(function: &Function) -> Option<ReadPlan<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, requested] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || !matches!(data.parameter_type, Type::Pointer(_))
        || requested.parameter_type != Type::UnsignedInt
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
    let [empty, compute, clamp, copy, advance] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = empty
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::Equal, left, right
        } if var(left, &requested.name) && constant_value(right) == Some(0))
        || !matches!(then_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(0))
    {
        return None;
    }

    let Statement::Assign {
        name: available_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: length_read,
                right: position_read,
            },
    } = compute
    else {
        return None;
    };
    let (length_offset, length_type) = member(length_read, &buffer.name)?;
    let (position_offset, position_type) = member(position_read, &buffer.name)?;
    if available_name != &available.name
        || length_type != Type::UnsignedInt
        || position_type != Type::UnsignedInt
    {
        return None;
    }

    let Statement::If {
        condition,
        then_body,
        else_body,
    } = clamp
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::Greater, left, right
        } if var(left, &requested.name) && var(right, &available.name))
    {
        return None;
    }
    let [Statement::Assign {
        name: error_name,
        value: overflow,
    }, Statement::Assign {
        name: requested_name,
        value: clamped,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if error_name != &error.name
        || requested_name != &requested.name
        || !var(clamped, &available.name)
    {
        return None;
    }
    let overflow = i16::try_from(constant_value(overflow)?).ok()?;

    let Statement::Expression(Expression::Call {
        name: callee,
        arguments,
    }) = copy
    else {
        return None;
    };
    let [destination, source, count] = arguments.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: data_address,
        right: source_position,
    } = source
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: source_buffer,
        offset: data_offset,
        element: Pointee::UnsignedChar,
        index_stride: None,
    } = data_address.as_ref()
    else {
        return None;
    };
    if !var(destination, &data.name)
        || !var(source_buffer, &buffer.name)
        || member(source_position, &buffer.name) != Some((position_offset, Type::UnsignedInt))
        || !var(count, &requested.name)
    {
        return None;
    }
    let data_offset = i16::try_from(*data_offset).ok()?;

    let Statement::Store { target, value } = advance else {
        return None;
    };
    if member(target, &buffer.name) != Some((position_offset, Type::UnsignedInt))
        || !matches!(value, Expression::Binary {
            operator: BinaryOperator::Add, left, right
        } if member(left, &buffer.name) == Some((position_offset, Type::UnsignedInt))
            && var(right, &requested.name))
    {
        return None;
    }
    Some(ReadPlan {
        callee,
        overflow,
        length_offset,
        position_offset,
        data_offset,
    })
}

impl Generator {
    pub(crate) fn try_bounded_buffer_read(&mut self, function: &Function) -> Compilation<bool> {
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
        const REQUESTED: u8 = 30;
        const BUFFER: u8 = 29;
        let nonempty = self.fresh_label();
        let unclamped = self.fresh_label();
        let epilogue = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![ERROR, REQUESTED, BUFFER];
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
                s: REQUESTED,
                a: 1,
                offset: 16,
            },
            Instruction::OrRecord {
                a: REQUESTED,
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
            Instruction::AddImmediate {
                d: 3,
                a: 4,
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
                d: 4,
                a: BUFFER,
                offset: plan.position_offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: BUFFER,
                offset: plan.length_offset,
            },
            Instruction::SubtractFrom { d: 0, a: 4, b: 0 },
            Instruction::CompareLogicalWord { a: REQUESTED, b: 0 },
        ]);
        self.emit_branch_conditional_to(4, 1, unclamped);
        self.output
            .instructions
            .push(Instruction::load_immediate(ERROR, plan.overflow));
        self.output
            .instructions
            .push(Instruction::move_register(REQUESTED, 0));
        self.bind_label(unclamped);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: plan.data_offset,
            },
            Instruction::AddImmediate {
                d: 5,
                a: REQUESTED,
                immediate: 0,
            },
            Instruction::Add {
                d: 4,
                a: BUFFER,
                b: 4,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_string(),
        });
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
                b: REQUESTED,
            },
            Instruction::StoreWord {
                s: 0,
                a: BUFFER,
                offset: plan.position_offset,
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
                d: REQUESTED,
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
