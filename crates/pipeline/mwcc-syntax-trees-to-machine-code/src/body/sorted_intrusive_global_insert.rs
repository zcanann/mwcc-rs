//! Priority-sorted insertion into a queue held by three pointer globals.
//!
//! This is the `while`/`break` source topology used by the Revolution DSP task
//! queue. It is the semantic sibling of `sorted_intrusive_insert`, whose SDK
//! queue is written as an empty-bodied `for` followed by splice repairs.

#[allow(unused_imports)]
use super::*;

struct Shape<'a> {
    current: &'a str,
    head: &'a str,
    tail: &'a str,
    priority: i16,
    next: i16,
    previous: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn member(expression: &Expression, base_name: &str) -> Option<(u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    (variable(base)? == base_name).then_some((*offset, *member_type))
}

fn global_store(statement: &Statement) -> Option<(&str, &Expression)> {
    let Statement::Store {
        target: Expression::Variable(name),
        value,
    } = statement
    else {
        return None;
    };
    Some((name, value))
}

fn member_store<'a>(
    statement: &'a Statement,
    base_name: &str,
) -> Option<(u32, Type, &'a Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (offset, member_type) = member(target, base_name)?;
    Some((offset, member_type, value))
}

fn null_variable_comparison(expression: &Expression, name: &str, operator: BinaryOperator) -> bool {
    matches!(expression, Expression::Binary { operator: actual, left, right }
        if *actual == operator
            && variable(left) == Some(name)
            && constant_value(right) == Some(0))
}

fn null_member_comparison(expression: &Expression, base_name: &str, offset: u32) -> bool {
    matches!(expression, Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } if member(left, base_name).is_some_and(|(actual, _)| actual == offset)
        && constant_value(right) == Some(0))
}

fn pointer_type(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}

