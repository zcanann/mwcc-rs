//! Debug-monitor file-support request transaction.
//!
//! This handler snapshots a command byte from the emulated CPU state, posts an
//! exception event for unsupported commands, otherwise performs file I/O and
//! conditionally flushes the transferred range. The CPU-state member addresses,
//! four saved values, tiny frame objects, and call argument schedule form one
//! legacy linkage-first allocation transaction.

#[allow(unused_imports)]
use super::*;

struct SupportFileRequest {
    global: String,
    event: String,
    io_result: String,
    read_command: u16,
    write_command: u16,
    event_kind: i16,
    pc_offset: i16,
    construct_event: String,
    post_event: String,
    access_file: String,
    flush_cache: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn strip_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn global_word(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Index { base, index } = strip_casts(expression) else {
        return None;
    };
    let Expression::MemberAddress {
        base,
        offset: 0,
        element: Pointee::UnsignedInt,
        index_stride: None,
    } = base.as_ref()
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, constant_value(index)?))
}

fn comparison_constant(
    expression: &Expression,
    name: &str,
    operator: BinaryOperator,
) -> Option<i64> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*actual == operator && variable(left, name)).then(|| constant_value(right))?
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn local_type(function: &Function, name: &str, expected: Type) -> bool {
    function.locals.iter().any(|local| {
        local.name == name && local.declared_type == expected && !local.is_static
    })
}

fn recognize(function: &Function) -> Option<SupportFileRequest> {
    if function.return_type != Type::Int
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.locals.len() != 5
    {
        return None;
    }
    let [command_assignment, unsupported, length_assignment, access_assignment, io_guard,
        io_store, flush_guard, pc_store] = function.statements.as_slice()
    else {
        return None;
    };

    let Statement::Assign {
        name: command,
        value: command_value,
    } = command_assignment
    else {
        return None;
    };
    let (global, command_index) = global_word(command_value)?;
    if command_index != 3 || !local_type(function, command, Type::UnsignedChar) {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: first_command,
                right: second_command,
            },
        then_body: unsupported_body,
        else_body,
    } = unsupported
    else {
        return None;
    };
    let read_command_value = comparison_constant(first_command, command, BinaryOperator::NotEqual)?;
    let write_command_value = comparison_constant(second_command, command, BinaryOperator::NotEqual)?;
    let [Statement::Expression(construct), Statement::Expression(post), Statement::Return(Some(zero))] =
        unsupported_body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() || constant_value(zero) != Some(0) {
        return None;
    }
    let (construct_event, construct_arguments) = direct_call(construct)?;
    let [Expression::AddressOf { operand: event_operand }, event_kind] = construct_arguments else {
        return None;
    };
    let Expression::Variable(event) = event_operand.as_ref() else {
        return None;
    };
    let event_kind = i16::try_from(constant_value(event_kind)?).ok()?;
    let (post_event, post_arguments) = direct_call(post)?;
    if !matches!(post_arguments, [Expression::AddressOf { operand }] if variable(operand, event))
        || !local_type(function, event, Type::Struct { size: 12, align: 4 })
    {
        return None;
    }

    let Statement::Assign {
        name: length,
        value: length_value,
    } = length_assignment
    else {
        return None;
    };
    if global_word(length_value)? != (global, 5)
        || !local_type(function, length, Type::Pointer(Pointee::UnsignedInt))
    {
        return None;
    }

    let Statement::Assign {
        name: error,
        value: access_call,
    } = access_assignment
    else {
        return None;
    };
    let (access_file, access_arguments) = direct_call(access_call)?;
    let [read_buffer, data, length_argument, io_pointer, one, read_test] = access_arguments else {
        return None;
    };
    let (read_global, read_index) = global_word(read_buffer)?;
    let (data_global, data_index) = global_word(data)?;
    let Expression::Cast { operand: io_address, .. } = io_pointer else {
        return None;
    };
    let Expression::AddressOf { operand: io_operand } = io_address.as_ref() else {
        return None;
    };
    let Expression::Variable(io_result) = io_operand.as_ref() else {
        return None;
    };
    if read_global != global
        || data_global != global
        || read_index != 4
        || data_index != 6
        || !variable(length_argument, length)
        || constant_value(one) != Some(1)
        || comparison_constant(read_test, command, BinaryOperator::Equal) != Some(read_command_value)
        || !local_type(function, error, Type::Int)
        || !local_type(function, io_result, Type::UnsignedChar)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: io_test,
                right: error_test,
            },
        then_body,
        else_body,
    } = io_guard
    else {
        return None;
    };
    if comparison_constant(io_test, io_result, BinaryOperator::Equal) != Some(0)
        || comparison_constant(error_test, error, BinaryOperator::NotEqual) != Some(0)
        || !matches!(then_body.as_slice(), [Statement::Assign { name, value }]
            if name == io_result && constant_value(value) == Some(1))
        || !else_body.is_empty()
    {
        return None;
    }

    let Statement::Store { target, value } = io_store else {
        return None;
    };
    if global_word(target)? != (global, 3) || !variable(value, io_result) {
        return None;
    }

    let Statement::If {
        condition: flush_test,
        then_body,
        else_body,
    } = flush_guard
    else {
        return None;
    };
    let [Statement::Expression(flush_call)] = then_body.as_slice() else {
        return None;
    };
    let (flush_cache, flush_arguments) = direct_call(flush_call)?;
    let [flush_data, Expression::Dereference { pointer }] = flush_arguments else {
        return None;
    };
    if comparison_constant(flush_test, command, BinaryOperator::Equal) != Some(read_command_value)
        || global_word(flush_data)? != (global, 6)
        || !variable(pointer, length)
        || !else_body.is_empty()
    {
        return None;
    }

    let Statement::Store {
        target:
            Expression::Member {
                base: pc_base,
                offset: pc_offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value:
            Expression::IndexedUpdateValue {
                value: pc_update,
            },
    } = pc_store
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: pc_left,
        right: four,
    } = pc_update.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base: source_pc_base,
        offset: source_pc_offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = pc_left.as_ref()
    else {
        return None;
    };
    if !variable(pc_base, global)
        || !variable(source_pc_base, global)
        || source_pc_offset != pc_offset
        || constant_value(four) != Some(4)
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, error))
    {
        return None;
    }

    Some(SupportFileRequest {
        global: global.into(),
        event: event.clone(),
        io_result: io_result.clone(),
        read_command: u16::try_from(read_command_value).ok()?,
        write_command: u16::try_from(write_command_value).ok()?,
        event_kind,
        pc_offset: i16::try_from(*pc_offset).ok()?,
        construct_event: construct_event.into(),
        post_event: post_event.into(),
        access_file: access_file.into(),
        flush_cache: flush_cache.into(),
    })
}

