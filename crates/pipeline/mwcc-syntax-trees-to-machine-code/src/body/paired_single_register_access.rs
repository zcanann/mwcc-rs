//! Extended paired-single register access transaction.
//!
//! The debug monitor enables paired-single state through two SPR writes, clears
//! GQR0, then transfers a bounded register range through one shared two-word
//! frame buffer. MWCC retains the loop error in `r3` and schedules both frame
//! arrays, the exception snapshot, and seven saved GPRs as one transaction.

#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
struct PairedSingleRegisterAccess {
    value_buffer: String,
    setup_buffer: String,
    snapshot: String,
    status: String,
    status_member_offset: i16,
    maximum: u16,
    invalid_error: i64,
    exception_error: i64,
    enable_spr: i16,
    clear_spr: i16,
    enable_mask: u16,
    spr_access: String,
    paired_access: String,
    append: String,
    read_buffer: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
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

fn assignment_call<'a>(
    statement: &'a Statement,
    result: &str,
) -> Option<(&'a str, &'a [Expression])> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    (name == result).then(|| direct_call(value))?
}

fn zero_dereference_store(statement: &Statement, pointer_name: &str) -> bool {
    matches!(statement,
        Statement::Store {
            target: Expression::Dereference { pointer },
            value,
        } if variable(pointer, pointer_name) && constant_value(value) == Some(0))
}

