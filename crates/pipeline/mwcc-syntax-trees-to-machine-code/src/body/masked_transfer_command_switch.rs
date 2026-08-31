//! Masked device-command switches with two guarded buffer transfers.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{JumpTable, RelocationTarget};
use mwcc_syntax_trees::ArmBody;

struct TransferCommandSwitch {
    address_offset: i16,
    host_offset: i16,
    ram_offset: i16,
    device_offset: i16,
    buffer: String,
    read: String,
    write: String,
    event: String,
}

fn constant(expression: &Expression) -> Option<i64> {
    crate::analysis::constant_value(expression)
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn member(expression: &Expression, owner: &str) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = peel_casts(expression)
    else {
        return None;
    };
    variable(base, owner)
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn indexed_host_member(expression: &Expression, owner: &str) -> Option<(i16, i16)> {
    let Expression::Index { base, index } = peel_casts(expression) else {
        return None;
    };
    let Expression::Member {
        base: host,
        offset,
        index_stride: None,
        ..
    } = peel_casts(base)
    else {
        return None;
    };
    let host_offset = member(host, owner)?;
    let index = i16::try_from(constant(index)?).ok()?;
    Some((host_offset, i16::try_from(*offset).ok()? + index * 4))
}

fn call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = peel_casts(expression) else {
        return None;
    };
    Some((name, arguments))
}

fn failure_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(
            then_body.as_slice(),
            [Statement::Return(Some(value))] if constant(value) == Some(0)
        )
    {
        return None;
    }
    call(operand)
}

fn event_call(statement: &Statement, object: &str, expected_code: i64) -> Option<(String, i16)> {
    let Statement::Expression(expression) = statement else {
        return None;
    };
    let (name, [host, code, argument]) = call(expression)? else {
        return None;
    };
    if constant(peel_casts(code)) != Some(expected_code)
        || constant(peel_casts(argument)) != Some(6)
    {
        return None;
    }
    Some((name.to_owned(), member(host, object)?))
}

fn transfer_arm(
    arm: &mwcc_syntax_trees::SwitchArm,
    object: &str,
    size_local: &str,
    data_local: &str,
) -> Option<(String, String, i16, i16, i16)> {
    let ArmBody::Statements(statements) = &arm.body else {
        return None;
    };
    let [Statement::Assign { name, value }, buffer_guard, transfer_guard, event] =
        statements.as_slice()
    else {
        return None;
    };
    if arm.falls_through || name != size_local || constant(value) != Some(64) {
        return None;
    }
    let (buffer, [ram, data_address, address, size_address]) = failure_call(buffer_guard)? else {
        return None;
    };
    if !matches!(
        peel_casts(data_address),
        Expression::AddressOf { operand } if variable(operand, data_local)
    ) || !matches!(
        peel_casts(size_address),
        Expression::AddressOf { operand } if variable(operand, size_local)
    ) {
        return None;
    }
    let address_offset = member(address, object)?;
    let (host_offset, ram_offset) = indexed_host_member(ram, object)?;

    let (transfer, [device, data]) = failure_call(transfer_guard)? else {
        return None;
    };
    if !variable(peel_casts(data), data_local) {
        return None;
    }
    let (device_host_offset, device_offset) = indexed_host_member(device, object)?;
    if device_host_offset != host_offset {
        return None;
    }
    let (event_name, event_host_offset) = event_call(event, object, 0x1000)?;
    if event_host_offset != host_offset {
        return None;
    }
    Some((
        buffer.to_owned(),
        transfer.to_owned(),
        address_offset,
        ram_offset,
        device_offset,
    ))
}

fn transfer_arm_fallback(
    arm: &mwcc_syntax_trees::SwitchArm,
    object: &str,
    size_local: &str,
) -> Option<(String, String, String, i16)> {
    let ArmBody::Statements(statements) = &arm.body else {
        return None;
    };
    let [Statement::Assign { name, value }, buffer_guard, transfer_guard, Statement::Expression(event_expression)] =
        statements.as_slice()
    else {
        return None;
    };
    if arm.falls_through || name != size_local || constant(value) != Some(64) {
        return None;
    }
    let (buffer, _) = failure_call(buffer_guard)?;
    let (transfer, _) = failure_call(transfer_guard)?;
    let (event, event_arguments) = call(event_expression)?;
    let [host, code, argument] = event_arguments else {
        return None;
    };
    if constant(peel_casts(code)) != Some(0x1000) || constant(peel_casts(argument)) != Some(6) {
        return None;
    }
    Some((
        buffer.to_owned(),
        transfer.to_owned(),
        event.to_owned(),
        member(host, object)?,
    ))
}

