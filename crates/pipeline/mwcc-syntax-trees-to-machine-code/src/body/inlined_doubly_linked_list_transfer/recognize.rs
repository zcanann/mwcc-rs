use super::*;

pub(super) struct ListTransfer<'a> {
    pub(super) heap_array: &'a str,
    pub(super) extract_helper: &'a str,
    pub(super) insert_helper: &'a str,
    pub(super) descriptor_stride: i16,
    pub(super) cell_header_size: i16,
    pub(super) previous_offset: i16,
    pub(super) next_offset: i16,
    pub(super) free_offset: i16,
    pub(super) allocated_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn is_noop(statement: &Statement) -> bool {
    match statement {
        Statement::Expression(Expression::IntegerLiteral(_)) => true,
        Statement::Expression(Expression::Cast { operand, .. }) => {
            matches!(operand.as_ref(), Expression::IntegerLiteral(_))
        }
        _ => false,
    }
}

fn member(expression: &Expression, base: &str) -> Option<(i16, Type)> {
    let Expression::Member {
        base: member_base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    (variable(member_base) == Some(base)).then_some((i16::try_from(*offset).ok()?, *member_type))
}

fn pointer_word(member_type: Type) -> bool {
    matches!(member_type, Type::Pointer(_) | Type::StructPointer { .. })
}

fn adjusted_parameter(expression: &Expression, parameter: &str) -> Option<i16> {
    let expression = match expression {
        Expression::Cast { operand, .. } => operand.as_ref(),
        _ => expression,
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (variable(left) == Some(parameter))
        .then(|| i16::try_from(constant_value(right)?).ok())
        .flatten()
}

fn descriptor_address<'a>(expression: &'a Expression, heap_parameter: &str) -> Option<&'a str> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Index { base, index } = operand.as_ref() else {
        return None;
    };
    (variable(index) == Some(heap_parameter))
        .then(|| variable(base))
        .flatten()
}

fn member_call_store<'a>(
    statement: &'a Statement,
    descriptor: &str,
    cell: &str,
) -> Option<(i16, Type, &'a str)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (target_offset, target_type) = member(target, descriptor)?;
    let Expression::Call { name, arguments } = value else {
        return None;
    };
    let [head, passed_cell] = arguments.as_slice() else {
        return None;
    };
    let (head_offset, head_type) = member(head, descriptor)?;
    if target_offset != head_offset
        || target_type != head_type
        || !pointer_word(target_type)
        || variable(passed_cell) != Some(cell)
    {
        return None;
    }
    Some((target_offset, target_type, name))
}

pub(super) fn classify(function: &Function) -> Option<ListTransfer<'_>> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [heap, pointer] = function.parameters.as_slice() else {
        return None;
    };
    let [descriptor, cell] = function.locals.as_slice() else {
        return None;
    };
    let descriptor_stride = match descriptor.declared_type {
        Type::StructPointer { element_size } => i16::try_from(element_size).ok()?,
        _ => return None,
    };
    if heap.parameter_type != Type::Int
        || !matches!(pointer.parameter_type, Type::Pointer(_))
        || !matches!(cell.declared_type, Type::StructPointer { .. })
        || descriptor_stride <= 0
        || function.locals.iter().any(|local| {
            local.initializer.is_some()
                || local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }

    let mut statements = function
        .statements
        .iter()
        .filter(|statement| !is_noop(statement));
    let cell_assignment = statements.next()?;
    let descriptor_assignment = statements.next()?;
    let extract_store = statements.next()?;
    let insert_store = statements.next()?;
    if statements.next().is_some() {
        return None;
    }
    let Statement::Assign {
        name: cell_name,
        value: cell_value,
    } = cell_assignment
    else {
        return None;
    };
    let Statement::Assign {
        name: descriptor_name,
        value: descriptor_value,
    } = descriptor_assignment
    else {
        return None;
    };
    if cell_name != &cell.name || descriptor_name != &descriptor.name {
        return None;
    }
    let cell_header_size = adjusted_parameter(cell_value, &pointer.name)?;
    let heap_array = descriptor_address(descriptor_value, &heap.name)?;
    let (allocated_offset, _, extract_helper) =
        member_call_store(extract_store, &descriptor.name, &cell.name)?;
    let (free_offset, _, insert_helper) =
        member_call_store(insert_store, &descriptor.name, &cell.name)?;
    if allocated_offset == free_offset || extract_helper == insert_helper {
        return None;
    }

    Some(ListTransfer {
        heap_array,
        extract_helper,
        insert_helper,
        descriptor_stride,
        cell_header_size,
        // Filled from the verified helper summary before emission.
        previous_offset: 0,
        next_offset: 0,
        free_offset,
        allocated_offset,
    })
}