impl Generator {
    pub(crate) fn try_support_file_request(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        let Some(request) = recognize(function) else {
            return Ok(false);
        };
        self.emit_support_file_request(request);
        Ok(true)
    }

    fn emit_support_file_request(&mut self, request: SupportFileRequest) {
        const IO_OFFSET: i16 = 8;
        const EVENT_OFFSET: i16 = 12;
        self.non_leaf = true;
        self.frame_size = 48;
        self.callee_saved = vec![31, 30, 29, 28, 27];
        self.frame_slots.insert(
            request.event.clone(),
            FrameSlot {
                offset: EVENT_OFFSET,
                class: ValueClass::General,
                size: 12,
                value_type: Type::Struct { size: 12, align: 4 },
                parameter_register: None,
                is_array: false,
            },
        );
        self.frame_slots.insert(
            request.io_result.clone(),
            FrameSlot {
                offset: IO_OFFSET,
                class: ValueClass::General,
                size: 4,
                value_type: Type::UnsignedChar,
                parameter_register: None,
                is_array: false,
            },
        );

        let main = self.fresh_label();
        let io_join = self.fresh_label();
        let no_flush = self.fresh_label();
        let done = self.fresh_label();
        let emit_call = |generator: &mut Self, name: &str| {
            generator.record_relocation(RelocationKind::Rel24, name);
            generator
                .output
                .instructions
                .push(Instruction::BranchAndLink { target: name.into() });
        };

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -48 },
            Instruction::StoreMultipleWord { s: 27, a: 1, offset: 28 },
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &request.global);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &request.global);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 12 },
            Instruction::LoadWord { d: 0, a: 31, offset: 0 },
            Instruction::ClearLeftImmediate { a: 27, s: 0, clear: 24 },
            Instruction::CompareLogicalWordImmediate { a: 27, immediate: request.read_command },
        ]);
        self.emit_branch_conditional_to(12, 2, main);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 27, immediate: request.write_command });
        self.emit_branch_conditional_to(12, 2, main);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 3, a: 1, immediate: EVENT_OFFSET },
            Instruction::load_immediate(4, request.event_kind),
        ]);
        emit_call(self, &request.construct_event);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: EVENT_OFFSET });
        emit_call(self, &request.post_event);
        self.output.instructions.push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(done);

        self.bind_label(main);
        self.record_relocation(RelocationKind::Addr16Ha, &request.global);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 6, a: 1, immediate: IO_OFFSET },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &request.global);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 3, a: 4, offset: 16 },
            Instruction::SubtractFromImmediate { d: 0, a: 27, immediate: request.read_command as i16 },
            Instruction::LoadWord { d: 28, a: 4, offset: 20 },
            Instruction::AddImmediate { d: 30, a: 4, immediate: 24 },
            Instruction::CountLeadingZeros { a: 0, s: 0 },
            Instruction::LoadWord { d: 4, a: 30, offset: 0 },
            Instruction::ClearLeftImmediate { a: 3, s: 3, clear: 24 },
            Instruction::move_register(5, 28),
            Instruction::ShiftRightLogicalImmediate { a: 8, s: 0, shift: 5 },
            Instruction::load_immediate(7, 1),
        ]);
        emit_call(self, &request.access_file);
        self.output.instructions.extend([
            Instruction::LoadByteZero { d: 0, a: 1, offset: IO_OFFSET },
            Instruction::move_register(29, 3),
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, io_join);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 29, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, io_join);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 1),
            Instruction::StoreByte { s: 0, a: 1, offset: IO_OFFSET },
        ]);
        self.bind_label(io_join);
        self.output.instructions.extend([
            Instruction::LoadByteZero { d: 0, a: 1, offset: IO_OFFSET },
            Instruction::CompareLogicalWordImmediate { a: 27, immediate: request.read_command },
            Instruction::StoreWord { s: 0, a: 31, offset: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, no_flush);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 3, a: 30, offset: 0 },
            Instruction::LoadWord { d: 4, a: 28, offset: 0 },
        ]);
        emit_call(self, &request.flush_cache);

        self.bind_label(no_flush);
        self.record_relocation(RelocationKind::Addr16Ha, &request.global);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &request.global);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 5, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 4, a: 5, offset: request.pc_offset },
            Instruction::move_register(3, 29),
            Instruction::AddImmediate { d: 0, a: 4, immediate: 4 },
            Instruction::StoreWord { s: 0, a: 5, offset: request.pc_offset },
        ]);

        self.bind_label(done);
        self.output.instructions.extend([
            Instruction::LoadMultipleWord { d: 27, a: 1, offset: 28 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 48 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 10;
    }
}