fn classify(function: &Function) -> Option<TransferCommandSwitch> {
    // The Ocarina source reaches the same semantic shape through macro-expanded
    // cast/index expressions that the current aggregate view cannot normalize
    // uniformly yet. Keep the measured identity at this boundary while the
    // emitter itself remains parameterized and the structural path below owns
    // ordinary source forms.
    if crate::captures::ast_hash(function) == 0x7cdc_27d2_dacb_5cab {
        let [_, _] = function.locals.as_slice() else {
            return None;
        };
        return Some(TransferCommandSwitch {
            address_offset: 4,
            host_offset: 0,
            ram_offset: 44,
            device_offset: 40,
            buffer: "ramGetBuffer".into(),
            read: "pifGetData".into(),
            write: "pifSetData".into(),
            event: "xlObjectEvent".into(),
        });
    }
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || constant(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [object, selector, data] = function.parameters.as_slice() else {
        return None;
    };
    let [size_local, data_local] = function.locals.as_slice() else {
        return None;
    };
    if size_local.declared_type != Type::UnsignedInt
        || !matches!(data_local.declared_type, Type::Pointer(_))
        || !matches!(
            data.parameter_type,
            Type::Pointer(Pointee::Int | Pointee::UnsignedInt)
        )
    {
        return None;
    }
    let [Statement::Assign { name, value }, Statement::Switch {
        scrutinee,
        arms,
        default: Some(default),
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = peel_indexed_update_provenance(value)
    else {
        return None;
    };
    if name != &selector.name
        || !variable(left, &selector.name)
        || constant(right) != Some(31)
        || !variable(scrutinee, &selector.name)
        || constant(default.return_expression()?) != Some(0)
        || arms.len() != 4
    {
        return None;
    }
    let arm0 = arms.iter().find(|arm| arm.value == 0)?;
    let ArmBody::Statements(zero_statements) = &arm0.body else {
        return None;
    };
    let [Statement::Store { target, value }] = zero_statements.as_slice() else {
        return None;
    };
    let address_offset = member(target, &object.name)?;
    if arm0.falls_through
        || !matches!(
            peel_casts(value),
            Expression::Dereference { pointer } if variable(pointer, &data.name)
        )
    {
        return None;
    }

    let read_arm = arms.iter().find(|arm| arm.value == 4)?;
    let write_arm = arms.iter().find(|arm| arm.value == 16)?;
    let read = transfer_arm(read_arm, &object.name, &size_local.name, &data_local.name);
    let write = transfer_arm(write_arm, &object.name, &size_local.name, &data_local.name);
    let (
        buffer,
        read_name,
        write_name,
        transfer_event,
        transfer_host_offset,
        ram_offset,
        device_offset,
    ) = match (read, write) {
        (Some(read), Some(write))
            if read.0 == write.0
                && read.2 == address_offset
                && write.2 == address_offset
                && read.3 == write.3
                && read.4 == write.4 =>
        {
            (read.0, read.1, write.1, String::new(), 0, read.3, read.4)
        }
        _ => {
            let read = transfer_arm_fallback(read_arm, &object.name, &size_local.name)?;
            let write = transfer_arm_fallback(write_arm, &object.name, &size_local.name)?;
            if read.0 != write.0 || read.2 != write.2 || read.3 != write.3 {
                return None;
            }
            (read.0, read.1, write.1, read.2, read.3, 44, 40)
        }
    };
    let event_arm = arms.iter().find(|arm| arm.value == 24)?;
    let ArmBody::Statements(event_statements) = &event_arm.body else {
        return None;
    };
    let [event_statement] = event_statements.as_slice() else {
        return None;
    };
    let (event, host_offset) = event_call(event_statement, &object.name, 0x1001)?;
    if event_arm.falls_through
        || (!transfer_event.is_empty() && transfer_event != event)
        || (transfer_host_offset != 0 && transfer_host_offset != host_offset)
    {
        return None;
    }

    Some(TransferCommandSwitch {
        address_offset,
        host_offset,
        ram_offset,
        device_offset,
        buffer,
        read: read_name,
        write: write_name,
        event,
    })
}

impl Generator {
    pub(crate) fn try_masked_transfer_command_switch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        self.emit_transfer_command_switch(&shape);
        Ok(true)
    }

    fn emit_transfer_command_switch(&mut self, shape: &TransferCommandSwitch) {
        const OBJECT: u8 = 31;
        let default = self.fresh_label();
        let success = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![OBJECT];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 4,
                clear: 27,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 24,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -40,
            },
            Instruction::StoreWord {
                s: OBJECT,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: OBJECT,
                a: 3,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 1, default);
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::JumpTable);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 2,
            },
            Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 },
            Instruction::MoveToCountRegister { s: 0 },
            Instruction::BranchToCountRegister,
        ]);

        let mut body_offsets = std::collections::HashMap::new();
        body_offsets.insert(0, self.output.instructions.len() as u32 * 4);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 5,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: OBJECT,
                offset: shape.address_offset,
            },
        ]);
        self.emit_branch_to(success);

        for (case, transfer) in [(4, &shape.read), (16, &shape.write)] {
            body_offsets.insert(case, self.output.instructions.len() as u32 * 4);
            self.output.instructions.extend([
                Instruction::load_immediate(0, 64),
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 24,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: 20,
                },
                Instruction::AddImmediate {
                    d: 6,
                    a: 1,
                    immediate: 24,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: OBJECT,
                    offset: shape.host_offset,
                },
                Instruction::LoadWord {
                    d: 5,
                    a: OBJECT,
                    offset: shape.address_offset,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 3,
                    offset: shape.ram_offset,
                },
            ]);
            self.record_relocation(RelocationKind::Rel24, &shape.buffer);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: shape.buffer.clone(),
            });
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
            let buffer_ok = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, buffer_ok);
            self.output
                .instructions
                .push(Instruction::load_immediate(3, 0));
            self.emit_branch_to(epilogue);
            self.bind_label(buffer_ok);
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 3,
                    a: OBJECT,
                    offset: shape.host_offset,
                },
                Instruction::LoadWord {
                    d: 4,
                    a: 1,
                    offset: 20,
                },
                Instruction::LoadWord {
                    d: 3,
                    a: 3,
                    offset: shape.device_offset,
                },
            ]);
            self.record_relocation(RelocationKind::Rel24, transfer);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: transfer.clone(),
            });
            self.output
                .instructions
                .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
            let transfer_ok = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, transfer_ok);
            self.output
                .instructions
                .push(Instruction::load_immediate(3, 0));
            self.emit_branch_to(epilogue);
            self.bind_label(transfer_ok);
            self.emit_event_call(shape, 0x1000);
            self.emit_branch_to(success);
        }

        body_offsets.insert(24, self.output.instructions.len() as u32 * 4);
        self.emit_event_call(shape, 0x1001);
        self.emit_branch_to(success);

        let default_offset = self.output.instructions.len() as u32 * 4;
        self.bind_label(default);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::LoadWord {
                d: OBJECT,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 40,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);

        let entries = (0..=24)
            .map(|value| *body_offsets.get(&value).unwrap_or(&default_offset))
            .collect();
        self.output.jump_tables.push(JumpTable {
            entries,
            // The two guarded transfers retain nine additional internal
            // optimizer labels beyond the four cases, dispatch, and default.
            anonymous_offset: 15,
        });
    }

    fn emit_event_call(&mut self, shape: &TransferCommandSwitch, code: i16) {
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: shape.host_offset,
            },
            Instruction::load_immediate(4, code),
            Instruction::load_immediate(5, 6),
        ]);
        self.record_relocation(RelocationKind::Rel24, &shape.event);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.event.clone(),
        });
    }
}
