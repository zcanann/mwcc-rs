//! Priority-sorted insertion into an intrusive doubly linked SDK queue.

#[allow(unused_imports)]
use super::*;

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
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
    variable(base, base_name).then_some((*offset, *member_type))
}

fn store_value<'a>(
    statement: &'a Statement,
    base_name: &str,
    offset: u32,
) -> Option<&'a Expression> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    (member(target, base_name)?.0 == offset).then_some(value)
}

fn assignment_value<'a>(statement: &'a Statement, name: &str) -> Option<&'a Expression> {
    let Statement::Assign {
        name: assigned,
        value,
    } = statement
    else {
        return None;
    };
    (assigned == name).then_some(value)
}

fn null_comparison(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Binary { operator: BinaryOperator::Equal, left, right }
        if variable(left, name) && integer_constant(right) == Some(0))
}

fn integer_constant(expression: &Expression) -> Option<i64> {
    match expression {
        Expression::Cast { operand, .. } => integer_constant(operand),
        other => constant_value(other),
    }
}

impl Generator {
    /// Lower an empty-bodied priority search followed by the empty/tail and predecessor splice
    /// repairs. Build 145 keeps the iterator in r5, folds `&queue.tail` through `lwzu`, and places
    /// the final predecessor store in the compare/branch shadow.
    pub(crate) fn try_sorted_intrusive_insert(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.parameters.len() != 1
            || function.locals.len() != 2
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let item = &function.parameters[0].name;
        let temporary = &function.locals[0].name;
        let iterator = &function.locals[1].name;
        if !matches!(function.parameters[0].parameter_type, Type::StructPointer { .. })
            || !matches!(function.locals[0].declared_type, Type::StructPointer { .. })
            || !matches!(function.locals[1].declared_type, Type::StructPointer { .. })
        {
            return Ok(false);
        }
        let [loop_statement, empty_if, set_item_next, load_previous, set_iter_previous, set_item_previous, head_if, set_previous_next] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };

        let Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign { target, value }),
            condition: Some(condition),
            step: Some(Expression::Assign { target: step_target, value: step_value }),
            body,
        } = loop_statement
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: queue_base,
            offset: 0,
            index_stride: None,
            ..
        } = value.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Variable(queue) = queue_base.as_ref() else {
            return Ok(false);
        };
        if !body.is_empty()
            || !variable(target, iterator)
            || !variable(step_target, iterator)
            || !self.globals.contains_key(queue.as_str())
        {
            return Ok(false);
        }
        let Some((next_offset, _)) = member(step_value, iterator) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: present,
            right: priority_test,
        } = condition
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::LessEqual,
            left: iter_priority,
            right: item_priority,
        } = priority_test.as_ref()
        else {
            return Ok(false);
        };
        let (Some((priority_offset, _)), Some((item_priority_offset, _))) =
            (member(iter_priority, iterator), member(item_priority, item))
        else {
            return Ok(false);
        };
        if !variable(present.as_ref(), iterator) || priority_offset != item_priority_offset {
            return Ok(false);
        }

        let Statement::If {
            condition: empty_condition,
            then_body: empty_body,
            else_body: empty_else,
        } = empty_if
        else {
            return Ok(false);
        };
        if !empty_else.is_empty() || !null_comparison(empty_condition, iterator) {
            return Ok(false);
        }
        let [load_tail, tail_if, save_previous, clear_next, save_tail, Statement::Return(None)] =
            empty_body.as_slice()
        else {
            return Ok(false);
        };
        let Some(tail_value) = assignment_value(load_tail, temporary) else {
            return Ok(false);
        };
        let Some((tail_offset, _)) = member(tail_value, queue) else {
            return Ok(false);
        };
        let Statement::If {
            condition: tail_condition,
            then_body: empty_queue_body,
            else_body: existing_tail_body,
        } = tail_if
        else {
            return Ok(false);
        };
        let [empty_head_store] = empty_queue_body.as_slice() else {
            return Ok(false);
        };
        let [tail_next_store] = existing_tail_body.as_slice() else {
            return Ok(false);
        };
        let Statement::Store {
            target: save_previous_target,
            value: save_previous_value,
        } = save_previous
        else {
            return Ok(false);
        };
        let Some((previous_offset, _)) = member(save_previous_target, item) else {
            return Ok(false);
        };
        if !null_comparison(tail_condition, temporary)
            || !store_value(empty_head_store, queue, 0).is_some_and(|value| variable(value, item))
            || !store_value(tail_next_store, temporary, next_offset)
                .is_some_and(|value| variable(value, item))
            || !variable(save_previous_value, temporary)
            || !store_value(clear_next, item, next_offset)
                .is_some_and(|value| integer_constant(value) == Some(0))
            || !store_value(save_tail, queue, tail_offset)
                .is_some_and(|value| variable(value, item))
        {
            return Ok(false);
        }

        if !store_value(set_item_next, item, next_offset)
            .is_some_and(|value| variable(value, iterator))
        {
            return Ok(false);
        }
        let Some(previous_value) = assignment_value(load_previous, temporary) else {
            return Ok(false);
        };
        let Some((loaded_previous_offset, _)) = member(previous_value, iterator) else {
            return Ok(false);
        };
        if loaded_previous_offset != previous_offset
            || !store_value(set_iter_previous, iterator, previous_offset)
            .is_some_and(|value| variable(value, item))
            || !store_value(set_item_previous, item, previous_offset)
                .is_some_and(|value| variable(value, temporary))
        {
            return Ok(false);
        }
        let Statement::If {
            condition: head_condition,
            then_body: head_body,
            else_body: head_else,
        } = head_if
        else {
            return Ok(false);
        };
        let [new_head_store, Statement::Return(None)] = head_body.as_slice() else {
            return Ok(false);
        };
        if !head_else.is_empty()
            || !null_comparison(head_condition, temporary)
            || !store_value(new_head_store, queue, 0).is_some_and(|value| variable(value, item))
            || !store_value(set_previous_next, temporary, next_offset)
                .is_some_and(|value| variable(value, item))
        {
            return Ok(false);
        }
        let (Ok(priority), Ok(next), Ok(previous), Ok(tail)) = (
            i16::try_from(priority_offset),
            i16::try_from(next_offset),
            i16::try_from(previous_offset),
            i16::try_from(tail_offset),
        ) else {
            return Ok(false);
        };

        self.output.pre_scheduled = true;
        let loop_step = self.fresh_label();
        let loop_condition = self.fresh_label();
        let after_loop = self.fresh_label();
        let nonempty_insert = self.fresh_label();
        let existing_tail = self.fresh_label();
        let tail_join = self.fresh_label();
        let existing_previous = self.fresh_label();

        self.record_relocation(RelocationKind::EmbSda21, queue);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 0, offset: 0 });
        self.emit_branch_to(loop_condition);
        self.bind_label(loop_step);
        self.output.instructions.push(Instruction::LoadWord { d: 5, a: 5, offset: next });
        self.bind_label(loop_condition);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, after_loop);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 5, offset: priority });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 3, offset: priority });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        self.emit_branch_conditional_to(4, 1, loop_step);

        self.bind_label(after_loop);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, nonempty_insert);
        self.record_relocation(RelocationKind::EmbSda21, queue);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: 4, a: 5, offset: tail });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, existing_tail);
        self.record_relocation(RelocationKind::EmbSda21, queue);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 0, offset: 0 });
        self.emit_branch_to(tail_join);
        self.bind_label(existing_tail);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 4, offset: next });
        self.bind_label(tail_join);
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 3, offset: previous });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: next });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 5, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);

        self.bind_label(nonempty_insert);
        self.output.instructions.push(Instruction::StoreWord { s: 5, a: 3, offset: next });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 5, offset: previous });
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 5, offset: previous });
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 4, a: 3, offset: previous });
        self.emit_branch_conditional_to(4, 2, existing_previous);
        self.record_relocation(RelocationKind::EmbSda21, queue);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 0, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(existing_previous);
        self.output.instructions.push(Instruction::StoreWord { s: 3, a: 4, offset: next });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
