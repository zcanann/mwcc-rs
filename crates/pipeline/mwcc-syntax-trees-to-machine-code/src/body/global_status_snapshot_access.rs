//! Buffer access guarded by a temporary global status snapshot.
//!
//! Debug-monitor register accessors preserve a four-word exception-status
//! object around one of two buffer calls. The snapshot, flag clear, derived
//! array address, byte count, call diamond, exception check, and restore share
//! one register/latency schedule in legacy optimized MWCC. Lowering those
//! statements independently can clobber still-live ABI parameters and can send
//! the early error exit past saved-register reloads, so this owner validates and
//! emits the complete transaction.

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy)]
struct GlobalStatusSnapshotAccess<'a> {
    status: &'a str,
    status_member_offset: u32,
    cpu_state: &'a str,
    cpu_array_offset: u32,
    maximum_index: i64,
    invalid_index_error: i64,
    exception_error: i64,
    append: &'a str,
    read: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn local_has_type(function: &Function, name: &str, expected: Type) -> bool {
    function
        .locals
        .iter()
        .any(|local| local.name == name && local.declared_type == expected && !local.is_static)
}

fn call_assignment<'a>(
    statement: &'a Statement,
    result: &str,
    arguments: [&str; 3],
) -> Option<&'a str> {
    let Statement::Assign {
        name,
        value:
            Expression::Call {
                name: callee,
                arguments: actual,
            },
    } = statement
    else {
        return None;
    };
    (name == result
        && matches!(actual.as_slice(), [first, second, third]
            if variable(first, arguments[0])
                && variable(second, arguments[1])
                && variable(third, arguments[2])))
    .then_some(callee)
}

fn recognize(function: &Function) -> Option<GlobalStatusSnapshotAccess<'_>> {
    let [first, last, buffer, length, read_parameter] = function.parameters.as_slice() else {
        return None;
    };
    if first.parameter_type != Type::UnsignedInt
        || last.parameter_type != Type::UnsignedInt
        || !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || length.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || read_parameter.parameter_type != Type::Int
        || function.return_type != Type::Int
        || !function.guards.is_empty()
    {
        return None;
    }

    let [range_guard, snapshot, clear_status, data_assignment, count_assignment, length_store,
        call_diamond, exception_guard, restore] = function.statements.as_slice()
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
    let (Some(maximum_index), Some(invalid_index_error)) =
        (constant_value(maximum), constant_value(invalid_error))
    else {
        return None;
    };
    if !invalid_else.is_empty() || !variable(guarded_last, &last.name) {
        return None;
    }

    let Statement::Assign {
        name: snapshot_name,
        value: Expression::Variable(status),
    } = snapshot
    else {
        return None;
    };
    if !local_has_type(
        function,
        snapshot_name,
        Type::Struct { size: 16, align: 4 },
    ) {
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
    if constant_value(cleared) != Some(0) || !variable(clear_base, status) {
        return None;
    }

    let Statement::Assign {
        name: data_name,
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
    let Expression::MemberAddress {
        base: cpu_base,
        offset: cpu_array_offset,
        element: Pointee::UnsignedInt,
        index_stride: None,
    } = data_base.as_ref()
    else {
        return None;
    };
    let Expression::Variable(cpu_state) = cpu_base.as_ref() else {
        return None;
    };
    if !variable(data_index, &first.name)
        || !local_has_type(function, data_name, Type::Pointer(Pointee::UnsignedInt))
    {
        return None;
    }

    let Statement::Assign {
        name: count_name,
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
        left: difference_last,
        right: difference_first,
    } = difference.as_ref()
    else {
        return None;
    };
    if constant_value(one) != Some(1)
        || !variable(difference_last, &last.name)
        || !variable(difference_first, &first.name)
        || !local_has_type(function, count_name, Type::UnsignedInt)
    {
        return None;
    }

    let Statement::Store {
        target: Expression::Dereference { pointer: length_pointer },
        value:
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: stored_count,
                right: count_scale,
            },
    } = length_store
    else {
        return None;
    };
    if !variable(length_pointer, &length.name)
        || !variable(stored_count, count_name)
        || constant_value(count_scale) != Some(4)
    {
        return None;
    }

    let Statement::If {
        condition: read_condition,
        then_body,
        else_body,
    } = call_diamond
    else {
        return None;
    };
    let [then_call] = then_body.as_slice() else {
        return None;
    };
    let [else_call] = else_body.as_slice() else {
        return None;
    };
    let Statement::Assign { name: error_name, .. } = then_call else {
        return None;
    };
    let append = call_assignment(
        then_call,
        error_name,
        [&buffer.name, data_name, count_name],
    )?;
    let read = call_assignment(
        else_call,
        error_name,
        [&buffer.name, data_name, count_name],
    )?;
    if !variable(read_condition, &read_parameter.name)
        || !local_has_type(function, error_name, Type::Int)
    {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Member {
                base: detected_base,
                offset: detected_offset,
                member_type: Type::UnsignedChar,
                index_stride: None,
            },
        then_body: exception_body,
        else_body: exception_else,
    } = exception_guard
    else {
        return None;
    };
    let [
        Statement::Store {
            target: Expression::Dereference { pointer: reset_length },
            value: reset_value,
        },
        Statement::Assign {
            name: exception_result,
            value: exception_error,
        },
    ] = exception_body.as_slice()
    else {
        return None;
    };
    let Some(exception_error) = constant_value(exception_error) else {
        return None;
    };
    if !exception_else.is_empty()
        || !variable(detected_base, status)
        || detected_offset != status_member_offset
        || !variable(reset_length, &length.name)
        || constant_value(reset_value) != Some(0)
        || exception_result != error_name
    {
        return None;
    }

    if !matches!(restore,
        Statement::Store {
            target: Expression::Variable(restored_status),
            value: Expression::Variable(restored_snapshot),
        } if restored_status == status && restored_snapshot == snapshot_name)
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Variable(returned)) if returned == error_name)
    {
        return None;
    }

    Some(GlobalStatusSnapshotAccess {
        status,
        status_member_offset: *status_member_offset,
        cpu_state,
        cpu_array_offset: *cpu_array_offset,
        maximum_index,
        invalid_index_error,
        exception_error,
        append,
        read,
    })
}

