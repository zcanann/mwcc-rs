use super::*;

pub(super) fn classify(function: &Function) -> Option<GlobalStructArrayInitialization<'_>> {
    if !function.parameters.is_empty()
        || function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [owner, index] = function.locals.as_slice() else {
        return None;
    };
    let Type::StructPointer { element_size: owner_size } = owner.declared_type else {
        return None;
    };
    if owner.initializer.is_some()
        || owner.is_volatile
        || owner.is_static
        || owner.array_length.is_some()
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || index.is_volatile
        || index.is_static
        || index.array_length.is_some()
    {
        return None;
    }
    let [owner_assignment, owner_init, loop_statement, count_store] =
        function.statements.as_slice()
    else {
        return None;
    };
    let Statement::Assign {
        name,
        value: Expression::AddressOf { operand },
    } = owner_assignment
    else {
        return None;
    };
    if name != &owner.name {
        return None;
    }
    let owner_global = variable(operand)?;
    let (owner_init, owner_arguments) = call_statement(owner_init)?;
    if !matches!(owner_arguments, [argument] if variable(argument) == Some(owner.name.as_str())) {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = loop_statement
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
        left,
        right,
    } = condition
    else {
        return None;
    };
    if variable(left) != Some(index.name.as_str()) {
        return None;
    }
    let count = i16::try_from(constant_value(right)?).ok()?;
    if count <= 0 {
        return None;
    }
    let [element_init, append, owner_store] = body.as_slice() else {
        return None;
    };
    let (element_init, element_arguments) = call_statement(element_init)?;
    let [element_argument] = element_arguments else {
        return None;
    };
    let (array_global, element_index) = addressed_array_element(element_argument)?;
    if element_index != index.name {
        return None;
    }

    let (append, append_arguments) = call_statement(append)?;
    let [list_argument, appended_element] = append_arguments else {
        return None;
    };
    let Expression::AddressOf { operand: list_member } = list_argument else {
        return None;
    };
    let (list_base, list_offset, _) = member(list_member)?;
    if variable(list_base) != Some(owner.name.as_str()) {
        return None;
    }
    let (appended_array, appended_index) = addressed_array_element(appended_element)?;
    if appended_array != array_global || appended_index != index.name {
        return None;
    }

    let Statement::Store {
        target: owner_target,
        value: owner_value,
    } = owner_store
    else {
        return None;
    };
    let Expression::Member {
        base: owner_target_base,
        offset: owner_offset,
        member_type: Type::StructPointer {
            element_size: stored_owner_size,
        },
        index_stride: Some(stride),
    } = owner_target
    else {
        return None;
    };
    let Expression::Index {
        base: owner_array,
        index: owner_index,
    } = owner_target_base.as_ref()
    else {
        return None;
    };
    if variable(owner_array) != Some(array_global)
        || variable(owner_index) != Some(index.name.as_str())
        || variable(owner_value) != Some(owner.name.as_str())
        || *stored_owner_size != owner_size
    {
        return None;
    }

    let Statement::Store {
        target: count_target,
        value: count_value,
    } = count_store
    else {
        return None;
    };
    let (count_base, count_offset, count_type) = member(count_target)?;
    if variable(count_base) != Some(owner.name.as_str())
        || count_type != Type::UnsignedInt
        || constant_value(count_value) != Some(i64::from(count))
    {
        return None;
    }
    Some(GlobalStructArrayInitialization {
        owner_global,
        array_global,
        owner_init,
        element_init,
        append,
        count,
        stride: i16::try_from(*stride).ok()?,
        list_offset: i16::try_from(list_offset).ok()?,
        owner_offset: i16::try_from(*owner_offset).ok()?,
        count_offset: i16::try_from(count_offset).ok()?,
    })
}

fn addressed_array_element(expression: &Expression) -> Option<(&str, &str)> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Index { base, index } = operand.as_ref() else {
        return None;
    };
    Some((variable(base)?, variable(index)?))
}

fn member(expression: &Expression) -> Option<(&Expression, u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    Some((base, *offset, *member_type))
}

fn call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn assigns_constant(expression: &Expression, name: &str, expected: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(expected))
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

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_function() {
        let function = Function {
            return_type: Type::Void,
            name: "not_a_global_array_init".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        assert!(classify(&function).is_none());
    }
}
