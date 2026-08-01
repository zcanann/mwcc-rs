//! Extended-register buffer access with restore-flag overlap tracking.
//!
//! Debug-monitor writes to the emulated TBR and DEC windows must publish
//! restore flags before reading the incoming buffer. The range guard,
//! exception snapshot, scaled register address, two overlap tests, buffer call,
//! exception check, and restore form one optimized linkage-first transaction.

#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
struct ExtendedRegisterAccess {
    status: String,
    snapshot: String,
    status_member_offset: i16,
    cpu: String,
    data_offset: i16,
    first_upper_offset: i16,
    first_lower_offset: i16,
    second_offset: i16,
    restore_flags: String,
    first_flag_offset: i16,
    second_flag_offset: i16,
    maximum: u16,
    invalid_error: i64,
    exception_error: i64,
    append: String,
    read: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn dereference_of(expression: &Expression, expected: &str) -> bool {
    matches!(expression,
        Expression::Dereference { pointer } if variable(pointer, expected))
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn local_has_type(function: &Function, name: &str, expected: Type) -> bool {
    function.locals.iter().any(|local| {
        local.name == name
            && local.declared_type == expected
            && local.initializer.is_none()
            && !local.is_static
            && local.array_length.is_none()
    })
}

fn zero_length_store(statement: &Statement, length: &str) -> bool {
    matches!(statement,
        Statement::Store {
            target: Expression::Dereference { pointer },
            value,
        } if variable(pointer, length) && constant_value(value) == Some(0))
}

fn global_member_address(expression: &Expression) -> Option<(&str, u32)> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Member {
        base,
        offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = operand.as_ref()
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, *offset))
}

