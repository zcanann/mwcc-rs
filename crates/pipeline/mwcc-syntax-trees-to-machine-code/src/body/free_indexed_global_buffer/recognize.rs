use super::*;

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn plain_local(local: &LocalDeclaration) -> bool {
    !local.is_volatile
        && !local.is_static
        && local.array_length.is_none()
        && local.data_bytes.is_none()
        && local.data_relocations.is_empty()
}

fn null_pointer(expression: &Expression) -> bool {
    match expression {
        Expression::Cast { operand, .. } => null_pointer(operand),
        _ => constant_value(expression) == Some(0),
    }
}

fn dereference_of(expression: &Expression, pointer: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer: found } if variable(found, pointer))
}

fn direct_call<'a>(statement: &'a Statement, argument: &str) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    matches!(arguments.as_slice(), [value] if variable(value, argument)).then_some(name)
}

fn reset_offsets(function: &Function, stride: i16) -> Option<(i16, i16)> {
    let [object, keep_data] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Void
        || !matches!(object.parameter_type, Type::StructPointer { element_size } if element_size == stride as u32)
        || keep_data.parameter_type != Type::UnsignedChar
        || !function.locals.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [
        Statement::Store {
            target:
                Expression::Member {
                    base: first_base,
                    offset: first_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value: first_value,
        },
        Statement::Store {
            target:
                Expression::Member {
                    base: second_base,
                    offset: second_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value: second_value,
        },
        Statement::If {
            condition:
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand: clear_condition,
                },
            then_body,
            else_body,
        },
    ] = function.statements.as_slice()
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call { arguments, .. })] = then_body.as_slice() else {
        return None;
    };
    if !else_body.is_empty()
        || !variable(first_base, &object.name)
        || !variable(second_base, &object.name)
        || constant_value(first_value) != Some(0)
        || constant_value(second_value) != Some(0)
        || !variable(clear_condition, &keep_data.name)
        || !matches!(arguments.as_slice(), [
            Expression::MemberAddress { base, .. }, zero, size
        ] if variable(base, &object.name)
            && constant_value(zero) == Some(0)
            && constant_value(size).is_some_and(|value| value > 0))
    {
        return None;
    }
    Some((
        i16::try_from(*first_offset).ok()?,
        i16::try_from(*second_offset).ok()?,
    ))
}

