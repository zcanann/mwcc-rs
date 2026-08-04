use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) fn classify(function: &Function) -> Option<DspChannelParameterUpdate<'_>> {
    let [object] = function.parameters.as_slice() else {
        return None;
    };
    let [channel_id, index] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Void
        || !matches!(object.parameter_type, Type::StructPointer { .. })
        || !matches!(channel_id.declared_type, Type::Int | Type::UnsignedChar)
        || index.declared_type != Type::UnsignedInt
        || index.initializer.is_some()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }

    let (channel_source, body) = if let Some(initializer) = &channel_id.initializer {
        (initializer, function.statements.as_slice())
    } else {
        let [Statement::Assign { name, value }, body @ ..] = function.statements.as_slice() else {
            return None;
        };
        if name != &channel_id.name {
            return None;
        }
        (value, body)
    };
    let (channel, channel_id_offset) = member(channel_source)?;
    let (channel_base, channel_pointer_offset) = member(channel)?;
    if variable(channel_base) != Some(object.name.as_str()) {
        return None;
    }

    let (leading, body) = match body {
        [leading_statement, Statement::Loop { .. }, rest @ ..] => {
            let (call, arguments) = direct_call_statement(leading_statement)?;
            let [id_argument, value_argument] = arguments else {
                return None;
            };
            if variable(id_argument) != Some(channel_id.name.as_str()) {
                return None;
            }
            let (value_base, offset) = member(value_argument)?;
            let (manager_base, _) = member(value_base)?;
            if variable(manager_base) != Some(object.name.as_str()) {
                return None;
            }
            (Some(DirectMemberCall {
                call,
                offset: i16::try_from(offset).ok()?,
            }), std::slice::from_ref(&body[1]).iter().chain(rest.iter()).collect::<Vec<_>>())
        }
        _ => (None, body.iter().collect::<Vec<_>>()),
    };
    let [loop_statement, pitch_statement, iir_statement, fir_statement, mode_statement,
        tail @ ..] = body.as_slice()
    else {
        return None;
    };
    let (mixer, manager_offset, lane_values_offset, lane_modes_offset, lane_count) =
        mixer_loop(loop_statement, object, channel_id, index)?;

    let (pitch, pitch_arguments) = direct_call_statement(pitch_statement)?;
    let [pitch_id, pitch_value] = pitch_arguments else {
        return None;
    };
    let (pitch_base, pitch_offset) = member(pitch_value)?;
    if variable(pitch_id) != Some(channel_id.name.as_str())
        || variable(pitch_base) != Some(object.name.as_str())
    {
        return None;
    }

    let conditional_iir = conditional_filter_call(iir_statement, object, channel_id, 32)?;
    let conditional_fir = conditional_filter_call(fir_statement, object, channel_id, 31)?;
    if conditional_iir.manager_offset != manager_offset
        || conditional_fir.manager_offset != manager_offset
        || conditional_iir.mode_offset != conditional_fir.mode_offset
    {
        return None;
    }

    let (mode, mode_arguments) = direct_call_statement(mode_statement)?;
    let [mode_id, mode_value] = mode_arguments else {
        return None;
    };
    let (mode_manager, filter_mode_offset) = member(mode_value)?;
    let (mode_base, mode_manager_offset) = member(mode_manager)?;
    if variable(mode_id) != Some(channel_id.name.as_str())
        || variable(mode_base) != Some(object.name.as_str())
        || mode_manager_offset != manager_offset
        || filter_mode_offset != conditional_iir.mode_offset
    {
        return None;
    }

    let (distance, pause_statement) = match tail {
        [distance_statement, pause_statement] => {
            let (call, arguments) = direct_call_statement(distance_statement)?;
            let [distance_id, distance_value] = arguments else {
                return None;
            };
            let (distance_manager, offset) = member(distance_value)?;
            let (distance_base, distance_manager_offset) = member(distance_manager)?;
            if variable(distance_id) != Some(channel_id.name.as_str())
                || variable(distance_base) != Some(object.name.as_str())
                || distance_manager_offset != manager_offset
            {
                return None;
            }
            (Some(DirectMemberCall {
                call,
                offset: i16::try_from(offset).ok()?,
            }), *pause_statement)
        }
        [pause_statement] => (None, *pause_statement),
        _ => return None,
    };
    let (pause, pause_arguments) = direct_call_statement(pause_statement)?;
    let [pause_id, pause_value] = pause_arguments else {
        return None;
    };
    let (pause_base, pause_offset) = member(pause_value)?;
    if variable(pause_id) != Some(channel_id.name.as_str())
        || variable(pause_base) != Some(object.name.as_str())
        || leading.is_some() == distance.is_some()
    {
        return None;
    }

    Some(DspChannelParameterUpdate {
        channel_pointer_offset: i16::try_from(channel_pointer_offset).ok()?,
        channel_id_offset: i16::try_from(channel_id_offset).ok()?,
        manager_offset: i16::try_from(manager_offset).ok()?,
        lane_values_offset: i16::try_from(lane_values_offset).ok()?,
        lane_modes_offset: i16::try_from(lane_modes_offset).ok()?,
        lane_count,
        pitch_offset: i16::try_from(pitch_offset).ok()?,
        filter_mode_offset: i16::try_from(filter_mode_offset).ok()?,
        iir_offset: conditional_iir.value_offset,
        fir_offset: conditional_fir.value_offset,
        pause_offset: i16::try_from(pause_offset).ok()?,
        leading,
        mixer,
        pitch,
        iir: conditional_iir.call,
        fir: conditional_fir.call,
        mode,
        distance,
        pause,
    })
}

