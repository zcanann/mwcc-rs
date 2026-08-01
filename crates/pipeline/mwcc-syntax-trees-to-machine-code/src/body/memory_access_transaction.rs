//! Memory access guarded by a temporary exception-status snapshot.
//!
//! The debug monitor translates and validates an address, switches the MSR for
//! one of two memory copies, flushes writes, observes a shared exception flag,
//! and finally restores the complete exception object. Legacy optimized MWCC
//! schedules the snapshot, retained ABI parameters, calls, and restore as one
//! transaction; treating the statements independently duplicates the global
//! base and changes every saved-register home after it.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ConditionalOrigin;

#[derive(Debug)]
struct MemoryAccessTransaction {
    status: String,
    snapshot: String,
    status_member_offset: i16,
    cpu_state: String,
    cpu_msr_offset: i16,
    exception_error: i64,
    translate: String,
    validate: String,
    get_msr: String,
    copy: String,
    flush: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn dereference_of(expression: &Expression, expected: &str) -> bool {
    matches!(expression,
        Expression::Dereference { pointer } if variable(pointer, expected))
}

fn local_has_type(function: &Function, name: &str, expected: Type) -> bool {
    function.locals.iter().any(|local| {
        local.name == name
            && local.declared_type == expected
            && !local.is_static
            && local.array_length.is_none()
    })
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn expression_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(expression) = statement else {
        return None;
    };
    direct_call(expression)
}

fn zero_length_store(statement: &Statement, length: &str) -> bool {
    matches!(statement,
        Statement::Store {
            target: Expression::Dereference { pointer },
            value,
        } if variable(pointer, length) && constant_value(value) == Some(0))
}

fn cast_variable(expression: &Expression, expected: &str, target_type: Type) -> bool {
    matches!(expression,
        Expression::Cast { target_type: actual, operand }
            if *actual == target_type && variable(operand, expected))
}

fn recognize(function: &Function) -> Option<MemoryAccessTransaction> {
    let [data, start, length, access_options, read] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || data.parameter_type != Type::Pointer(Pointee::Int)
        || start.parameter_type != Type::UnsignedInt
        || length.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || access_options.parameter_type != Type::Int
        || read.parameter_type != Type::Int
        || !function.guards.is_empty()
    {
        return None;
    }

    let error = function.locals.iter().find(|local| {
        local.declared_type == Type::Int && local.initializer.is_none() && !local.is_static
    })?;
    let target_msr = function.locals.iter().find(|local| {
        local.declared_type == Type::UnsignedInt
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let addr = function.locals.iter().find(|local| {
        local.declared_type == Type::Pointer(Pointee::Int)
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let trk_msr = function.locals.iter().find(|local| {
        local.name != target_msr.name
            && local.declared_type == Type::UnsignedInt
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let snapshot = function.locals.iter().find(|local| {
        local.declared_type == Type::Struct { size: 16, align: 4 }
            && matches!(local.initializer.as_ref(), Some(Expression::Variable(_)))
            && !local.is_static
    })?;
    let Some(Expression::Variable(status)) = snapshot.initializer.as_ref() else {
        return None;
    };
    if function.locals.len() != 5
        || !local_has_type(function, &error.name, Type::Int)
        || !local_has_type(function, &target_msr.name, Type::UnsignedInt)
        || !local_has_type(function, &addr.name, Type::Pointer(Pointee::Int))
        || !local_has_type(function, &trk_msr.name, Type::UnsignedInt)
    {
        return None;
    }

    let [clear_status, translate_address, validate_address, access, exception_guard, restore] =
        function.statements.as_slice()
    else {
        return None;
    };

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
    if !variable(clear_base, status) || constant_value(cleared) != Some(0) {
        return None;
    }

    let Statement::Assign {
        name: assigned_addr,
        value:
            Expression::Cast {
                target_type: Type::Pointer(Pointee::Int),
                operand: translate_call,
            },
    } = translate_address
    else {
        return None;
    };
    let (translate, translate_arguments) = direct_call(translate_call)?;
    if assigned_addr != &addr.name
        || !matches!(translate_arguments, [argument] if variable(argument, &start.name))
    {
        return None;
    }

    let Statement::Assign {
        name: assigned_error,
        value: validate_call,
    } = validate_address
    else {
        return None;
    };
    let (validate, validate_arguments) = direct_call(validate_call)?;
    let [validate_addr, validate_length, validate_write] = validate_arguments else {
        return None;
    };
    let Expression::Conditional {
        condition: read_condition,
        when_true,
        when_false,
        origin: ConditionalOrigin::Ternary,
    } = validate_write
    else {
        return None;
    };
    if assigned_error != &error.name
        || !variable(validate_addr, &addr.name)
        || !dereference_of(validate_length, &length.name)
        || !variable(read_condition, &read.name)
        || constant_value(when_true) != Some(0)
        || constant_value(when_false) != Some(1)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: tested_error,
                right: zero,
            },
        then_body,
        else_body,
    } = access
    else {
        return None;
    };
    let [failure_store] = then_body.as_slice() else {
        return None;
    };
    let [target_assignment, trk_assignment, copy_diamond] = else_body.as_slice() else {
        return None;
    };
    if !variable(tested_error, &error.name)
        || constant_value(zero) != Some(0)
        || !zero_length_store(failure_store, &length.name)
    {
        return None;
    }

    let Statement::Assign {
        name: assigned_target_msr,
        value: get_msr_call,
    } = target_assignment
    else {
        return None;
    };
    let (get_msr, get_msr_arguments) = direct_call(get_msr_call)?;
    if assigned_target_msr != &target_msr.name || !get_msr_arguments.is_empty() {
        return None;
    }

    let Statement::Assign {
        name: assigned_trk_msr,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left: target_msr_value,
                right: masked_cpu_msr,
            },
    } = trk_assignment
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: cpu_member,
        right: msr_mask,
    } = masked_cpu_msr.as_ref()
    else {
        return None;
    };
    let Expression::Member {
        base: cpu_base,
        offset: cpu_msr_offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = cpu_member.as_ref()
    else {
        return None;
    };
    let Expression::Variable(cpu_state) = cpu_base.as_ref() else {
        return None;
    };
    let msr_mask = u8::try_from(constant_value(msr_mask)?).ok()?;
    if assigned_trk_msr != &trk_msr.name
        || !variable(target_msr_value, &target_msr.name)
        || msr_mask != 16
    {
        return None;
    }

    let Statement::If {
        condition: copy_condition,
        then_body: read_body,
        else_body: write_body,
    } = copy_diamond
    else {
        return None;
    };
    let [read_copy] = read_body.as_slice() else {
        return None;
    };
    let [write_copy, first_flush, alias_guard] = write_body.as_slice() else {
        return None;
    };
    let (copy, read_arguments) = expression_call(read_copy)?;
    let (write_copy_name, write_arguments) = expression_call(write_copy)?;
    let [read_data, read_addr, read_length, read_target_msr, read_trk_msr] = read_arguments else {
        return None;
    };
    let [write_addr, write_data, write_length, write_trk_msr, write_target_msr] = write_arguments else {
        return None;
    };
    if !variable(copy_condition, &read.name)
        || write_copy_name != copy
        || !variable(read_data, &data.name)
        || !variable(read_addr, &addr.name)
        || !dereference_of(read_length, &length.name)
        || !variable(read_target_msr, &target_msr.name)
        || !variable(read_trk_msr, &trk_msr.name)
        || !variable(write_addr, &addr.name)
        || !variable(write_data, &data.name)
        || !dereference_of(write_length, &length.name)
        || !variable(write_trk_msr, &trk_msr.name)
        || !variable(write_target_msr, &target_msr.name)
    {
        return None;
    }

    let (flush, first_flush_arguments) = expression_call(first_flush)?;
    if !matches!(first_flush_arguments, [flush_addr, flush_length]
        if variable(flush_addr, &addr.name) && dereference_of(flush_length, &length.name))
    {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left: original_address,
                right: translated_address,
            },
        then_body: alias_body,
        else_body: alias_else,
    } = alias_guard
    else {
        return None;
    };
    let [second_flush] = alias_body.as_slice() else {
        return None;
    };
    let (second_flush_name, second_flush_arguments) = expression_call(second_flush)?;
    if !alias_else.is_empty()
        || !cast_variable(original_address, &start.name, Type::Pointer(Pointee::Int))
        || !variable(translated_address, &addr.name)
        || second_flush_name != flush
        || !matches!(second_flush_arguments, [flush_start, flush_length]
            if cast_variable(flush_start, &start.name, Type::Pointer(Pointee::Int))
                && dereference_of(flush_length, &length.name))
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
    let exception_error = constant_value(exception_value)?;
    if !exception_else.is_empty()
        || !variable(exception_base, status)
        || exception_offset != status_member_offset
        || !zero_length_store(exception_length, &length.name)
        || exception_result != &error.name
    {
        return None;
    }

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

    Some(MemoryAccessTransaction {
        status: status.clone(),
        snapshot: snapshot.name.clone(),
        status_member_offset: i16::try_from(*status_member_offset).ok()?,
        cpu_state: cpu_state.clone(),
        cpu_msr_offset: i16::try_from(*cpu_msr_offset).ok()?,
        exception_error,
        translate: translate.into(),
        validate: validate.into(),
        get_msr: get_msr.into(),
        copy: copy.into(),
        flush: flush.into(),
    })
}

impl Generator {
    pub(crate) fn try_memory_access_transaction(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
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
            || !self.globals.contains_key(&access.cpu_state)
        {
            return Ok(false);
        }
        self.emit_memory_access_transaction(access);
        Ok(true)
    }

    fn emit_memory_access_transaction(&mut self, access: MemoryAccessTransaction) {
        const SNAPSHOT_OFFSET: i16 = 8;
        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 56;
        self.callee_saved = vec![31, 30, 29, 28, 27, 26, 25];
        self.frame_slots.insert(
            access.snapshot,
            FrameSlot {
                offset: SNAPSHOT_OFFSET,
                class: ValueClass::General,
                size: 16,
                value_type: Type::Struct { size: 16, align: 4 },
                parameter_register: None,
                is_array: false,
            },
        );

        let set_write = self.fresh_label();
        let validate = self.fresh_label();
        let valid = self.fresh_label();
        let write = self.fresh_label();
        let joined = self.fresh_label();
        let restore = self.fresh_label();
        let emit_call = |generator: &mut Self, name: &str| {
            generator.record_relocation(RelocationKind::Rel24, name);
            generator.output.instructions.push(Instruction::BranchAndLink {
                target: name.into(),
            });
        };

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -56 },
            Instruction::StoreMultipleWord { s: 25, a: 1, offset: 28 },
            Instruction::move_register(26, 3),
            Instruction::move_register(27, 4),
            Instruction::move_register(28, 5),
            Instruction::move_register(29, 7),
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, &access.status);
        self.output.instructions.push(Instruction::AddImmediateShifted {
            d: 3,
            a: 0,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 5, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 4, a: 5, offset: 0 },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: access.status_member_offset,
            },
            Instruction::LoadWord { d: 0, a: 5, offset: 4 },
            Instruction::load_immediate(30, 0),
            Instruction::move_register(3, 27),
            Instruction::StoreWord { s: 4, a: 1, offset: 8 },
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::LoadWord { d: 4, a: 5, offset: 8 },
            Instruction::LoadWord { d: 0, a: 5, offset: 12 },
            Instruction::StoreWord { s: 4, a: 1, offset: 16 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::StoreByte { s: 30, a: 31, offset: 0 },
        ]);
        emit_call(self, &access.translate);
        self.output.instructions.extend([
            Instruction::CompareWordImmediate { a: 29, immediate: 0 },
            Instruction::move_register(25, 3),
        ]);
        self.emit_branch_conditional_to(12, 2, set_write);
        self.emit_branch_to(validate);
        self.bind_label(set_write);
        self.output.instructions.push(Instruction::load_immediate(30, 1));
        self.bind_label(validate);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 4, a: 28, offset: 0 },
            Instruction::move_register(3, 25),
            Instruction::move_register(5, 30),
        ]);
        emit_call(self, &access.validate);
        self.output.instructions.extend([
            Instruction::move_register(30, 3),
            Instruction::CompareWordImmediate { a: 30, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, valid);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 28, offset: 0 },
        ]);
        self.emit_branch_to(joined);

        self.bind_label(valid);
        emit_call(self, &access.get_msr);
        self.record_relocation(RelocationKind::Addr16Ha, &access.cpu_state);
        self.output.instructions.push(Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 29,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, &access.cpu_state);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: access.cpu_msr_offset,
            },
            Instruction::move_register(8, 3),
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 27,
                end: 27,
            },
            Instruction::Or { a: 7, s: 8, b: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, write);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 5, a: 28, offset: 0 },
            Instruction::move_register(3, 26),
            Instruction::move_register(4, 25),
            Instruction::move_register(6, 8),
        ]);
        emit_call(self, &access.copy);
        self.emit_branch_to(joined);

        self.bind_label(write);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 5, a: 28, offset: 0 },
            Instruction::move_register(3, 25),
            Instruction::move_register(4, 26),
            Instruction::move_register(6, 7),
            Instruction::move_register(7, 8),
        ]);
        emit_call(self, &access.copy);
        self.output.instructions.extend([
            Instruction::move_register(3, 25),
            Instruction::LoadWord { d: 4, a: 28, offset: 0 },
        ]);
        emit_call(self, &access.flush);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 27, b: 25 });
        self.emit_branch_conditional_to(12, 2, joined);
        self.output.instructions.extend([
            Instruction::move_register(3, 27),
            Instruction::LoadWord { d: 4, a: 28, offset: 0 },
        ]);
        emit_call(self, &access.flush);

        self.bind_label(joined);
        self.output.instructions.extend([
            Instruction::LoadByteZero { d: 0, a: 31, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, restore);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 28, offset: 0 },
        ]);
        self.load_integer_constant(30, access.exception_error);

        self.bind_label(restore);
        self.record_relocation(RelocationKind::Addr16Ha, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 4, a: 1, offset: 8 },
            Instruction::LoadWord { d: 0, a: 1, offset: 12 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 5, a: 3, immediate: 0 },
            Instruction::move_register(3, 30),
            Instruction::StoreWord { s: 4, a: 5, offset: 0 },
            Instruction::StoreWord { s: 0, a: 5, offset: 4 },
            Instruction::LoadWord { d: 4, a: 1, offset: 16 },
            Instruction::LoadWord { d: 0, a: 1, offset: 20 },
            Instruction::StoreWord { s: 4, a: 5, offset: 8 },
            Instruction::StoreWord { s: 0, a: 5, offset: 12 },
            Instruction::LoadMultipleWord { d: 25, a: 1, offset: 28 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 56 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 9;
    }
}
