//! Semantic recognition for object creation and its two retained helpers.

use super::*;

pub(super) struct InlinedObjectMake {
    pub(super) registry: String,
    pub(super) make_item_callee: String,
    pub(super) make_list_callee: String,
    pub(super) head_offset: i16,
    pub(super) list_offset: i16,
    pub(super) type_offset: i16,
    pub(super) size_offset: i16,
    pub(super) callback_offset: i16,
    pub(super) node_header_size: i16,
    pub(super) create_event: i16,
    pub(super) ready_event: i16,
}

struct Caller {
    find_helper: String,
    make_helper: String,
    make_item_callee: String,
    list_offset: i16,
    callback_offset: i16,
    node_header_size: i16,
    create_event: i16,
    ready_event: i16,
}

struct FindHelper {
    registry: String,
    head_offset: i16,
    type_offset: i16,
    node_header_size: i16,
}

struct MakeHelper {
    registry: String,
    make_item_callee: String,
    make_list_callee: String,
    type_offset: i16,
    size_offset: i16,
    node_header_size: i16,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn casted_var(expression: &Expression, expected: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => casted_var(operand, expected),
        _ => var(expression, expected),
    }
}

fn dereferenced_var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Dereference { pointer } if var(pointer, expected))
}

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn member(expression: &Expression, base_name: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    var(base, base_name).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn negated_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand,
    } = expression
    else {
        return None;
    };
    let Expression::Call { name, arguments } = operand.as_ref() else {
        return None;
    };
    Some((name, arguments))
}

fn helper_call(expression: &Expression, payload: &str, object_type: &str) -> Option<String> {
    let (name, arguments) = negated_call(expression)?;
    matches!(
        arguments,
        [Expression::AddressOf { operand }, type_argument]
            if var(operand, payload) && var(type_argument, object_type)
    )
    .then(|| name.to_owned())
}

fn callback(
    expression: &Expression,
    object_type: &str,
    output: &str,
    event: Option<i64>,
    argument: Option<&str>,
) -> Option<(i16, i16)> {
    let Expression::CallThrough { target, arguments } = expression else {
        return None;
    };
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = target.as_ref()
    else {
        return None;
    };
    if !var(base, object_type)
        || !matches!(member_type, Type::Pointer(_) | Type::StructPointer { .. })
    {
        return None;
    }
    let [object_argument, event_argument, trailing_argument] = arguments.as_slice() else {
        return None;
    };
    if !dereferenced_var(object_argument, output)
        || event.is_some_and(|expected| !is_constant(event_argument, expected))
        || match argument {
            Some(name) => !var(trailing_argument, name),
            None => !is_constant(trailing_argument, 0),
        }
    {
        return None;
    }
    Some((
        i16::try_from(*offset).ok()?,
        i16::try_from(constant_value(event_argument)?).ok()?,
    ))
}

