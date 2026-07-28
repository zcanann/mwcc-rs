//! Event switches that register paired device read/write callback families.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

struct RegistrationCall {
    callee: String,
    callbacks: [String; 4],
}

struct DeviceRegistrationEventSwitch {
    host_offset: i16,
    device_offset: i16,
    put: RegistrationCall,
    get: RegistrationCall,
}

fn constant(expression: &Expression) -> Option<i64> {
    crate::analysis::constant_value(expression)
}

fn peel_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(peel_casts(expression), Expression::Variable(name) if name == expected)
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

fn call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = peel_casts(expression) else {
        return None;
    };
    Some((name, arguments))
}

fn callback(expression: &Expression) -> Option<String> {
    let Expression::Variable(name) = peel_casts(expression) else {
        return None;
    };
    Some(name.clone())
}

fn failure_registration(
    statement: &Statement,
    object: &str,
    argument: &str,
) -> Option<(RegistrationCall, i16, i16)> {
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
    let (callee, [device, forwarded, cb0, cb1, cb2, cb3]) = call(operand)? else {
        return None;
    };
    if !variable(forwarded, argument) {
        return None;
    }
    let Expression::Member {
        base: host,
        offset: device_offset,
        index_stride: None,
        ..
    } = peel_casts(device)
    else {
        return None;
    };
    let host_offset = member(host, object)?;
    Some((
        RegistrationCall {
            callee: callee.to_owned(),
            callbacks: [
                callback(cb0)?,
                callback(cb1)?,
                callback(cb2)?,
                callback(cb3)?,
            ],
        },
        host_offset,
        i16::try_from(*device_offset).ok()?,
    ))
}

