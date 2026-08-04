use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) fn classify(function: &Function) -> Option<FixedListBankTransfer<'_>> {
    let [source, destination] = function.parameters.as_slice() else {
        return None;
    };
    let [object, padding] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || !matches!(source.parameter_type, Type::StructPointer { .. })
        || destination.parameter_type != source.parameter_type
        || !matches!(object.declared_type, Type::StructPointer { .. })
        || object.initializer.is_some()
        || padding.declared_type != Type::Int
        || padding.array_length != Some(2)
        || padding.initializer.is_some()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [first, second, third, fourth, add_first, clear_first, add_second, clear_second,
        Statement::Return(Some(return_value))] = function.statements.as_slice()
    else {
        return None;
    };
    if constant_value(return_value) != Some(0) {
        return None;
    }

    let transfers = [first, second, third, fourth]
        .map(|statement| transfer_loop(statement, source, destination, object))
        .into_iter()
        .collect::<Option<Vec<_>>>()?;
    let [first_transfer, second_transfer, third_transfer, fourth_transfer] =
        transfers.as_slice()
    else {
        return None;
    };
    if transfers.iter().any(|transfer| {
        transfer.take != first_transfer.take
            || transfer.append != first_transfer.append
            || transfer.object_owner_offset != first_transfer.object_owner_offset
    }) {
        return None;
    }
    let list_offsets = [
        first_transfer.list_offset,
        second_transfer.list_offset,
        third_transfer.list_offset,
        fourth_transfer.list_offset,
    ];
    if !list_offsets.windows(2).all(|pair| pair[1] == pair[0] + 4) {
        return None;
    }

    let first_count = counter_transfer(add_first, clear_first, source, destination)?;
    let second_count = counter_transfer(add_second, clear_second, source, destination)?;
    if second_count != first_count + 4 {
        return None;
    }

    Some(FixedListBankTransfer {
        list_offsets,
        count_offsets: [first_count, second_count],
        object_owner_offset: first_transfer.object_owner_offset,
        take: first_transfer.take,
        append: first_transfer.append,
    })
}

struct TransferLoop<'a> {
    list_offset: i16,
    object_owner_offset: i16,
    take: &'a str,
    append: &'a str,
}

fn transfer_loop<'a>(
    statement: &'a Statement,
    source: &Parameter,
    destination: &Parameter,
    object: &LocalDeclaration,
) -> Option<TransferLoop<'a>> {
    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    if constant_value(condition) != Some(1) {
        return None;
    }
    let [Statement::Assign { name, value: take_call }, empty_break, append_statement, owner_store] =
        body.as_slice()
    else {
        return None;
    };
    if name != &object.name || !breaks_when_null(empty_break, &object.name) {
        return None;
    }
    let Expression::Call { name: take, arguments: take_arguments } = take_call else {
        return None;
    };
    let [Expression::AddressOf { operand: source_list }] = take_arguments.as_slice() else {
        return None;
    };
    let (source_base, source_offset) = member(source_list)?;
    if variable(source_base) != Some(source.name.as_str()) {
        return None;
    }

    let (append, append_arguments) = direct_call_statement(append_statement)?;
    let [Expression::AddressOf { operand: destination_list }, appended_object] = append_arguments else {
        return None;
    };
    let (destination_base, destination_offset) = member(destination_list)?;
    if source_offset != destination_offset
        || variable(destination_base) != Some(destination.name.as_str())
        || variable(appended_object) != Some(object.name.as_str())
    {
        return None;
    }

    let (owner_target, owner_value) = store(owner_store)?;
    let (owner_base, object_owner_offset) = member(owner_target)?;
    if variable(owner_base) != Some(object.name.as_str())
        || variable(owner_value) != Some(destination.name.as_str())
    {
        return None;
    }
    Some(TransferLoop {
        list_offset: i16::try_from(source_offset).ok()?,
        object_owner_offset: i16::try_from(object_owner_offset).ok()?,
        take,
        append,
    })
}

fn counter_transfer(
    add: &Statement,
    clear: &Statement,
    source: &Parameter,
    destination: &Parameter,
) -> Option<i16> {
    let (destination_target, added_value) = store(add)?;
    let (destination_base, destination_offset) = member(destination_target)?;
    let Expression::IndexedUpdateValue { value: added_value } = added_value else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: destination_value,
        right: source_value,
    } = added_value.as_ref()
    else {
        return None;
    };
    let (read_destination, read_destination_offset) = member(destination_value)?;
    let (read_source, source_offset) = member(source_value)?;
    if destination_offset != read_destination_offset
        || destination_offset != source_offset
        || variable(destination_base) != Some(destination.name.as_str())
        || variable(read_destination) != Some(destination.name.as_str())
        || variable(read_source) != Some(source.name.as_str())
    {
        return None;
    }

    let (clear_target, clear_value) = store(clear)?;
    let (clear_base, clear_offset) = member(clear_target)?;
    if clear_offset != source_offset
        || variable(clear_base) != Some(source.name.as_str())
        || constant_value(clear_value) != Some(0)
    {
        return None;
    }
    i16::try_from(source_offset).ok()
}

fn breaks_when_null(statement: &Statement, object: &str) -> bool {
    let Statement::If { condition, then_body, else_body } = statement else {
        return false;
    };
    else_body.is_empty()
        && matches!(then_body.as_slice(), [Statement::Break])
        && matches!(condition, Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if variable(left) == Some(object) && constant_value(right) == Some(0))
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

fn store(statement: &Statement) -> Option<(&Expression, &Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    Some((target, value))
}

fn direct_call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_body_without_all_four_list_transfers() {
        let function = Function {
            return_type: Type::Int,
            name: "not_a_bank_transfer".into(),
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