pub(super) fn classify(generator: &Generator, function: &Function) -> Option<Plan> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.return_expression.as_ref().is_none()
    {
        return None;
    }
    let [message_id, output] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(message_id.parameter_type, Type::Pointer(_))
        || !matches!(output.parameter_type, Type::Pointer(_))
    {
        return None;
    }
    let [buffer, error, index] = function.locals.as_slice() else {
        return None;
    };
    let Type::StructPointer { element_size } = buffer.declared_type else {
        return None;
    };
    let unavailable = i16::try_from(constant_value(error.initializer.as_ref()?)?).ok()?;
    if error.declared_type != Type::Int
        || index.declared_type != Type::Int
        || buffer.initializer.is_some()
        || index.initializer.is_some()
        || ![buffer, error, index].into_iter().all(plain_local)
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value, &error.name))
    {
        return None;
    }

    let [
        Statement::Store { target: cleared_output, value: null },
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        },
        Statement::If {
            condition: report_condition,
            then_body: report_body,
            else_body: report_else,
        },
    ] = function.statements.as_slice()
    else {
        return None;
    };
    if !dereference_of(cleared_output, &output.name)
        || !null_pointer(null)
        || !matches!(initializer, Expression::Assign { target, value }
            if variable(target, &index.name) && constant_value(value) == Some(0))
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::Less, left, right
        } if variable(left, &index.name) && constant_value(right).is_some())
        || !matches!(step, Expression::Assign { target, value }
            if variable(target, &index.name)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add, left, right
                } if variable(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let Expression::Binary { right: loop_bound, .. } = condition else {
        unreachable!()
    };
    let loop_bound = i16::try_from(constant_value(loop_bound)?).ok()?;

    let [
        Statement::Assign {
            name: assigned_buffer,
            value: Expression::Call { name: lookup, arguments: lookup_arguments },
        },
        acquire_statement,
        Statement::If {
            condition:
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    operand: used,
                },
            then_body,
            else_body,
        },
        release_statement,
    ] = body.as_slice()
    else {
        return None;
    };
    let Expression::Member {
        base: used_base,
        offset: used_offset,
        member_type: Type::Int,
        index_stride: None,
    } = used.as_ref()
    else {
        return None;
    };
    let acquire = direct_call(acquire_statement, &buffer.name)?;
    let release = direct_call(release_statement, &buffer.name)?;
    if assigned_buffer != &buffer.name
        || !matches!(lookup_arguments.as_slice(), [value] if variable(value, &index.name))
        || !variable(used_base, &buffer.name)
        || !else_body.is_empty()
    {
        return None;
    }
    let [
        Statement::Expression(Expression::Call { name: reset, arguments: reset_arguments }),
        Statement::Expression(Expression::Call { name: setter, arguments: setter_arguments }),
        Statement::Assign { name: assigned_error, value: success_value },
        Statement::Store { target: published_buffer, value: published_value },
        Statement::Store { target: published_id, value: published_index },
        Statement::Assign { name: stopped_index, value: stop_value },
    ] = then_body.as_slice()
    else {
        return None;
    };
    let success = i16::try_from(constant_value(success_value)?).ok()?;
    if !matches!(reset_arguments.as_slice(), [object, keep]
        if variable(object, &buffer.name) && constant_value(keep) == Some(1))
        || !matches!(setter_arguments.as_slice(), [object, state]
            if variable(object, &buffer.name) && constant_value(state) == Some(1))
        || assigned_error != &error.name
        || !dereference_of(published_buffer, &output.name)
        || !variable(published_value, &buffer.name)
        || !dereference_of(published_id, &message_id.name)
        || !variable(published_index, &index.name)
        || stopped_index != &index.name
        || constant_value(stop_value) != Some(i64::from(loop_bound))
    {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: report_left,
        right: report_right,
    } = report_condition
    else {
        return None;
    };
    let [Statement::Expression(Expression::Call {
        name: report,
        arguments: report_arguments,
    })] = report_body.as_slice()
    else {
        return None;
    };
    let [Expression::StringLiteral(report_text)] = report_arguments.as_slice() else {
        return None;
    };
    if !report_else.is_empty()
        || !variable(report_left, &error.name)
        || constant_value(report_right) != Some(i64::from(unavailable))
    {
        return None;
    }

    let lookup_body = generator.inline_bodies.definition_body(lookup)?;
    let lookup_shape = super::super::range_guarded_array_address::classify(
        lookup_body,
        &generator.globals,
        &generator.global_array_sizes,
    )?;
    if lookup_shape.bound != loop_bound
        || lookup_shape.stride != i16::try_from(element_size).ok()?
    {
        return None;
    }
    let used_offset = i16::try_from(*used_offset).ok()?;
    if generator
        .inline_bodies
        .definition_body(setter)
        .and_then(super::super::indexed_global_object_initialization::setter_offset)
        != Some(used_offset)
    {
        return None;
    }
    let (length_offset, position_offset) = generator
        .inline_bodies
        .definition_body(reset)
        .and_then(|body| reset_offsets(body, lookup_shape.stride))?;

    Some(Plan {
        array: lookup_shape.array.to_owned(),
        bound: loop_bound,
        stride: lookup_shape.stride,
        used_offset,
        length_offset,
        position_offset,
        unavailable,
        success,
        acquire: acquire.to_owned(),
        release: release.to_owned(),
        report: report.to_owned(),
        report_text: report_text.clone(),
    })
}