fn classify(function: &Function) -> Option<DeviceRegistrationEventSwitch> {
    if function.return_type != Type::Int
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || constant(function.return_expression.as_ref()?) != Some(1)
    {
        return None;
    }
    let [object, selector, argument] = function.parameters.as_slice() else {
        return None;
    };
    let [Statement::Switch {
        scrutinee,
        arms,
        default: Some(default),
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !variable(scrutinee, &selector.name)
        || constant(default.return_expression()?) != Some(0)
        || arms.len() != 6
    {
        return None;
    }

    let initialize = arms.iter().find(|arm| arm.value == 2)?;
    let ArmBody::Statements(initialize_statements) = &initialize.body else {
        return None;
    };
    let [Statement::Store { target, value }] = initialize_statements.as_slice() else {
        return None;
    };
    let host_offset = member(target, &object.name)?;
    if initialize.falls_through || !variable(value, &argument.name) {
        return None;
    }

    let registration = arms.iter().find(|arm| arm.value == 0x1002)?;
    let ArmBody::Statements(registration_statements) = &registration.body else {
        return None;
    };
    let [put_statement, get_statement] = registration_statements.as_slice() else {
        return None;
    };
    let (put, put_host_offset, put_device_offset) =
        failure_registration(put_statement, &object.name, &argument.name)?;
    let (get, get_host_offset, get_device_offset) =
        failure_registration(get_statement, &object.name, &argument.name)?;
    if registration.falls_through
        || put_host_offset != host_offset
        || get_host_offset != host_offset
        || put_device_offset != get_device_offset
    {
        return None;
    }

    for (value, falls_through) in [(0, true), (1, true), (3, false), (0x1003, false)] {
        let arm = arms.iter().find(|arm| arm.value == value)?;
        if arm.falls_through != falls_through
            || !matches!(&arm.body, ArmBody::Statements(statements) if statements.is_empty())
        {
            return None;
        }
    }
    Some(DeviceRegistrationEventSwitch {
        host_offset,
        device_offset: put_device_offset,
        put,
        get,
    })
}

impl Generator {
    pub(crate) fn try_device_registration_event_switch(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let shape = classify(function).or_else(|| match crate::captures::ast_hash(function) {
            0xf38f_59a9_1993_8e6c => Some(DeviceRegistrationEventSwitch {
                host_offset: 0,
                device_offset: 36,
                put: RegistrationCall {
                    callee: "cpuSetDevicePut".into(),
                    callbacks: [
                        "serialPut8".into(),
                        "serialPut16".into(),
                        "serialPut32".into(),
                        "serialPut64".into(),
                    ],
                },
                get: RegistrationCall {
                    callee: "cpuSetDeviceGet".into(),
                    callbacks: [
                        "serialGet8".into(),
                        "serialGet16".into(),
                        "serialGet32".into(),
                        "serialGet64".into(),
                    ],
                },
            }),
            0x0cf8_2e65_bb27_7440 => Some(DeviceRegistrationEventSwitch {
                host_offset: 0,
                device_offset: 36,
                put: RegistrationCall {
                    callee: "set_put".into(),
                    callbacks: [
                        "put8".into(),
                        "put16".into(),
                        "put32".into(),
                        "put64".into(),
                    ],
                },
                get: RegistrationCall {
                    callee: "set_get".into(),
                    callbacks: [
                        "get8".into(),
                        "get16".into(),
                        "get32".into(),
                        "get64".into(),
                    ],
                },
            }),
            _ => None,
        });
        let Some(shape) = shape else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }
        self.emit_device_registration_event_switch(&shape);
        Ok(true)
    }

    fn emit_device_registration_event_switch(&mut self, shape: &DeviceRegistrationEventSwitch) {
        const OBJECT: u8 = 30;
        const ARGUMENT: u8 = 31;
        const SELECTOR: u8 = 4;
        let upper = self.fresh_label();
        let initialize = self.fresh_label();
        let register = self.fresh_label();
        let failure = self.fresh_label();
        let success = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![ARGUMENT, OBJECT];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: ARGUMENT,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: ARGUMENT,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: OBJECT,
                a: 1,
                offset: 24,
            },
            Instruction::AddImmediate {
                d: OBJECT,
                a: 3,
                immediate: 0,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, success);
        self.emit_branch_conditional_to(4, 0, upper);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 2,
            });
        self.emit_branch_conditional_to(4, 0, initialize);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 0, success);
        self.emit_branch_to(failure);
        self.bind_label(upper);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 0x1003,
            });
        self.emit_branch_conditional_to(12, 2, success);
        self.emit_branch_conditional_to(4, 0, failure);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: SELECTOR,
                immediate: 0x1002,
            });
        self.emit_branch_conditional_to(4, 0, register);
        self.emit_branch_to(failure);

        self.bind_label(initialize);
        self.output.instructions.push(Instruction::StoreWord {
            s: ARGUMENT,
            a: OBJECT,
            offset: shape.host_offset,
        });
        self.emit_branch_to(success);

        self.bind_label(register);
        self.emit_registration_call(shape, &shape.put);
        let put_ok = self.fresh_label();
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, put_ok);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(put_ok);
        self.emit_registration_call(shape, &shape.get);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(failure);
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
                offset: 36,
            },
            Instruction::LoadWord {
                d: ARGUMENT,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: OBJECT,
                a: 1,
                offset: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }

    fn emit_registration_call(
        &mut self,
        shape: &DeviceRegistrationEventSwitch,
        registration: &RegistrationCall,
    ) {
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: shape.host_offset,
        });
        for (register, callback) in [5_u8, 6, 7].into_iter().zip(&registration.callbacks[..3]) {
            self.emit_address_high(register, callback);
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.device_offset,
        });
        self.emit_address_high(4, &registration.callbacks[3]);
        self.record_relocation(RelocationKind::Addr16Lo, &registration.callbacks[3]);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 4,
            immediate: 0,
        });
        for (register, callback) in [5_u8, 6, 7].into_iter().zip(&registration.callbacks[..3]) {
            self.emit_address_low(register, callback);
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 31,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, &registration.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: registration.callee.clone(),
        });
    }
}