fn recognize_overlap_guard<'a>(
    statement: &'a Statement,
    data: &str,
    count: &str,
) -> Option<(&'a str, u32, u32, &'a str, u32)> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: upper_test,
                right: lower_test,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LessEqual,
        left: upper_data,
        right: upper_address,
    } = upper_test.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: end_address,
        right: lower_address,
    } = lower_test.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: data_plus_count,
        right: one,
    } = end_address.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: end_data,
        right: end_count,
    } = data_plus_count.as_ref()
    else {
        return None;
    };
    let (upper_global, upper_offset) = global_member_address(upper_address)?;
    let (lower_global, lower_offset) = global_member_address(lower_address)?;
    let [Statement::Store {
        target:
            Expression::Member {
                base: flag_base,
                offset: flag_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        value: flag_value,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let Expression::Variable(flags) = flag_base.as_ref() else {
        return None;
    };
    if !else_body.is_empty()
        || upper_global != lower_global
        || !variable(upper_data, data)
        || !variable(end_data, data)
        || !variable(end_count, count)
        || constant_value(one) != Some(1)
        || constant_value(flag_value) != Some(1)
    {
        return None;
    }
    Some((upper_global, upper_offset, lower_offset, flags, *flag_offset))
}

fn recognize(function: &Function) -> Option<ExtendedRegisterAccess> {
    let [first, last, buffer, length, read_parameter] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || first.parameter_type != Type::UnsignedInt
        || last.parameter_type != Type::UnsignedInt
        || !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || length.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || read_parameter.parameter_type != Type::Int
        || !function.guards.is_empty()
        || function.locals.len() != 4
    {
        return None;
    }
    let snapshot = function.locals.iter().find(|local| {
        local.declared_type == Type::Struct { size: 16, align: 4 }
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let error = function.locals.iter().find(|local| {
        local.declared_type == Type::Int
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let data = function.locals.iter().find(|local| {
        local.declared_type == Type::Pointer(Pointee::UnsignedInt)
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let count = function.locals.iter().find(|local| {
        local.name != error.name
            && local.declared_type == Type::Int
            && local.initializer.is_none()
            && !local.is_static
    })?;
    if !local_has_type(function, &error.name, Type::Int)
        || !local_has_type(function, &data.name, Type::Pointer(Pointee::UnsignedInt))
        || !local_has_type(function, &count.name, Type::Int)
    {
        return None;
    }

    let [range_guard, snapshot_assignment, clear_status, clear_length, active_guard,
        exception_guard, restore] = function.statements.as_slice()
    else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: guarded_last,
                right: maximum,
            },
        then_body: invalid_body,
        else_body: invalid_else,
    } = range_guard
    else {
        return None;
    };
    let [Statement::Return(Some(invalid_error))] = invalid_body.as_slice() else {
        return None;
    };
    if !invalid_else.is_empty() || !variable(guarded_last, &last.name) {
        return None;
    }
    let maximum = u16::try_from(constant_value(maximum)?).ok()?;
    let invalid_error = constant_value(invalid_error)?;

    let Statement::Assign {
        name: assigned_snapshot,
        value: Expression::Variable(status),
    } = snapshot_assignment
    else {
        return None;
    };
    if assigned_snapshot != &snapshot.name {
        return None;
    }
    let Statement::Store {
        target:
            Expression::Member {
                base: clear_base,
                offset: status_member_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        value: cleared,
    } = clear_status
    else {
        return None;
    };
    if !variable(clear_base, status)
        || constant_value(cleared) != Some(0)
        || !zero_length_store(clear_length, &length.name)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LessEqual,
                left: guarded_first,
                right: active_last,
            },
        then_body: active_body,
        else_body: active_else,
    } = active_guard
    else {
        return None;
    };
    let [data_assignment, count_assignment, length_update, call_diamond] = active_body.as_slice()
    else {
        return None;
    };
    if !active_else.is_empty()
        || !variable(guarded_first, &first.name)
        || !variable(active_last, &last.name)
    {
        return None;
    }

    let Statement::Assign {
        name: assigned_data,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: data_base,
                right: data_index,
            },
    } = data_assignment
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::Pointer(Pointee::UnsignedInt),
        operand: data_address,
    } = data_base.as_ref()
    else {
        return None;
    };
    let Expression::AddressOf { operand: data_member } = data_address.as_ref() else {
        return None;
    };
    let Expression::Member {
        base: data_global_base,
        offset: data_offset,
        index_stride: None,
        ..
    } = data_member.as_ref()
    else {
        return None;
    };
    let Expression::Variable(cpu) = data_global_base.as_ref() else {
        return None;
    };
    if assigned_data != &data.name || !variable(data_index, &first.name) {
        return None;
    }

    let Statement::Assign {
        name: assigned_count,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: difference,
                right: one,
            },
    } = count_assignment
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: count_last,
        right: count_first,
    } = difference.as_ref()
    else {
        return None;
    };
    if assigned_count != &count.name
        || !variable(count_last, &last.name)
        || !variable(count_first, &first.name)
        || constant_value(one) != Some(1)
    {
        return None;
    }

    let Statement::Store {
        target: Expression::Dereference { pointer: updated_length },
        value: Expression::IndexedUpdateValue { value: update_value },
    } = length_update
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_length,
        right: byte_count,
    } = update_value.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: stored_count,
        right: four,
    } = byte_count.as_ref()
    else {
        return None;
    };
    if !variable(updated_length, &length.name)
        || !dereference_of(old_length, &length.name)
        || !variable(stored_count, &count.name)
        || constant_value(four) != Some(4)
    {
        return None;
    }

    let Statement::If {
        condition: read_condition,
        then_body: read_body,
        else_body: write_body,
    } = call_diamond
    else {
        return None;
    };
    let [Statement::Assign { name: read_error, value: append_call }] = read_body.as_slice()
    else {
        return None;
    };
    let [first_overlap, second_overlap, Statement::Assign { name: write_error, value: read_call }] =
        write_body.as_slice()
    else {
        return None;
    };
    let (append, append_arguments) = direct_call(append_call)?;
    let (read, read_arguments) = direct_call(read_call)?;
    if !variable(read_condition, &read_parameter.name)
        || read_error != &error.name
        || write_error != &error.name
        || !matches!(append_arguments, [call_buffer, call_data, call_count]
            if variable(call_buffer, &buffer.name)
                && variable(call_data, &data.name)
                && variable(call_count, &count.name))
        || !matches!(read_arguments, [call_buffer, call_data, call_count]
            if variable(call_buffer, &buffer.name)
                && variable(call_data, &data.name)
                && variable(call_count, &count.name))
    {
        return None;
    }
    let (first_cpu, first_upper_offset, first_lower_offset, restore_flags, first_flag_offset) =
        recognize_overlap_guard(first_overlap, &data.name, &count.name)?;
    let (second_cpu, second_upper_offset, second_lower_offset, second_flags, second_flag_offset) =
        recognize_overlap_guard(second_overlap, &data.name, &count.name)?;
    if first_cpu != cpu
        || second_cpu != cpu
        || second_flags != restore_flags
        || second_upper_offset != second_lower_offset
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Member {
                base: exception_base,
                offset: exception_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        then_body: exception_body,
        else_body: exception_else,
    } = exception_guard
    else {
        return None;
    };
    let [exception_length, Statement::Assign { name: exception_result, value: exception_value }] =
        exception_body.as_slice()
    else {
        return None;
    };
    if !exception_else.is_empty()
        || !variable(exception_base, status)
        || exception_offset != status_member_offset
        || !zero_length_store(exception_length, &length.name)
        || exception_result != &error.name
    {
        return None;
    }
    let exception_error = constant_value(exception_value)?;
    if !matches!(restore,
        Statement::Store {
            target: Expression::Variable(restored_status),
            value: Expression::Variable(restored_snapshot),
        } if restored_status == status && restored_snapshot == &snapshot.name)
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Variable(returned)) if returned == &error.name)
    {
        return None;
    }

    Some(ExtendedRegisterAccess {
        status: status.clone(),
        snapshot: snapshot.name.clone(),
        status_member_offset: i16::try_from(*status_member_offset).ok()?,
        cpu: cpu.clone(),
        data_offset: i16::try_from(*data_offset).ok()?,
        first_upper_offset: i16::try_from(first_upper_offset).ok()?,
        first_lower_offset: i16::try_from(first_lower_offset).ok()?,
        second_offset: i16::try_from(second_upper_offset).ok()?,
        restore_flags: restore_flags.into(),
        first_flag_offset: i16::try_from(first_flag_offset).ok()?,
        second_flag_offset: i16::try_from(second_flag_offset).ok()?,
        maximum,
        invalid_error,
        exception_error,
        append: append.into(),
        read: read.into(),
    })
}

