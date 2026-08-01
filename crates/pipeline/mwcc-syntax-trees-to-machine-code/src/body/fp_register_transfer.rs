//! Buffered transfer of one or more floating-point register images.
//!
//! The MetroTRK routine carries an address-taken `u64` through a loop.  That
//! value must live in an eight-byte frame slot and must be loaded as the aligned
//! r5:r6 argument pair for the append call.  Treat the complete transaction as
//! one schedule: its five live parameters, saved exception record, loop cursor,
//! and address-taken pair jointly determine the linkage frame and register homes.

use super::*;

struct FpRegisterTransfer<'a> {
    exception_status: &'a str,
    get_msr: &'a str,
    set_msr: &'a str,
    access_register: &'a str,
    append: &'a str,
    read_buffer: &'a str,
    maximum_register: i16,
    invalid_register: i16,
    exception_error: i16,
    msr_enable: u16,
    exception_flag_offset: i16,
    transfer_width: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn address_of(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::AddressOf { operand } if variable(operand, expected))
}

fn ordinary(local: &LocalDeclaration, expected: Type) -> bool {
    local.declared_type == expected
        && local.initializer.is_none()
        && !local.is_volatile
        && local.array_length.is_none()
        && !local.is_static
}

fn assignment<'a>(expression: &'a Expression, expected: &str) -> Option<&'a Expression> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    variable(target, expected).then_some(value)
}

fn dereference_of(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer } if variable(pointer, expected))
}

