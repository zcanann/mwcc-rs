//! Legacy asynchronous state callbacks with one saved request pointer.
//!
//! This family combines three schedules that cannot be reproduced independently:
//! the outer condition fills build 163's `mflr` latency slot, a two-case state
//! switch rejoins the shared epilogue, and the request pointer survives calls in
//! both the switch and retry paths. Keeping the owner separate avoids teaching
//! the general structured-body walker a build-specific prologue and switch CFG.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

struct AsyncStateCallback<'a> {
    positive_condition: &'a Expression,
    condition_parameter: &'a str,
    saved_parameter: &'a str,
    switch_scrutinee: &'a Expression,
    state: &'a str,
    request: &'a str,
    callback: &'a str,
    read: &'a str,
    zero_state: i16,
    zero_length: i16,
    zero_offset: i16,
    one_state: i16,
    address_member: i16,
    length_member: i16,
    offset_member: i16,
    reset_state: i16,
    reset: &'a str,
    retry: &'a str,
    disk_id: &'a str,
}

fn comparison(
    expression: &Expression,
    name: &str,
    operator: BinaryOperator,
    constant: i64,
) -> bool {
    matches!(expression,
        Expression::Binary { operator: found, left, right }
            if *found == operator
                && matches!(left.as_ref(), Expression::Variable(variable) if variable == name)
                && constant_value(right) == Some(constant))
}

fn direct_call(statement: &Statement) -> Option<(&str, &[Expression])> {
    match statement {
        Statement::Expression(Expression::Call { name, arguments }) => Some((name, arguments)),
        _ => None,
    }
}

fn literal_store(statement: &Statement) -> Option<(&str, i16)> {
    let Statement::Store {
        target: Expression::Variable(global),
        value,
    } = statement
    else {
        return None;
    };
    Some((global, i16::try_from(constant_value(value)?).ok()?))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn member(expression: &Expression, base_name: &str) -> Option<i16> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    (variable(base)? == base_name)
        .then(|| i16::try_from(*offset).ok())
        .flatten()
}

fn rounded_member(expression: &Expression, base_name: &str) -> Option<i16> {
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if constant_value(right).map(|value| value as u32) != Some(!31u32) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = left.as_ref()
    else {
        return None;
    };
    if constant_value(right) != Some(31) {
        return None;
    }
    let member_expression = match left.as_ref() {
        Expression::Cast { operand, .. } => operand.as_ref(),
        other => other,
    };
    member(member_expression, base_name)
}

fn is_void_return_body(body: &[Statement]) -> bool {
    matches!(body, [Statement::Return(None)])
}

fn without_void_return(body: &[Statement]) -> &[Statement] {
    match body.split_last() {
        Some((Statement::Return(None), prefix)) => prefix,
        _ => body,
    }
}