impl Generator {
    pub(crate) fn try_extended_register_access(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        let Some(access) = recognize(function) else {
            return Ok(false);
        };
        if !matches!(self.globals.get(&access.status), Some(Type::Struct { size: 16, .. }))
            || !self.globals.contains_key(&access.cpu)
            || !self.globals.contains_key(&access.restore_flags)
        {
            return Ok(false);
        }
        self.emit_extended_register_access(access);
        Ok(true)
    }

    fn emit_extended_register_access(&mut self, access: ExtendedRegisterAccess) {
        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31, 30];
        self.frame_slots.insert(
            access.snapshot,
            FrameSlot {
                offset: 8,
                class: ValueClass::General,
                size: 16,
                value_type: Type::Struct { size: 16, align: 4 },
                parameter_register: None,
                is_array: false,
            },
        );

        let body = self.fresh_label();
        let joined = self.fresh_label();
        let write = self.fresh_label();
        let second_overlap = self.fresh_label();
        let read_call = self.fresh_label();
        let restore = self.fresh_label();
        let epilogue = self.fresh_label();
        let emit_call = |generator: &mut Self, name: &str| {
            generator.record_relocation(RelocationKind::Rel24, name);
            generator.output.instructions.push(Instruction::BranchAndLink {
                target: name.into(),
            });
        };

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreWord { s: 31, a: 1, offset: 28 },
            Instruction::StoreWord { s: 30, a: 1, offset: 24 },
            Instruction::move_register(30, 6),
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: access.maximum },
        ]);
        self.emit_branch_conditional_to(4, 1, body);
        self.load_integer_constant(3, access.invalid_error);
        self.emit_branch_to(epilogue);

        self.bind_label(body);
        self.record_relocation(RelocationKind::Addr16Ha, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 6, a: 0, immediate: 0 },
            Instruction::CompareLogicalWord { a: 3, b: 4 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 9, a: 6, immediate: 0 },
            Instruction::LoadWord { d: 8, a: 9, offset: 0 },
            Instruction::AddImmediate {
                d: 31,
                a: 9,
                immediate: access.status_member_offset,
            },
            Instruction::LoadWord { d: 6, a: 9, offset: 4 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 8, a: 1, offset: 8 },
            Instruction::StoreWord { s: 6, a: 1, offset: 12 },
            Instruction::LoadWord { d: 8, a: 9, offset: 8 },
            Instruction::LoadWord { d: 6, a: 9, offset: 12 },
            Instruction::StoreWord { s: 8, a: 1, offset: 16 },
            Instruction::StoreWord { s: 6, a: 1, offset: 20 },
            Instruction::StoreByte { s: 0, a: 31, offset: 0 },
            Instruction::StoreWord { s: 0, a: 30, offset: 0 },
        ]);
        self.emit_branch_conditional_to(12, 1, joined);
        self.output.instructions.extend([
            Instruction::SubtractFrom { d: 4, a: 3, b: 4 },
            Instruction::LoadWord { d: 0, a: 30, offset: 0 },
            Instruction::AddImmediate { d: 8, a: 4, immediate: 1 },
            Instruction::CompareWordImmediate { a: 7, immediate: 0 },
            Instruction::ShiftLeftImmediate { a: 6, s: 8, shift: 2 },
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &access.cpu);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 });
        self.output.instructions.extend([
            Instruction::Add { d: 0, a: 0, b: 6 },
            Instruction::StoreWord { s: 0, a: 30, offset: 0 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &access.cpu);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 7, a: 4, immediate: 0 },
            Instruction::ShiftLeftImmediate { a: 0, s: 3, shift: 2 },
            Instruction::Add { d: 4, a: 7, b: 0 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: access.data_offset },
        ]);
        self.emit_branch_conditional_to(12, 2, write);
        self.output.instructions.extend([
            Instruction::move_register(3, 5),
            Instruction::move_register(5, 8),
        ]);
        emit_call(self, &access.append);
        self.emit_branch_to(joined);

        self.bind_label(write);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 0, a: 7, immediate: access.first_upper_offset },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
        ]);
        self.emit_branch_conditional_to(12, 1, second_overlap);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 3, a: 6, immediate: -4 },
            Instruction::AddImmediate { d: 0, a: 7, immediate: access.first_lower_offset },
            Instruction::Add { d: 3, a: 4, b: 3 },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
        ]);
        self.emit_branch_conditional_to(12, 0, second_overlap);
        self.record_relocation(RelocationKind::Addr16Ha, &access.restore_flags);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &access.restore_flags);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::load_immediate(0, 1),
            Instruction::StoreByte { s: 0, a: 3, offset: access.first_flag_offset },
        ]);

        self.bind_label(second_overlap);
        self.record_relocation(RelocationKind::Addr16Ha, &access.cpu);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &access.cpu);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 6, a: 3, immediate: access.second_offset },
            Instruction::CompareLogicalWord { a: 4, b: 6 },
        ]);
        self.emit_branch_conditional_to(12, 1, read_call);
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate { a: 3, s: 8, shift: 2 },
            Instruction::AddImmediate { d: 0, a: 3, immediate: -4 },
            Instruction::Add { d: 0, a: 4, b: 0 },
            Instruction::CompareLogicalWord { a: 0, b: 6 },
        ]);
        self.emit_branch_conditional_to(12, 0, read_call);
        self.record_relocation(RelocationKind::Addr16Ha, &access.restore_flags);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &access.restore_flags);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::load_immediate(0, 1),
            Instruction::StoreByte { s: 0, a: 3, offset: access.second_flag_offset },
        ]);

        self.bind_label(read_call);
        self.output.instructions.extend([
            Instruction::move_register(3, 5),
            Instruction::move_register(5, 8),
        ]);
        emit_call(self, &access.read);

        self.bind_label(joined);
        self.output.instructions.extend([
            Instruction::LoadByteZero { d: 0, a: 31, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, restore);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 30, offset: 0 },
        ]);
        self.load_integer_constant(3, access.exception_error);

        self.bind_label(restore);
        self.record_relocation(RelocationKind::Addr16Ha, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 5, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 4, a: 1, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 12 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 5, a: 5, immediate: 0 },
            Instruction::StoreWord { s: 4, a: 5, offset: 0 },
            Instruction::StoreWord { s: 0, a: 5, offset: 4 },
            Instruction::LoadWord { d: 4, a: 1, offset: 16 },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::StoreWord { s: 4, a: 5, offset: 8 },
            Instruction::StoreWord { s: 0, a: 5, offset: 12 },
        ]);

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 31, a: 1, offset: 28 },
            Instruction::LoadWord { d: 30, a: 1, offset: 24 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 13;
    }
}