fn classify_caller(function: &Function) -> Option<Caller> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 3
        || function.locals.len() != 4
    {
        return None;
    }
    let [output, argument, object_type] = function.parameters.as_slice() else {
        return None;
    };
    let [created, payload, temporary_object, temporary_payload] = function.locals.as_slice() else {
        return None;
    };
    if output.parameter_type != Type::Pointer(Pointee::Pointer)
        || !matches!(argument.parameter_type, Type::Pointer(_))
        || !matches!(object_type.parameter_type, Type::StructPointer { .. })
        || created.declared_type != Type::Int
        || !matches!(payload.declared_type, Type::StructPointer { .. })
        || !matches!(temporary_object.declared_type, Type::Pointer(_))
        || !matches!(temporary_payload.declared_type, Type::Pointer(_))
        || function.locals.iter().any(|local| {
            local.initializer.is_some()
                || local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }
    let [find_or_make, make_object, save_object, save_payload, publish_object, publish_payload, initialize_if] =
        function.statements.as_slice()
    else {
        return None;
    };

    let Statement::If {
        condition,
        then_body,
        else_body,
    } = find_or_make
    else {
        return None;
    };
    let find_helper = helper_call(condition, &payload.name, &object_type.name)?;
    let [make_guard, set_created] = then_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: make_condition,
        then_body: make_failure,
        else_body: make_else,
    } = make_guard
    else {
        return None;
    };
    let make_helper = helper_call(make_condition, &payload.name, &object_type.name)?;
    if !make_else.is_empty()
        || !matches!(
            make_failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
        || !matches!(
            set_created,
            Statement::Assign { name, value }
                if name == &created.name && is_constant(value, 1)
        )
        || !matches!(
            else_body.as_slice(),
            [Statement::Assign { name, value }]
                if name == &created.name && is_constant(value, 0)
        )
    {
        return None;
    }

    let Statement::If {
        condition: make_object_condition,
        then_body: make_object_failure,
        else_body: make_object_else,
    } = make_object
    else {
        return None;
    };
    let (make_item_callee, make_item_arguments) = negated_call(make_object_condition)?;
    let [list_argument, output_argument] = make_item_arguments else {
        return None;
    };
    let (list_offset, list_type) = member(list_argument, &payload.name)?;
    if !matches!(list_type, Type::Pointer(_) | Type::StructPointer { .. })
        || !var(output_argument, &output.name)
        || !make_object_else.is_empty()
        || !matches!(
            make_object_failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
        || !matches!(
            save_object,
            Statement::Assign { name, value }
                if name == &temporary_object.name && dereferenced_var(value, &output.name)
        )
        || !matches!(
            save_payload,
            Statement::Assign { name, value }
                if name == &temporary_payload.name && var(value, &payload.name)
        )
    {
        return None;
    }

    let Statement::Store {
        target: published_object_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: published_object,
                right: header_size,
            },
    } = publish_object
    else {
        return None;
    };
    let node_header_size = i16::try_from(constant_value(header_size)?).ok()?;
    if node_header_size <= 0
        || !dereferenced_var(published_object_target, &output.name)
        || !matches!(
            published_object.as_ref(),
            Expression::Cast { operand, .. } if dereferenced_var(operand, &output.name)
        )
        || !matches!(
            publish_payload,
            Statement::Store { target, value }
                if matches!(
                    target,
                    Expression::Dereference { pointer } if casted_var(pointer, &temporary_object.name)
                ) && var(value, &temporary_payload.name)
        )
    {
        return None;
    }

    let Statement::If {
        condition: initialize_condition,
        then_body: initialize_body,
        else_body: initialize_else,
    } = initialize_if
    else {
        return None;
    };
    let [Statement::Expression(initialize_callback)] = initialize_body.as_slice() else {
        return None;
    };
    if !var(initialize_condition, &created.name) || !initialize_else.is_empty() {
        return None;
    }
    let (callback_offset, create_event) = callback(
        initialize_callback,
        &object_type.name,
        &output.name,
        Some(0),
        None,
    )?;
    let (final_callback_offset, ready_event) = callback(
        function.return_expression.as_ref()?,
        &object_type.name,
        &output.name,
        Some(2),
        Some(&argument.name),
    )?;
    if final_callback_offset != callback_offset {
        return None;
    }

    Some(Caller {
        find_helper,
        make_helper,
        make_item_callee: make_item_callee.to_owned(),
        list_offset,
        callback_offset,
        node_header_size,
        create_event,
        ready_event,
    })
}

fn classify_find(function: &Function) -> Option<FindHelper> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [payload_out, object_type] = function.parameters.as_slice() else {
        return None;
    };
    let [node] = function.locals.as_slice() else {
        return None;
    };
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Assign {
        target: initialized_target,
        value: initialized_value,
    } = initializer
    else {
        return None;
    };
    let Expression::Variable(initialized_node) = initialized_target.as_ref() else {
        return None;
    };
    let Expression::Member {
        base: registry,
        offset: head_offset,
        index_stride: None,
        ..
    } = initialized_value.as_ref()
    else {
        return None;
    };
    let Expression::Variable(registry) = registry.as_ref() else {
        return None;
    };
    if initialized_node != &node.name
        || !matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if var(left, &node.name) && is_constant(right, 0)
        )
        || !matches!(
            step,
            Expression::Assign { target, value }
                if var(target, &node.name)
                    && matches!(
                        value.as_ref(),
                        Expression::Dereference { pointer } if casted_var(pointer, &node.name)
                    )
        )
    {
        return None;
    }
    let [Statement::Store {
        target: payload_target,
        value: Expression::Cast {
            operand: payload_value,
            ..
        },
    }, Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: found_type,
                right: requested_type,
            },
        then_body: found,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: found_node,
        right: header_size,
    } = payload_value.as_ref()
    else {
        return None;
    };
    let node_header_size = i16::try_from(constant_value(header_size)?).ok()?;
    let (type_offset, _) = member(found_type, &payload_out.name)?;
    if !matches!(
        payload_target,
        Expression::Dereference { pointer } if var(pointer, &payload_out.name)
    ) || !casted_var(found_node, &node.name)
        || !var(requested_type, &object_type.name)
        || !else_body.is_empty()
        || !matches!(
            found.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 1)
        )
    {
        return None;
    }
    Some(FindHelper {
        registry: registry.clone(),
        head_offset: i16::try_from(*head_offset).ok()?,
        type_offset,
        node_header_size,
    })
}