fn mixer_loop<'a>(
    statement: &'a Statement,
    object: &Parameter,
    channel_id: &LocalDeclaration,
    index: &LocalDeclaration,
) -> Option<(&'a str, u32, u32, u32, u16)> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    if !assigns_constant(initializer, &index.name, 0)
        || !increments_by_one(step, &index.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: loop_index,
        right: lane_count,
    } = condition
    else {
        return None;
    };
    if variable(loop_index) != Some(index.name.as_str()) {
        return None;
    }
    let lane_count = u16::try_from(constant_value(lane_count)?).ok()?;
    let [mixer_statement] = body.as_slice() else {
        return None;
    };
    let (mixer, arguments) = direct_call_statement(mixer_statement)?;
    let [id_argument, index_argument, lane_value, lane_mode] = arguments else {
        return None;
    };
    if variable(id_argument) != Some(channel_id.name.as_str())
        || variable(index_argument) != Some(index.name.as_str())
    {
        return None;
    }
    let (lane_values_base, lane_values_offset, lane_value_index) = indexed_member_address(lane_value)?;
    let (lane_modes_manager, lane_modes_offset, lane_mode_index) = indexed_member_address(lane_mode)?;
    let (lane_modes_base, manager_offset) = member(lane_modes_manager)?;
    if variable(lane_values_base) != Some(object.name.as_str())
        || variable(lane_value_index) != Some(index.name.as_str())
        || variable(lane_modes_base) != Some(object.name.as_str())
        || variable(lane_mode_index) != Some(index.name.as_str())
    {
        return None;
    }
    Some((mixer, manager_offset, lane_values_offset, lane_modes_offset, lane_count))
}

struct ConditionalFilter<'a> {
    call: &'a str,
    manager_offset: u32,
    mode_offset: u32,
    value_offset: i16,
}

fn conditional_filter_call<'a>(
    statement: &'a Statement,
    object: &Parameter,
    channel_id: &LocalDeclaration,
    expected_mask: i64,
) -> Option<ConditionalFilter<'a>> {
    let Statement::If { condition, then_body, else_body } = statement else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let masked = nonzero_operand(condition);
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: mode_value,
        right: mask,
    } = masked
    else {
        return None;
    };
    if constant_value(mask) != Some(expected_mask) {
        return None;
    }
    let (mode_manager, mode_offset) = member(mode_value)?;
    let (mode_base, manager_offset) = member(mode_manager)?;
    if variable(mode_base) != Some(object.name.as_str()) {
        return None;
    }
    let [call_statement] = then_body.as_slice() else {
        return None;
    };
    let (call, arguments) = direct_call_statement(call_statement)?;
    let [id_argument, value_argument] = arguments else {
        return None;
    };
    let (value_manager, value_offset) = zero_index_member_address(value_argument)?;
    let (value_base, value_manager_offset) = member(value_manager)?;
    if variable(id_argument) != Some(channel_id.name.as_str())
        || variable(value_base) != Some(object.name.as_str())
        || value_manager_offset != manager_offset
    {
        return None;
    }
    Some(ConditionalFilter {
        call,
        manager_offset,
        mode_offset,
        value_offset: i16::try_from(value_offset).ok()?,
    })
}

fn zero_index_member_address(expression: &Expression) -> Option<(&Expression, u32)> {
    match expression {
        Expression::MemberAddress { base, offset, .. } => Some((base, *offset)),
        Expression::AddressOf { operand } => {
            let Expression::Index { base, index } = operand.as_ref() else {
                return None;
            };
            let Expression::MemberAddress { base, offset, .. } = base.as_ref() else {
                return None;
            };
            (constant_value(index) == Some(0)).then_some((base, *offset))
        }
        _ => None,
    }
}

fn nonzero_operand(expression: &Expression) -> &Expression {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if constant_value(right) == Some(0) => left,
        _ => expression,
    }
}

fn indexed_member_address(expression: &Expression) -> Option<(&Expression, u32, &Expression)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::MemberAddress { base, offset, .. } = base.as_ref() else {
        return None;
    };
    Some((base, *offset, index))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member { base, offset, index_stride: None, .. } = expression else {
        return None;
    };
    Some((base, *offset))
}

fn direct_call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn assigns_constant(expression: &Expression, name: &str, constant: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(constant))
}

fn increments_by_one(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name)
            && matches!(value.as_ref(), Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if variable(left) == Some(name) && constant_value(right) == Some(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_incomplete_parameter_publication() {
        let function = Function {
            return_type: Type::Void,
            name: "not_a_dsp_update".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![],
            statements: vec![],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(classify(&function).is_none());
    }
}
