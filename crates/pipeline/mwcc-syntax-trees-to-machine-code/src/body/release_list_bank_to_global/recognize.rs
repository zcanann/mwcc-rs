use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) fn classify(function: &Function) -> Option<ReleaseListBankToGlobal<'_>> {
    let [source] = function.parameters.as_slice() else {
        return None;
    };
    let [object] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || !matches!(source.parameter_type, Type::StructPointer { .. })
        || !matches!(object.declared_type, Type::StructPointer { .. })
        || object.initializer.is_some()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [first, second, third, fourth, count_add, count_clear] =
        function.statements.as_slice()
    else {
        return None;
    };
    let first = release_loop(first, source, object, false)?;
    let second = release_loop(second, source, object, false)?;
    let third = release_loop(third, source, object, false)?;
    let fourth = release_loop(fourth, source, object, true)?;
    let releases = [&first, &second, &third, &fourth];
    if releases.iter().any(|release| {
        release.global != first.global
            || release.take != first.take
            || release.append != first.append
            || release.object_owner_offset != first.object_owner_offset
    }) || first.cancel.is_some()
        || second.cancel.is_some()
        || third.cancel.is_some()
    {
        return None;
    }
    let cancel = fourth.cancel?;
    let source_offsets = [
        first.source_offset,
        second.source_offset,
        third.source_offset,
        fourth.source_offset,
    ];
    if !source_offsets.windows(2).all(|pair| pair[1] == pair[0] + 4) {
        return None;
    }
    let destination_offsets = [
        first.destination_offset,
        second.destination_offset,
        third.destination_offset,
        fourth.destination_offset,
    ];
    if destination_offsets[..3] != source_offsets[..3]
        || destination_offsets[3] != destination_offsets[0]
    {
        return None;
    }
    let count_offset = counter_release(count_add, count_clear, source, first.global)?;

    Some(ReleaseListBankToGlobal {
        global: first.global,
        source_offsets,
        destination_offsets,
        count_offset,
        object_owner_offset: first.object_owner_offset,
        take: first.take,
        append: first.append,
        cancel,
    })
}

struct ReleaseLoop<'a> {
    global: &'a str,
    source_offset: i16,
    destination_offset: i16,
    object_owner_offset: i16,
    take: &'a str,
    append: &'a str,
    cancel: Option<&'a str>,
}

fn release_loop<'a>(
    statement: &'a Statement,
    source: &Parameter,
    object: &LocalDeclaration,
    expects_cancel: bool,
) -> Option<ReleaseLoop<'a>> {
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
    let (take_statement, empty_break, cancel_statement, append_statement, owner_store) =
        match (expects_cancel, body.as_slice()) {
            (false, [take, empty, append, owner]) => (take, empty, None, append, owner),
            (true, [take, empty, cancel, append, owner]) => {
                (take, empty, Some(cancel), append, owner)
            }
            _ => return None,
        };
    let Statement::Assign { name, value: take_call } = take_statement else {
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
    let cancel = if let Some(cancel_statement) = cancel_statement {
        let (call, arguments) = direct_call_statement(cancel_statement)?;
        if !single_variable_argument(arguments, &object.name) {
            return None;
        }
        Some(call)
    } else {
        None
    };

    let (append, append_arguments) = direct_call_statement(append_statement)?;
    let [Expression::AddressOf { operand: destination_list }, appended_object] = append_arguments else {
        return None;
    };
    let (destination_base, destination_offset) = member(destination_list)?;
    let global = variable(destination_base)?;
    if variable(appended_object) != Some(object.name.as_str()) {
        return None;
    }
    let (owner_target, owner_value) = store(owner_store)?;
    let (owner_base, object_owner_offset) = member(owner_target)?;
    let Expression::AddressOf { operand: owner_global } = owner_value else {
        return None;
    };
    if variable(owner_base) != Some(object.name.as_str())
        || variable(owner_global) != Some(global)
    {
        return None;
    }
    Some(ReleaseLoop {
        global,
        source_offset: i16::try_from(source_offset).ok()?,
        destination_offset: i16::try_from(destination_offset).ok()?,
        object_owner_offset: i16::try_from(object_owner_offset).ok()?,
        take,
        append,
        cancel,
    })
}

fn counter_release(
    add: &Statement,
    clear: &Statement,
    source: &Parameter,
    global: &str,
) -> Option<i16> {
    let (global_target, global_value) = store(add)?;
    let (global_base, global_offset) = member(global_target)?;
    let Expression::IndexedUpdateValue { value: global_value } = global_value else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: old_global,
        right: source_value,
    } = global_value.as_ref()
    else {
        return None;
    };
    let (old_global_base, old_global_offset) = member(old_global)?;
    let (source_base, source_offset) = member(source_value)?;
    if variable(global_base) != Some(global)
        || variable(old_global_base) != Some(global)
        || variable(source_base) != Some(source.name.as_str())
        || global_offset != old_global_offset
        || global_offset != source_offset
    {
        return None;
    }
    let (clear_target, clear_value) = store(clear)?;
    let (clear_base, clear_offset) = member(clear_target)?;
    if variable(clear_base) != Some(source.name.as_str())
        || clear_offset != source_offset
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

fn single_variable_argument(arguments: &[Expression], expected: &str) -> bool {
    matches!(arguments, [argument] if variable(argument) == Some(expected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_release_without_the_waiting_bank_exception() {
        let function = Function {
            return_type: Type::Int,
            name: "not_a_global_bank_release".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![],
            statements: vec![],
            guards: vec![],
            return_expression: Some(Expression::IntegerLiteral(0)),
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