fn classify_make(function: &Function) -> Option<MakeHelper> {
    if function.return_type != Type::Int
        || !is_constant(function.return_expression.as_ref()?, 1)
        || !function.locals.is_empty()
    {
        return None;
    }
    let [payload_out, object_type] = function.parameters.as_slice() else {
        return None;
    };
    let [make_item_guard, store_type] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition,
        then_body: make_item_failure,
        else_body: make_item_else,
    } = make_item_guard
    else {
        return None;
    };
    let (make_item_callee, make_item_arguments) = negated_call(condition)?;
    let [Expression::Variable(registry), output_argument] = make_item_arguments else {
        return None;
    };
    if !casted_var(output_argument, &payload_out.name)
        || !make_item_else.is_empty()
        || !matches!(
            make_item_failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
    {
        return None;
    }
    let Statement::Store {
        target: type_target,
        value: stored_type,
    } = store_type
    else {
        return None;
    };
    let (type_offset, _) = member(type_target, &payload_out.name)?;
    if !var(stored_type, &object_type.name) {
        return None;
    }

    let [guard] = function.guards.as_slice() else {
        return None;
    };
    if !is_constant(&guard.value, 0) {
        return None;
    }
    let (make_list_callee, make_list_arguments) = negated_call(&guard.condition)?;
    let [list_output, size_argument] = make_list_arguments else {
        return None;
    };
    if !matches!(
        list_output,
        Expression::Cast { operand, .. }
            if matches!(
                operand.as_ref(),
                Expression::Dereference { pointer } if var(pointer, &payload_out.name)
            )
    ) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: object_size,
        right: header_size,
    } = size_argument
    else {
        return None;
    };
    let (size_offset, size_type) = member(object_size, &object_type.name)?;
    let node_header_size = i16::try_from(constant_value(header_size)?).ok()?;
    if size_type != Type::Int || node_header_size <= 0 {
        return None;
    }
    Some(MakeHelper {
        registry: registry.clone(),
        make_item_callee: make_item_callee.to_owned(),
        make_list_callee: make_list_callee.to_owned(),
        type_offset,
        size_offset,
        node_header_size,
    })
}

pub(super) fn classify(
    function: &Function,
    inline_bodies: &crate::inline_expansion::InlineBodySet,
) -> Option<InlinedObjectMake> {
    let caller = classify_caller(function)?;
    let find = classify_find(inline_bodies.retained_body(&caller.find_helper)?)?;
    let make = classify_make(inline_bodies.retained_body(&caller.make_helper)?)?;
    if find.registry != make.registry
        || find.type_offset != make.type_offset
        || find.node_header_size != make.node_header_size
        || caller.make_item_callee != make.make_item_callee
        || caller.node_header_size != make.node_header_size
    {
        return None;
    }
    Some(InlinedObjectMake {
        registry: find.registry,
        make_item_callee: make.make_item_callee,
        make_list_callee: make.make_list_callee,
        head_offset: find.head_offset,
        list_offset: caller.list_offset,
        type_offset: find.type_offset,
        size_offset: make.size_offset,
        callback_offset: caller.callback_offset,
        node_header_size: find.node_header_size,
        create_event: caller.create_event,
        ready_event: caller.ready_event,
    })
}