impl Generator {
    pub(crate) fn try_global_status_snapshot_access(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
        {
            return Ok(false);
        }
        let Some(access) = recognize(function) else {
            return Ok(false);
        };
        if !matches!(self.globals.get(access.status), Some(Type::Struct { size: 16, .. }))
            || !self.globals.contains_key(access.cpu_state)
            || i16::try_from(access.status_member_offset).is_err()
            || i16::try_from(access.cpu_array_offset).is_err()
            || u16::try_from(access.maximum_index).is_err()
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31, 30];
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
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
        self.output
            .instructions
            .push(Instruction::move_register(31, 6));

        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 4,
                immediate: access.maximum_index as u16,
            });
        let body = self.fresh_label();
        let end = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, body);
        self.load_integer_constant(3, access.invalid_index_error);
        self.emit_branch_to(end);
        self.bind_label(body);

        self.record_relocation(RelocationKind::Addr16Ha, access.status);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 7, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, access.status);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 6,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 8,
            offset: 0,
        });
        self.output.instructions.push(Instruction::SubtractFrom {
            d: 4,
            a: 3,
            b: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 8,
            offset: 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 9,
            a: 4,
            immediate: 1,
        });
        self.record_relocation(RelocationKind::Addr16Ha, access.cpu_state);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 8,
        });
        self.record_relocation(RelocationKind::Addr16Lo, access.cpu_state);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: access.cpu_array_offset as i16,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 3, s: 3, shift: 2 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 8,
            immediate: access.status_member_offset as i16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 3 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 8,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 8,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 9, shift: 2 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(7, 9));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 6,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        });

        let read_arm = self.fresh_label();
        let call_join = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, read_arm);
        self.output
            .instructions
            .push(Instruction::move_register(3, 5));
        self.output
            .instructions
            .push(Instruction::move_register(5, 7));
        self.record_relocation(RelocationKind::Rel24, access.append);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: access.append.into(),
        });
        self.emit_branch_to(call_join);
        self.bind_label(read_arm);
        self.output
            .instructions
            .push(Instruction::move_register(3, 5));
        self.output
            .instructions
            .push(Instruction::move_register(5, 7));
        self.record_relocation(RelocationKind::Rel24, access.read);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: access.read.into(),
        });
        self.bind_label(call_join);

        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 });
        let restore = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, restore);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: 0,
        });
        self.load_integer_constant(3, access.exception_error);
        self.bind_label(restore);

        self.record_relocation(RelocationKind::Addr16Ha, access.status);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 12,
        });
        self.record_relocation(RelocationKind::Addr16Lo, access.status);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 5,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 5,
            offset: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 5,
            offset: 12,
        });

        self.bind_label(end);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