fn recognize<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<Shape<'a>> {
    if function.return_type != Type::Void
        || function.parameters.len() != 1
        || function.locals.len() != 1
        || !function.guards.is_empty()
        || function.return_expression.is_some()
        || !matches!(
            function.parameters[0].parameter_type,
            Type::StructPointer { .. }
        )
        || !matches!(function.locals[0].declared_type, Type::StructPointer { .. })
    {
        return None;
    }
    let item = function.parameters[0].name.as_str();
    let iterator = function.locals[0].name.as_str();
    let [empty, initialize_iterator, search, append] = function.statements.as_slice() else {
        return None;
    };

    let Statement::If {
        condition: empty_condition,
        then_body: empty_body,
        else_body: empty_else,
    } = empty
    else {
        return None;
    };
    let [publish_current, publish_head_tail, clear_links, Statement::Return(None)] =
        empty_body.as_slice()
    else {
        return None;
    };
    let (current, current_value) = global_store(publish_current)?;
    let (head, head_tail_value) = global_store(publish_head_tail)?;
    let Expression::Assign {
        target: tail_target,
        value: tail_value,
    } = head_tail_value
    else {
        return None;
    };
    let tail = variable(tail_target)?;
    let (next, next_type, clear_previous) = member_store(clear_links, item)?;
    let Expression::Assign {
        target: previous_target,
        value: zero,
    } = clear_previous
    else {
        return None;
    };
    let (previous, previous_type) = member(previous_target, item)?;
    if !empty_else.is_empty()
        || !null_variable_comparison(empty_condition, head, BinaryOperator::Equal)
        || variable(current_value) != Some(item)
        || variable(tail_value) != Some(item)
        || constant_value(zero) != Some(0)
        || next == previous
        || !pointer_type(next_type)
        || !pointer_type(previous_type)
    {
        return None;
    }

    let Statement::Assign {
        name: initialized,
        value: initial_head,
    } = initialize_iterator
    else {
        return None;
    };
    if initialized != iterator || variable(initial_head) != Some(head) {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(search_condition),
        step: None,
        body: search_body,
    } = search
    else {
        return None;
    };
    let [ordered_insert, advance] = search_body.as_slice() else {
        return None;
    };
    if !null_variable_comparison(search_condition, iterator, BinaryOperator::NotEqual) {
        return None;
    }
    let Statement::If {
        condition: priority_condition,
        then_body: insert_body,
        else_body: insert_else,
    } = ordered_insert
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: item_priority,
        right: iterator_priority,
    } = priority_condition
    else {
        return None;
    };
    let (priority, item_priority_type) = member(item_priority, item)?;
    let (iterator_priority_offset, iterator_priority_type) = member(iterator_priority, iterator)?;
    let [save_previous, repair_iterator_previous, save_next, repair_predecessor, Statement::Break] =
        insert_body.as_slice()
    else {
        return None;
    };
    let (saved_previous, saved_previous_type, previous_value) = member_store(save_previous, item)?;
    let (iterator_previous, _) = member(previous_value, iterator)?;
    let (repaired_previous, repaired_previous_type, repaired_previous_value) =
        member_store(repair_iterator_previous, iterator)?;
    let (saved_next, saved_next_type, saved_next_value) = member_store(save_next, item)?;
    let Statement::If {
        condition: predecessor_condition,
        then_body: predecessor_empty,
        else_body: predecessor_present,
    } = repair_predecessor
    else {
        return None;
    };
    let [publish_new_head] = predecessor_empty.as_slice() else {
        return None;
    };
    let (published_head, published_head_value) = global_store(publish_new_head)?;
    let [repair_previous_next] = predecessor_present.as_slice() else {
        return None;
    };
    let Statement::Store {
        target:
            Expression::Member {
                base: previous_base,
                offset: repaired_next,
                member_type: repaired_next_type,
                index_stride: None,
            },
        value: repaired_next_value,
    } = repair_previous_next
    else {
        return None;
    };
    let (previous_base_offset, previous_base_type) = member(previous_base, item)?;
    let Statement::Assign {
        name: advanced,
        value: advance_value,
    } = advance
    else {
        return None;
    };
    let (advance_offset, advance_type) = member(advance_value, iterator)?;
    if !insert_else.is_empty()
        || priority != iterator_priority_offset
        || !matches!(
            (item_priority_type, iterator_priority_type),
            (Type::UnsignedInt, Type::UnsignedInt)
        )
        || saved_previous != previous
        || iterator_previous != previous
        || repaired_previous != previous
        || variable(repaired_previous_value) != Some(item)
        || saved_next != next
        || variable(saved_next_value) != Some(iterator)
        || ![
            saved_previous_type,
            repaired_previous_type,
            saved_next_type,
            previous_base_type,
            *repaired_next_type,
            advance_type,
        ]
        .into_iter()
        .all(pointer_type)
        || !null_member_comparison(predecessor_condition, item, previous)
        || published_head != head
        || variable(published_head_value) != Some(item)
        || previous_base_offset != previous
        || *repaired_next != next
        || variable(repaired_next_value) != Some(item)
        || advanced != iterator
        || advance_offset != next
    {
        return None;
    }

    let Statement::If {
        condition: append_condition,
        then_body: append_body,
        else_body: append_else,
    } = append
    else {
        return None;
    };
    let [repair_tail_next, clear_item_next, save_item_previous, publish_tail] =
        append_body.as_slice()
    else {
        return None;
    };
    let (tail_next, tail_next_type, tail_next_value) = member_store(repair_tail_next, tail)?;
    let (cleared_next, cleared_next_type, cleared_next_value) =
        member_store(clear_item_next, item)?;
    let (saved_item_previous, saved_item_previous_type, saved_item_previous_value) =
        member_store(save_item_previous, item)?;
    let (published_tail, published_tail_value) = global_store(publish_tail)?;
    if !append_else.is_empty()
        || !null_variable_comparison(append_condition, iterator, BinaryOperator::Equal)
        || tail_next != next
        || variable(tail_next_value) != Some(item)
        || cleared_next != next
        || constant_value(cleared_next_value) != Some(0)
        || saved_item_previous != previous
        || variable(saved_item_previous_value) != Some(tail)
        || published_tail != tail
        || variable(published_tail_value) != Some(item)
        || ![tail_next_type, cleared_next_type, saved_item_previous_type]
            .into_iter()
            .all(pointer_type)
        || [current, head, tail]
            .into_iter()
            .any(|name| !globals.get(name).copied().is_some_and(pointer_type))
    {
        return None;
    }

    Some(Shape {
        current,
        head,
        tail,
        priority: i16::try_from(priority).ok()?,
        next: i16::try_from(next).ok()?,
        previous: i16::try_from(previous).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_sorted_intrusive_global_insert(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData {
            return Ok(false);
        }
        let Some(shape) = recognize(function, &self.globals) else {
            return Ok(false);
        };

        self.output.pre_scheduled = true;
        let loop_body = self.fresh_label();
        let loop_step = self.fresh_label();
        let loop_condition = self.fresh_label();
        let join = self.fresh_label();
        let existing_previous = self.fresh_label();

        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_condition);
        self.record_relocation(RelocationKind::EmbSda21, shape.current);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.previous,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.next,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        self.emit_branch_to(loop_condition);

        self.bind_label(loop_body);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: shape.priority,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: shape.priority,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 0, loop_step);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: shape.previous,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.previous,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 5,
            offset: shape.previous,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: shape.previous,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 5,
            a: 3,
            offset: shape.next,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, existing_previous);
        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.emit_branch_to(join);
        self.bind_label(existing_previous);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: shape.next,
        });
        self.emit_branch_to(join);

        self.bind_label(loop_step);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: shape.next,
        });
        self.bind_label(loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_body);

        self.bind_label(join);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            });
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: shape.next,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.next,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.previous,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
