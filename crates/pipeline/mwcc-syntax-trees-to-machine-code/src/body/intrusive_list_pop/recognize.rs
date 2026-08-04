use super::*;

pub(super) fn classify(function: &Function) -> Option<IntrusiveListPop> {
    let [head] = function.parameters.as_slice() else {
        return None;
    };
    let [node] = function.locals.as_slice() else {
        return None;
    };
    if head.parameter_type != Type::Pointer(Pointee::Pointer)
        || function.return_type != node.declared_type
        || !matches!(node.declared_type, Type::StructPointer { .. })
        || node.is_volatile
        || node.is_static
        || node.array_length.is_some()
        || !node.data_bytes.is_none()
        || !node.data_relocations.is_empty()
        || !function.guards.is_empty()
        || function_makes_call(function)
    {
        return None;
    }
    let Expression::Dereference { pointer } = node.initializer.as_ref()? else {
        return None;
    };
    if variable(pointer) != Some(head.name.as_str())
        || variable(function.return_expression.as_ref()?) != Some(node.name.as_str())
    {
        return None;
    }

    let [empty_return, publish_head, clear_owner] = function.statements.as_slice() else {
        return None;
    };
    if !returns_zero_when_null(empty_return, &node.name) {
        return None;
    }
    let Statement::Store {
        target: new_head_target,
        value: new_head,
    } = publish_head
    else {
        return None;
    };
    let Expression::Dereference { pointer: head_target } = new_head_target else {
        return None;
    };
    let (next_base, next_offset, next_type) = member(new_head)?;
    if variable(head_target) != Some(head.name.as_str())
        || variable(next_base) != Some(node.name.as_str())
        || !is_pointer(next_type)
    {
        return None;
    }

    let Statement::Store {
        target: owner_target,
        value: owner_value,
    } = clear_owner
    else {
        return None;
    };
    let (owner_base, owner_offset, owner_type) = member(owner_target)?;
    if variable(owner_base) != Some(node.name.as_str())
        || !is_pointer(owner_type)
        || constant_value(owner_value) != Some(0)
    {
        return None;
    }
    Some(IntrusiveListPop {
        next_offset: i16::try_from(next_offset).ok()?,
        owner_offset: i16::try_from(owner_offset).ok()?,
    })
}

fn returns_zero_when_null(statement: &Statement, node: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    if !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(0))
    {
        return false;
    }
    matches!(condition, Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } if (variable(left) == Some(node) && constant_value(right) == Some(0))
        || (variable(right) == Some(node) && constant_value(left) == Some(0)))
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

fn is_pointer(ty: Type) -> bool {
    matches!(ty, Type::Pointer(_) | Type::StructPointer { .. })
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
    fn rejects_a_pop_that_does_not_clear_the_removed_node() {
        let function = Function {
            return_type: Type::Int,
            name: "not_a_list_pop".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(0)),
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