fn classify(function: &Function) -> Option<FpRegisterTransfer<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [first, last, buffer, length, read] = function.parameters.as_slice() else {
        return None;
    };
    if first.parameter_type != Type::UnsignedInt
        || last.parameter_type != Type::UnsignedInt
        || !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || length.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || read.parameter_type != Type::Int
    {
        return None;
    }
    let [temporary, error, saved_status, current] = function.locals.as_slice() else {
        return None;
    };
    if !ordinary(temporary, Type::UnsignedLongLong)
        || !ordinary(error, Type::Int)
        || !ordinary(saved_status, Type::Struct { size: 16, align: 4 })
        || !ordinary(current, Type::UnsignedInt)
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, &error.name))
    {
        return None;
    }

    let [range_guard, save_status, clear_exception, enable_msr, clear_length, initialize_error, transfer_loop, exception_guard, restore_status] =
        function.statements.as_slice()
    else {
        return None;
    };

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Greater,
                left: tested_last,
                right: maximum,
            },
        then_body,
        else_body,
    } = range_guard
    else {
        return None;
    };
    let [Statement::Return(Some(invalid_register))] = then_body.as_slice() else {
        return None;
    };
    if !variable(tested_last, &last.name) || !else_body.is_empty() {
        return None;
    }
    let maximum_register = i16::try_from(constant_value(maximum)?).ok()?;
    let invalid_register = i16::try_from(constant_value(invalid_register)?).ok()?;

    let Statement::Assign {
        name: saved_target,
        value: Expression::Variable(exception_status),
    } = save_status
    else {
        return None;
    };
    if saved_target != &saved_status.name {
        return None;
    }

    let Statement::Store {
        target:
            Expression::Member {
                base: exception_base,
                offset: exception_flag_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        value: cleared_exception,
    } = clear_exception
    else {
        return None;
    };
    if !variable(exception_base, exception_status) || constant_value(cleared_exception) != Some(0) {
        return None;
    }
    let exception_flag_offset = i16::try_from(*exception_flag_offset).ok()?;

    let Statement::Expression(Expression::Call {
        name: set_msr,
        arguments: set_arguments,
    }) = enable_msr
    else {
        return None;
    };
    let [Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: get_expression,
        right: enable_mask,
    }] = set_arguments.as_slice()
    else {
        return None;
    };
    let Expression::Call {
        name: get_msr,
        arguments: get_arguments,
    } = get_expression.as_ref()
    else {
        return None;
    };
    let msr_enable = u16::try_from(constant_value(enable_mask)?).ok()?;
    if !get_arguments.is_empty() {
        return None;
    }

    let Statement::Store {
        target: length_target,
        value: cleared_length,
    } = clear_length
    else {
        return None;
    };
    if !dereference_of(length_target, &length.name) || constant_value(cleared_length) != Some(0) {
        return None;
    }
    let Statement::Assign {
        name: error_target,
        value: initial_error,
    } = initialize_error
    else {
        return None;
    };
    if error_target != &error.name || constant_value(initial_error) != Some(0) {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = transfer_loop
    else {
        return None;
    };
    if !matches!(initializer,
        Expression::Assign { target, value }
            if variable(target, &current.name) && variable(value, &first.name))
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: within_range,
        right: no_error,
    } = condition
    else {
        return None;
    };
    if !matches!(within_range.as_ref(),
        Expression::Binary { operator: BinaryOperator::LessEqual, left, right }
            if variable(left, &current.name) && variable(right, &last.name))
        || !matches!(no_error.as_ref(),
            Expression::Binary { operator: BinaryOperator::Equal, left, right }
                if variable(left, &error.name) && constant_value(right) == Some(0))
    {
        return None;
    }
    let Expression::Comma {
        left: increment_current,
        right: increment_length,
    } = step
    else {
        return None;
    };
    let current_value = assignment(increment_current, &current.name)?;
    if !matches!(current_value,
        Expression::Binary { operator: BinaryOperator::Add, left, right }
            if variable(left, &current.name) && constant_value(right) == Some(1))
    {
        return None;
    }
    let Expression::Assign {
        target: length_step_target,
        value: length_step_value,
    } = increment_length.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_length,
        right: transfer_width,
    } = length_step_value.as_ref()
    else {
        return None;
    };
    if !dereference_of(length_step_target, &length.name)
        || !dereference_of(old_length, &length.name)
    {
        return None;
    }
    let transfer_width = i16::try_from(constant_value(transfer_width)?).ok()?;

    let [Statement::If {
        condition: read_condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if !variable(read_condition, &read.name) {
        return None;
    }
    let [Statement::Expression(Expression::Call {
        name: access_register,
        arguments: read_access_arguments,
    }), Statement::Assign {
        name: read_error,
        value: Expression::Call {
            name: append,
            arguments: append_arguments,
        },
    }] = then_body.as_slice()
    else {
        return None;
    };
    if read_error != &error.name
        || !matches!(read_access_arguments.as_slice(), [address, index, mode]
            if address_of(address, &temporary.name)
                && variable(index, &current.name)
                && variable(mode, &read.name))
        || !matches!(append_arguments.as_slice(), [target, value]
            if variable(target, &buffer.name) && variable(value, &temporary.name))
    {
        return None;
    }
    let [Statement::Expression(Expression::Call {
        name: read_buffer,
        arguments: read_arguments,
    }), Statement::Assign {
        name: write_error,
        value: Expression::Call {
            name: write_access,
            arguments: write_access_arguments,
        },
    }] = else_body.as_slice()
    else {
        return None;
    };
    if write_error != &error.name
        || write_access != access_register
        || !matches!(read_arguments.as_slice(), [target, address]
            if variable(target, &buffer.name) && address_of(address, &temporary.name))
        || !matches!(write_access_arguments.as_slice(), [address, index, mode]
            if address_of(address, &temporary.name)
                && variable(index, &current.name)
                && variable(mode, &read.name))
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Member {
                base: tested_exception,
                offset: tested_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        then_body,
        else_body,
    } = exception_guard
    else {
        return None;
    };
    let [Statement::Store {
        target: reset_length,
        value: reset_value,
    }, Statement::Assign {
        name: exception_error_target,
        value: exception_error,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if !variable(tested_exception, exception_status)
        || *tested_offset != exception_flag_offset as u32
        || !else_body.is_empty()
        || !dereference_of(reset_length, &length.name)
        || constant_value(reset_value) != Some(0)
        || exception_error_target != &error.name
    {
        return None;
    }
    let exception_error = i16::try_from(constant_value(exception_error)?).ok()?;

    if !matches!(restore_status,
        Statement::Store { target, value }
            if variable(target, exception_status) && variable(value, &saved_status.name))
    {
        return None;
    }

    Some(FpRegisterTransfer {
        exception_status,
        get_msr,
        set_msr,
        access_register,
        append,
        read_buffer,
        maximum_register,
        invalid_register,
        exception_error,
        msr_enable,
        exception_flag_offset,
        transfer_width,
    })
}

impl Generator {
    pub(crate) fn try_fp_register_transfer(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        self.emit_fp_register_transfer(&plan);
        Ok(true)
    }

    fn emit_fp_transfer_call(&mut self, target: &str) {
        self.record_relocation(RelocationKind::Rel24, target);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: target.to_string(),
        });
    }

    fn emit_fp_register_transfer(&mut self, plan: &FpRegisterTransfer<'_>) {
        const FIRST: u8 = 25;
        const EXCEPTION_FLAG: u8 = 26;
        const CURRENT: u8 = 27;
        const LAST: u8 = 28;
        const BUFFER: u8 = 29;
        const LENGTH: u8 = 30;
        const READ: u8 = 31;

        let valid = self.fresh_label();
        let setup_branch_one = self.fresh_label();
        let setup_branch_two = self.fresh_label();
        let loop_body = self.fresh_label();
        let write_path = self.fresh_label();
        let loop_step = self.fresh_label();
        let loop_test = self.fresh_label();
        let restore_status = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 64;
        self.callee_saved = vec![31, 30, 29, 28, 27, 26, 25];
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
                offset: -64,
            },
            Instruction::StoreMultipleWord {
                s: FIRST,
                a: 1,
                offset: 36,
            },
            Instruction::move_register(FIRST, 3),
            Instruction::move_register(LAST, 4),
            Instruction::move_register(BUFFER, 5),
            Instruction::move_register(LENGTH, 6),
            Instruction::move_register(READ, 7),
            Instruction::CompareLogicalWordImmediate {
                a: LAST,
                immediate: plan.maximum_register as u16,
            },
        ]);
        self.emit_branch_conditional_to(4, 1, valid); // ble
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.invalid_register));
        self.emit_branch_to(epilogue);

        self.bind_label(valid);
        self.record_relocation(RelocationKind::Addr16Ha, plan.exception_status);
        self.output.instructions.push(Instruction::AddImmediateShifted {
            d: 3,
            a: 0,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, plan.exception_status);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 4,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: EXCEPTION_FLAG,
                a: 4,
                immediate: plan.exception_flag_offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 4,
            },
            Instruction::load_immediate(CURRENT, 0),
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreByte {
                s: CURRENT,
                a: EXCEPTION_FLAG,
                offset: 0,
            },
        ]);
        self.emit_fp_transfer_call(plan.get_msr);
        self.output.instructions.push(Instruction::OrImmediate {
            a: 3,
            s: 3,
            immediate: plan.msr_enable,
        });
        self.emit_fp_transfer_call(plan.set_msr);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: CURRENT,
                a: LENGTH,
                offset: 0,
            },
            Instruction::move_register(CURRENT, FIRST),
            Instruction::load_immediate(3, 0),
        ]);
        self.emit_branch_to(setup_branch_one);
        self.bind_label(setup_branch_one);
        self.emit_branch_to(setup_branch_two);
        self.bind_label(setup_branch_two);
        self.emit_branch_to(loop_test);

        self.bind_label(loop_body);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: READ, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, write_path); // beq
        self.output.instructions.extend([
            Instruction::move_register(4, CURRENT),
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 24,
            },
            Instruction::move_register(5, READ),
        ]);
        self.emit_fp_transfer_call(plan.access_register);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: 1,
                offset: 24,
            },
            Instruction::move_register(3, BUFFER),
            Instruction::LoadWord {
                d: 6,
                a: 1,
                offset: 28,
            },
        ]);
        self.emit_fp_transfer_call(plan.append);
        self.emit_branch_to(loop_step);

        self.bind_label(write_path);
        self.output.instructions.extend([
            Instruction::move_register(3, BUFFER),
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 24,
            },
        ]);
        self.emit_fp_transfer_call(plan.read_buffer);
        self.output.instructions.extend([
            Instruction::move_register(4, CURRENT),
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 24,
            },
            Instruction::move_register(5, READ),
        ]);
        self.emit_fp_transfer_call(plan.access_register);

        self.bind_label(loop_step);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: LENGTH,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: CURRENT,
                a: CURRENT,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: plan.transfer_width,
            },
            Instruction::StoreWord {
                s: 0,
                a: LENGTH,
                offset: 0,
            },
        ]);

        self.bind_label(loop_test);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: CURRENT, b: LAST });
        self.emit_branch_conditional_to(12, 1, restore_status); // bgt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, loop_body); // beq

        self.bind_label(restore_status);
        self.output.instructions.extend([
            Instruction::LoadByteZero {
                d: 0,
                a: EXCEPTION_FLAG,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        let copy_status = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, copy_status); // beq
        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: LENGTH,
                offset: 0,
            },
            Instruction::load_immediate(3, plan.exception_error),
        ]);

        self.bind_label(copy_status);
        self.record_relocation(RelocationKind::Addr16Ha, plan.exception_status);
        self.output.instructions.push(Instruction::AddImmediateShifted {
            d: 5,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, plan.exception_status);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 4,
                a: 5,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 4,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 16,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 4,
                a: 5,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: 12,
            },
        ]);

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadMultipleWord {
                d: FIRST,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 64,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