fn recognize(function: &Function) -> Option<PairedSingleRegisterAccess> {
    let [first, last, buffer, length, read] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || first.parameter_type != Type::UnsignedInt
        || last.parameter_type != Type::UnsignedInt
        || !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || length.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || read.parameter_type != Type::Int
        || !function.guards.is_empty()
        || function.locals.len() != 5
    {
        return None;
    }
    let value_buffer = function.locals.iter().find(|local| {
        local.declared_type == Type::UnsignedInt
            && local.array_length == Some(2)
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let setup_buffer = function.locals.iter().find(|local| {
        local.declared_type == Type::UnsignedInt
            && local.array_length == Some(1)
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let snapshot = function.locals.iter().find(|local| {
        local.declared_type == Type::Struct { size: 16, align: 4 }
            && local.array_length.is_none()
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let index = function.locals.iter().find(|local| {
        local.declared_type == Type::UnsignedInt
            && local.array_length.is_none()
            && local.initializer.is_none()
            && !local.is_static
    })?;
    let error = function.locals.iter().find(|local| {
        local.declared_type == Type::Int
            && local.array_length.is_none()
            && local.initializer.is_none()
            && !local.is_static
    })?;

    let [range_guard, snapshot_assignment, clear_status, enable_read, enable_patch,
        enable_write, clear_setup_buffer, clear_spr_call, clear_length, clear_error,
        transfer_loop, exception_guard, restore] = function.statements.as_slice()
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
    let [Statement::Return(Some(invalid_value))] = invalid_body.as_slice() else {
        return None;
    };
    if !invalid_else.is_empty() || !variable(guarded_last, &last.name) {
        return None;
    }
    let maximum = u16::try_from(constant_value(maximum)?).ok()?;
    let invalid_error = constant_value(invalid_value)?;

    let Statement::Assign {
        name: assigned_snapshot,
        value: Expression::Variable(status),
    } = snapshot_assignment
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
    if assigned_snapshot != &snapshot.name
        || !variable(clear_base, status)
        || constant_value(cleared) != Some(0)
    {
        return None;
    }

    let (spr_access, enable_read_arguments) = expression_call(enable_read)?;
    let [enable_read_buffer, enable_spr, enable_read_flag] = enable_read_arguments else {
        return None;
    };
    let enable_spr = i16::try_from(constant_value(enable_spr)?).ok()?;
    if !variable(enable_read_buffer, &setup_buffer.name)
        || constant_value(enable_read_flag) != Some(1)
    {
        return None;
    }
    let Statement::Store {
        target: Expression::Index { base: patch_base, index: patch_index },
        value: Expression::IndexedUpdateValue { value: patch_value },
    } = enable_patch
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: old_setup,
        right: enable_mask,
    } = patch_value.as_ref()
    else {
        return None;
    };
    let enable_mask_value = constant_value(enable_mask)?;
    let enable_mask = u16::try_from((enable_mask_value >> 16) & 0xffff).ok()?;
    if !variable(patch_base, &setup_buffer.name)
        || constant_value(patch_index) != Some(0)
        || !matches!(old_setup.as_ref(), Expression::Index { base, index }
            if variable(base, &setup_buffer.name) && constant_value(index) == Some(0))
        || enable_mask_value != i64::from(enable_mask) << 16
    {
        return None;
    }
    let (enable_write_name, enable_write_arguments) = expression_call(enable_write)?;
    if enable_write_name != spr_access
        || !matches!(enable_write_arguments, [call_buffer, call_spr, call_read]
            if variable(call_buffer, &setup_buffer.name)
                && constant_value(call_spr) == Some(i64::from(enable_spr))
                && constant_value(call_read) == Some(0))
        || !matches!(clear_setup_buffer,
            Statement::Store { target: Expression::Index { base, index }, value }
                if variable(base, &setup_buffer.name)
                    && constant_value(index) == Some(0)
                    && constant_value(value) == Some(0))
    {
        return None;
    }
    let (clear_spr_name, clear_spr_arguments) = expression_call(clear_spr_call)?;
    let [clear_call_buffer, clear_spr, clear_read] = clear_spr_arguments else {
        return None;
    };
    let clear_spr = i16::try_from(constant_value(clear_spr)?).ok()?;
    if clear_spr_name != spr_access
        || !variable(clear_call_buffer, &setup_buffer.name)
        || constant_value(clear_read) != Some(0)
        || !zero_dereference_store(clear_length, &length.name)
        || !matches!(clear_error,
            Statement::Assign { name, value }
                if name == &error.name && constant_value(value) == Some(0))
    {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Assign { target: loop_target, value: loop_start }),
        condition: Some(loop_condition),
        step: Some(loop_step),
        body: loop_body,
    } = transfer_loop
    else {
        return None;
    };
    if !variable(loop_target, &index.name)
        || !variable(loop_start, &first.name)
        || !matches!(loop_condition,
            Expression::Binary { operator: BinaryOperator::LogicalAnd, .. })
        || !matches!(loop_step,
            Expression::Assign { target, value } if matches!(value.as_ref(),
                Expression::Binary { operator: BinaryOperator::Add, left, right }
                if variable(target, &index.name)
                    && variable(left, &index.name)
                    && constant_value(right) == Some(1)))
    {
        return None;
    }
    let [transfer_diamond, length_update] = loop_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: transfer_read,
        then_body: read_body,
        else_body: write_body,
    } = transfer_diamond
    else {
        return None;
    };
    let [paired_read, append_call] = read_body.as_slice() else {
        return None;
    };
    let [read_call, paired_write] = write_body.as_slice() else {
        return None;
    };
    let (paired_access, paired_read_arguments) = assignment_call(paired_read, &error.name)?;
    let (append, append_arguments) = assignment_call(append_call, &error.name)?;
    let (read_buffer, read_arguments) = assignment_call(read_call, &error.name)?;
    let (paired_write_name, paired_write_arguments) = assignment_call(paired_write, &error.name)?;
    let Statement::Store {
        target: Expression::Dereference { pointer: updated_length },
        value: Expression::IndexedUpdateValue { value: length_value },
    } = length_update
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_length,
        right: eight,
    } = length_value.as_ref()
    else {
        return None;
    };
    if !variable(transfer_read, &read.name)
        || paired_write_name != paired_access
        || paired_read_arguments.len() != 3
        || paired_write_arguments.len() != 3
        || !matches!(append_arguments, [call_buffer, _]
            if variable(call_buffer, &buffer.name))
        || !matches!(read_arguments, [call_buffer, _]
            if variable(call_buffer, &buffer.name))
        || !variable(updated_length, &length.name)
        || !matches!(old_length.as_ref(), Expression::Dereference { pointer }
            if variable(pointer, &length.name))
        || constant_value(eight) != Some(8)
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
        || !zero_dereference_store(exception_length, &length.name)
        || exception_result != &error.name
        || !matches!(restore,
            Statement::Store {
                target: Expression::Variable(restored_status),
                value: Expression::Variable(restored_snapshot),
            } if restored_status == status && restored_snapshot == &snapshot.name)
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Variable(returned)) if returned == &error.name)
    {
        return None;
    }

    Some(PairedSingleRegisterAccess {
        value_buffer: value_buffer.name.clone(),
        setup_buffer: setup_buffer.name.clone(),
        snapshot: snapshot.name.clone(),
        status: status.clone(),
        status_member_offset: i16::try_from(*status_member_offset).ok()?,
        maximum,
        invalid_error,
        exception_error: constant_value(exception_value)?,
        enable_spr,
        clear_spr,
        enable_mask,
        spr_access: spr_access.into(),
        paired_access: paired_access.into(),
        append: append.into(),
        read_buffer: read_buffer.into(),
    })
}