fn recognize(function: &Function) -> Option<AsyncStateCallback<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || !function.locals.is_empty()
    {
        return None;
    }
    let [condition_parameter, saved_parameter] = function.parameters.as_slice() else {
        return None;
    };
    if condition_parameter.parameter_type != Type::Int
        || !matches!(
            saved_parameter.parameter_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
    {
        return None;
    }
    let (positive_condition, then_body, recovery) = match function.statements.as_slice() {
        [Statement::If {
            condition,
            then_body,
            else_body,
        }] => {
            let [recovery] = else_body.as_slice() else {
                return None;
            };
            (condition, then_body, recovery)
        }
        [Statement::If {
            condition,
            then_body,
            else_body,
        }, recovery @ Statement::If { .. }]
            if else_body.is_empty() =>
        {
            (condition, then_body, recovery)
        }
        _ => return None,
    };
    if !comparison(
        positive_condition,
        &condition_parameter.name,
        BinaryOperator::Greater,
        0,
    ) {
        return None;
    }
    let [Statement::Switch {
        scrutinee,
        arms,
        default,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if arms.len() != 2 {
        return None;
    }
    let terminal_default = match default {
        None => false,
        Some(ArmBody::Statements(body)) if is_void_return_body(body) => true,
        _ => return None,
    };
    let mut zero_body = None;
    let mut one_body = None;
    for (arm_index, arm) in arms.iter().enumerate() {
        // Falling out of the final case is semantically the same as an
        // explicit break. Only fallthrough into another case changes this
        // two-state shape.
        if arm.falls_through
            && (arm_index + 1 != arms.len() || default.is_some() && !terminal_default)
        {
            return None;
        }
        let ArmBody::Statements(body) = &arm.body else {
            return None;
        };
        let body = without_void_return(body);
        match arm.value {
            0 if zero_body.is_none() => zero_body = Some(body),
            1 if one_body.is_none() => one_body = Some(body),
            _ => return None,
        }
    }
    let (Some(zero_body), Some(one_body)) = (zero_body, one_body) else {
        return None;
    };
    let [zero_store, zero_call] = zero_body else {
        return None;
    };
    let (state, zero_state) = literal_store(zero_store)?;
    let (read, zero_arguments) = direct_call(zero_call)?;
    let [zero_saved, zero_request, zero_length, zero_offset, zero_callback] = zero_arguments else {
        return None;
    };
    let request = variable(zero_request)?;
    let callback = variable(zero_callback)?;
    if variable(zero_saved) != Some(saved_parameter.name.as_str()) || callback != function.name {
        return None;
    }
    let zero_length = i16::try_from(constant_value(zero_length)?).ok()?;
    let zero_offset = i16::try_from(constant_value(zero_offset)?).ok()?;

    let [one_store, one_call] = one_body else {
        return None;
    };
    let (one_state_name, one_state) = literal_store(one_store)?;
    let (one_read, one_arguments) = direct_call(one_call)?;
    let [one_saved, one_address, one_length, one_offset, one_callback] = one_arguments else {
        return None;
    };
    if one_state_name != state
        || one_read != read
        || variable(one_saved) != Some(saved_parameter.name.as_str())
        || variable(one_callback) != Some(callback)
    {
        return None;
    }
    let address_member = member(one_address, request)?;
    let length_member = rounded_member(one_length, request)?;
    let offset_member = member(one_offset, request)?;

    let Statement::If {
        condition: ignored_condition,
        then_body: ignored_body,
        else_body: retry_else,
    } = recovery
    else {
        return None;
    };
    if !(ignored_body.is_empty() || is_void_return_body(ignored_body))
        || !comparison(
            ignored_condition,
            &condition_parameter.name,
            BinaryOperator::Equal,
            -1,
        )
    {
        return None;
    }
    let [Statement::If {
        condition: retry_condition,
        then_body: retry_body,
        else_body: retry_miss,
    }] = retry_else.as_slice()
    else {
        return None;
    };
    if !retry_miss.is_empty()
        || !comparison(
            retry_condition,
            &condition_parameter.name,
            BinaryOperator::Equal,
            -4,
        )
    {
        return None;
    }
    let [reset_store, reset_call, retry_call] = retry_body.as_slice() else {
        return None;
    };
    let (reset_state_name, reset_state) = literal_store(reset_store)?;
    let (reset, reset_arguments) = direct_call(reset_call)?;
    let (retry, retry_arguments) = direct_call(retry_call)?;
    let [retry_saved, disk_id_expression, retry_callback] = retry_arguments else {
        return None;
    };
    let disk_id = variable(disk_id_expression)?;
    if reset_state_name != state
        || !reset_arguments.is_empty()
        || variable(retry_saved) != Some(saved_parameter.name.as_str())
        || variable(retry_callback) != Some(callback)
    {
        return None;
    }

    Some(AsyncStateCallback {
        positive_condition,
        condition_parameter: &condition_parameter.name,
        saved_parameter: &saved_parameter.name,
        switch_scrutinee: scrutinee,
        state,
        request,
        callback,
        read,
        zero_state,
        zero_length,
        zero_offset,
        one_state,
        address_member,
        length_member,
        offset_member,
        reset_state,
        reset,
        retry,
        disk_id,
    })
}

impl Generator {
    fn emit_async_callback_address(&mut self, callback: &str, destination: u8) {
        self.emit_address_high(3, callback);
        self.record_relocation(RelocationKind::Addr16Lo, callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: destination,
            a: 3,
            immediate: 0,
        });
    }

    fn emit_async_direct_call(&mut self, callee: &str) {
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.to_string(),
        });
    }

    pub(crate) fn try_async_state_callback(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        let Some(condition_register) = self.lookup_general(shape.condition_parameter) else {
            return Ok(false);
        };
        let Some(saved_incoming) = self.lookup_general(shape.saved_parameter) else {
            return Ok(false);
        };
        if condition_register != 3 || saved_incoming != 4 {
            return Ok(false);
        }

        const SAVED_REQUEST: u8 = 31;
        const FRAME_SIZE: i16 = 24;
        self.non_leaf = true;
        self.frame_size = FRAME_SIZE;
        self.callee_saved = vec![SAVED_REQUEST];

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        let (outer_options, outer_condition_bit) =
            self.emit_condition_test(shape.positive_condition)?;
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
                offset: -FRAME_SIZE,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: SAVED_REQUEST,
            a: 1,
            offset: FRAME_SIZE - 4,
        });
        self.emit_callee_saved_home_copy(SAVED_REQUEST, saved_incoming);
        self.locations
            .get_mut(shape.saved_parameter)
            .expect("recognized parameter has an incoming location")
            .register = SAVED_REQUEST;

        let retry = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(outer_options, outer_condition_bit, retry);

        self.evaluate(shape.switch_scrutinee, Type::Int, 0)?;
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        let one = self.fresh_label();
        let zero = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, one);
        self.emit_branch_conditional_to(4, 0, join);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, zero);
        self.emit_branch_to(join);

        self.bind_label(zero);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, shape.zero_state));
        self.emit_global_load(shape.request, 4)?;
        self.emit_address_high(3, shape.callback);
        self.emit_global_store(shape.state, Pointee::Int, 0)?;
        self.record_relocation(RelocationKind::Addr16Lo, shape.callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 3,
            immediate: 0,
        });
        self.emit_callee_saved_home_copy(3, SAVED_REQUEST);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, shape.zero_length));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, shape.zero_offset));
        self.emit_async_direct_call(shape.read);
        self.emit_branch_to(join);

        self.bind_label(one);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, shape.one_state));
        self.emit_global_load(shape.request, 6)?;
        self.emit_global_store(shape.state, Pointee::Int, 0)?;
        self.emit_async_callback_address(shape.callback, 7);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 6,
            offset: shape.length_member,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, SAVED_REQUEST));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 6,
            offset: shape.address_member,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 5,
            immediate: 31,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: 6,
            offset: shape.offset_member,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 0,
            shift: 0,
            begin: 0,
            end: 26,
        });
        self.emit_async_direct_call(shape.read);
        self.emit_branch_to(join);

        self.bind_label(retry);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: condition_register,
                immediate: -1,
            });
        self.emit_branch_conditional_to(12, 2, join);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: condition_register,
                immediate: -4,
            });
        self.emit_branch_conditional_to(4, 2, join);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, shape.reset_state));
        self.emit_global_store(shape.state, Pointee::Int, 0)?;
        self.emit_async_direct_call(shape.reset);
        self.emit_address_high(3, shape.callback);
        self.emit_global_load(shape.disk_id, 4)?;
        self.record_relocation(RelocationKind::Addr16Lo, shape.callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: 0,
        });
        self.emit_callee_saved_home_copy(3, SAVED_REQUEST);
        self.emit_async_direct_call(shape.retry);

        self.bind_label(join);
        // Build 163's state-machine lowering registers the asynchronous read
        // before the recovery calls, even though the AST traversal reaches the
        // reset branch first. Preserve that compilation order for the object
        // symbol table; the relocation stream already has the correct order.
        self.output.symbol_order = [
            shape.state,
            shape.request,
            shape.callback,
            shape.read,
            shape.reset,
            shape.disk_id,
            shape.retry,
        ]
        .into_iter()
        .map(String::from)
        .collect();
        // This first-use K&R call is discovered while build 163 lowers the
        // state dispatch, before it drains the prototyped recovery references.
        self.output
            .early_implicit_external_callees
            .push(shape.read.to_string());
        // Build 163 retains twenty optimizer labels for the nested if/switch
        // CFG. They are not emitted into .text, but they advance the @N stream
        // consumed by later string objects in the translation unit.
        self.output.anonymous_label_bump += 20;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