impl Generator {
    pub(crate) fn try_paired_single_register_access(
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
        if !matches!(self.globals.get(&access.status), Some(Type::Struct { size: 16, .. })) {
            return Ok(false);
        }
        self.emit_paired_single_register_access(access);
        Ok(true)
    }

    fn emit_paired_single_register_access(&mut self, access: PairedSingleRegisterAccess) {
        self.non_leaf = true;
        self.frame_size = 64;
        self.callee_saved = vec![31, 30, 29, 28, 27, 26, 25];
        for (name, offset, size, value_type, is_array) in [
            (access.setup_buffer, 8, 4, Type::UnsignedInt, true),
            (access.snapshot, 12, 16, Type::Struct { size: 16, align: 4 }, false),
            (access.value_buffer, 28, 8, Type::UnsignedInt, true),
        ] {
            self.frame_slots.insert(
                name,
                FrameSlot {
                    offset,
                    class: ValueClass::General,
                    size,
                    value_type,
                    parameter_register: None,
                    is_array,
                },
            );
        }

        let body = self.fresh_label();
        let init_second = self.fresh_label();
        let init_third = self.fresh_label();
        let condition = self.fresh_label();
        let loop_body = self.fresh_label();
        let write = self.fresh_label();
        let increment = self.fresh_label();
        let joined = self.fresh_label();
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
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -64 },
            Instruction::StoreMultipleWord { s: 25, a: 1, offset: 36 },
            Instruction::move_register(25, 3),
            Instruction::move_register(27, 4),
            Instruction::move_register(28, 5),
            Instruction::move_register(29, 6),
            Instruction::move_register(30, 7),
            Instruction::CompareLogicalWordImmediate { a: 27, immediate: access.maximum },
        ]);
        self.emit_branch_conditional_to(4, 1, body);
        self.load_integer_constant(3, access.invalid_error);
        self.emit_branch_to(epilogue);

        self.bind_label(body);
        self.record_relocation(RelocationKind::Addr16Ha, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 7, a: 4, immediate: 0 },
            Instruction::LoadWord { d: 5, a: 7, offset: 0 },
            Instruction::AddImmediate { d: 31, a: 7, immediate: access.status_member_offset },
            Instruction::LoadWord { d: 0, a: 7, offset: 4 },
            Instruction::load_immediate(26, 0),
            Instruction::load_immediate(4, access.enable_spr),
            Instruction::StoreWord { s: 5, a: 1, offset: 12 },
            Instruction::load_immediate(5, 1),
            Instruction::StoreWord { s: 0, a: 1, offset: 16 },
            Instruction::LoadWord { d: 6, a: 7, offset: 8 },
            Instruction::LoadWord { d: 0, a: 7, offset: 12 },
            Instruction::StoreWord { s: 6, a: 1, offset: 20 },
            Instruction::StoreWord { s: 0, a: 1, offset: 24 },
            Instruction::StoreByte { s: 26, a: 31, offset: 0 },
        ]);
        emit_call(self, &access.spr_access);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 1, offset: 8 },
            Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
            Instruction::load_immediate(4, access.enable_spr),
            Instruction::OrImmediateShifted { a: 0, s: 0, immediate: access.enable_mask },
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::load_immediate(5, 0),
        ]);
        emit_call(self, &access.spr_access);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 26, a: 1, offset: 8 },
            Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
            Instruction::load_immediate(4, access.clear_spr),
            Instruction::load_immediate(5, 0),
        ]);
        emit_call(self, &access.spr_access);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 26, a: 29, offset: 0 },
            Instruction::move_register(26, 25),
            Instruction::load_immediate(3, 0),
        ]);
        self.emit_branch_to(init_second);
        self.bind_label(init_second);
        self.emit_branch_to(init_third);
        self.bind_label(init_third);
        self.emit_branch_to(condition);

        self.bind_label(loop_body);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 30, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, write);
        self.output.instructions.extend([
            Instruction::move_register(4, 26),
            Instruction::AddImmediate { d: 3, a: 1, immediate: 28 },
            Instruction::move_register(5, 30),
        ]);
        emit_call(self, &access.paired_access);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 5, a: 1, offset: 28 },
            Instruction::move_register(3, 28),
            Instruction::LoadWord { d: 6, a: 1, offset: 32 },
        ]);
        emit_call(self, &access.append);
        self.emit_branch_to(increment);

        self.bind_label(write);
        self.output.instructions.extend([
            Instruction::move_register(3, 28),
            Instruction::AddImmediate { d: 4, a: 1, immediate: 28 },
        ]);
        emit_call(self, &access.read_buffer);
        self.output.instructions.extend([
            Instruction::move_register(4, 26),
            Instruction::AddImmediate { d: 3, a: 1, immediate: 28 },
            Instruction::move_register(5, 30),
        ]);
        emit_call(self, &access.paired_access);

        self.bind_label(increment);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 4, a: 29, offset: 0 },
            Instruction::AddImmediate { d: 26, a: 26, immediate: 1 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 8 },
            Instruction::StoreWord { s: 0, a: 29, offset: 0 },
        ]);
        self.bind_label(condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 26, b: 27 });
        self.emit_branch_conditional_to(12, 1, joined);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, loop_body);

        self.bind_label(joined);
        self.output.instructions.extend([
            Instruction::LoadByteZero { d: 0, a: 31, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, restore);
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord { s: 0, a: 29, offset: 0 },
        ]);
        self.load_integer_constant(3, access.exception_error);

        self.bind_label(restore);
        self.record_relocation(RelocationKind::Addr16Ha, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 5, a: 0, immediate: 0 },
            Instruction::LoadWord { d: 4, a: 1, offset: 12 },
            Instruction::LoadWord { d: 0, a: 1, offset: 16 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &access.status);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 5, a: 5, immediate: 0 },
            Instruction::StoreWord { s: 4, a: 5, offset: 0 },
            Instruction::StoreWord { s: 0, a: 5, offset: 4 },
            Instruction::LoadWord { d: 4, a: 1, offset: 20 },
            Instruction::LoadWord { d: 0, a: 1, offset: 24 },
            Instruction::StoreWord { s: 4, a: 5, offset: 8 },
            Instruction::StoreWord { s: 0, a: 5, offset: 12 },
        ]);

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadMultipleWord { d: 25, a: 1, offset: 36 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 64 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 14;
    }
}
