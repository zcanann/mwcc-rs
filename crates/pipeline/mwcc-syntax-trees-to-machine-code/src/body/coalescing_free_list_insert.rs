//! Sorted insertion and adjacent-block coalescing for an intrusive free list.
//!
//! Dolphin's heap allocator keeps `prev`, `next`, and `size` in a three-word
//! cell. Its insertion routine is one indivisible scheduling region: the
//! rotated search feeds two link-repair diamonds, each of which can merge an
//! adjacent block. This owner recognizes the topology and keeps the register
//! and branch schedule out of the general statement driver.

#[allow(unused_imports)]
use super::*;

struct CoalescingFreeListInsert {
    previous_offset: i16,
    next_offset: i16,
    size_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn member(expression: &Expression) -> Option<(&str, i16)> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    Some((variable(base)?, i16::try_from(*offset).ok()?))
}

fn assignment<'a>(expression: &'a Expression, target: &str) -> Option<&'a Expression> {
    let Expression::Assign {
        target: assigned,
        value,
    } = expression
    else {
        return None;
    };
    (variable(assigned) == Some(target)).then_some(value)
}

fn statement_assignment<'a>(statement: &'a Statement, target: &str) -> Option<&'a Expression> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    (name == target).then_some(value)
}

fn member_store<'a>(
    statement: &'a Statement,
    base: &str,
    offset: i16,
) -> Option<&'a Expression> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    (member(target) == Some((base, offset))).then_some(value)
}

fn member_sum(expression: &Expression, destination: &str, left: &str, right: &str, offset: i16) -> bool {
    let expression = peel_indexed_update_provenance(expression);
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: lhs,
        right: rhs,
    } = expression
    else {
        return false;
    };
    destination == left
        && member(lhs) == Some((left, offset))
        && member(rhs) == Some((right, offset))
}

fn adjacent_test(expression: &Expression, left: &str, right: &str, size_offset: i16) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: end,
        right: following,
    } = expression
    else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: base,
        right: size,
    } = end.as_ref()
    else {
        return false;
    };
    variable(base) == Some(left)
        && member(size) == Some((left, size_offset))
        && variable(following) == Some(right)
}

fn single_member_store(
    statements: &[Statement],
    base: &str,
    offset: i16,
    value: &str,
) -> bool {
    matches!(statements,
        [statement] if member_store(statement, base, offset)
            .is_some_and(|expression| variable(expression) == Some(value)))
}

fn classify(function: &Function) -> Option<CoalescingFreeListInsert> {
    let [list_parameter, cell_parameter] = function.parameters.as_slice() else {
        return None;
    };
    let [previous_local, next_local] = function.locals.as_slice() else {
        return None;
    };
    let (list, cell, previous, next) = (
        list_parameter.name.as_str(),
        cell_parameter.name.as_str(),
        previous_local.name.as_str(),
        next_local.name.as_str(),
    );
    if !matches!(function.return_type, Type::StructPointer { .. })
        || !matches!(list_parameter.parameter_type, Type::StructPointer { .. })
        || !matches!(cell_parameter.parameter_type, Type::StructPointer { .. })
        || !matches!(previous_local.declared_type, Type::StructPointer { .. })
        || !matches!(next_local.declared_type, Type::StructPointer { .. })
        || previous_local.initializer.is_some()
        || next_local.initializer.is_some()
        || !function.guards.is_empty()
        || function_makes_call(function)
        || !matches!(function.return_expression.as_ref(), Some(value) if variable(value) == Some(cell))
    {
        return None;
    }
    let [
        loop_statement,
        set_cell_next,
        set_cell_previous,
        next_if,
        previous_if,
    ] = function.statements.as_slice()
    else {
        return None;
    };

    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Comma {
            left: initialize_next,
            right: initialize_previous,
        }),
        condition: Some(loop_condition),
        step:
            Some(Expression::Comma {
                left: step_previous,
                right: step_next,
            }),
        body,
    } = loop_statement
    else {
        return None;
    };
    if variable(assignment(initialize_next, next)?) != Some(list)
        || constant_value(assignment(initialize_previous, previous)?) != Some(0)
        || !matches!(loop_condition,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if variable(left) == Some(next) && constant_value(right) == Some(0))
        || variable(assignment(step_previous, previous)?) != Some(next)
    {
        return None;
    }
    let (step_base, next_offset) = member(assignment(step_next, next)?)?;
    if step_base != next
        || !matches!(body.as_slice(),
            [Statement::If {
                condition:
                    Expression::Binary {
                        operator: BinaryOperator::LessEqual,
                        left,
                        right,
                    },
                then_body,
                else_body,
            }] if variable(left) == Some(cell)
                && variable(right) == Some(next)
                && matches!(then_body.as_slice(), [Statement::Break])
                && else_body.is_empty())
        || member_store(set_cell_next, cell, next_offset)
            .is_none_or(|value| variable(value) != Some(next))
    {
        return None;
    }
    let (previous_base, previous_offset) = member(match set_cell_previous {
        Statement::Store { target, .. } => target,
        _ => return None,
    })?;
    if previous_base != cell
        || member_store(set_cell_previous, cell, previous_offset)
            .is_none_or(|value| variable(value) != Some(previous))
    {
        return None;
    }

    let Statement::If {
        condition: next_condition,
        then_body: next_body,
        else_body: next_else,
    } = next_if
    else {
        return None;
    };
    let [set_next_previous, merge_next_if] = next_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: merge_next_condition,
        then_body: merge_next_body,
        else_body: merge_next_else,
    } = merge_next_if
    else {
        return None;
    };
    let [
        grow_cell,
        load_next,
        repair_cell_next,
        repair_next_if,
    ] = merge_next_body.as_slice()
    else {
        return None;
    };
    let (grow_base, size_offset) = member(match grow_cell {
        Statement::Store { target, .. } => target,
        _ => return None,
    })?;
    if variable(next_condition) != Some(next)
        || !next_else.is_empty()
        || member_store(set_next_previous, next, previous_offset)
            .is_none_or(|value| variable(value) != Some(cell))
        || grow_base != cell
        || !adjacent_test(merge_next_condition, cell, next, size_offset)
        || !merge_next_else.is_empty()
        || member_store(grow_cell, cell, size_offset)
            .is_none_or(|value| !member_sum(value, cell, cell, next, size_offset))
        || member(statement_assignment(load_next, next)?) != Some((next, next_offset))
        || member_store(repair_cell_next, cell, next_offset)
            .is_none_or(|value| variable(value) != Some(next))
        || !matches!(repair_next_if,
            Statement::If {
                condition,
                then_body,
                else_body,
            } if variable(condition) == Some(next)
                && single_member_store(then_body, next, previous_offset, cell)
                && else_body.is_empty())
    {
        return None;
    }

    let Statement::If {
        condition: previous_condition,
        then_body: previous_body,
        else_body: previous_else,
    } = previous_if
    else {
        return None;
    };
    let [set_previous_next, merge_previous_if, Statement::Return(Some(return_list))] =
        previous_body.as_slice()
    else {
        return None;
    };
    let Statement::If {
        condition: merge_previous_condition,
        then_body: merge_previous_body,
        else_body: merge_previous_else,
    } = merge_previous_if
    else {
        return None;
    };
    let [grow_previous, repair_previous_next, repair_following_if] =
        merge_previous_body.as_slice()
    else {
        return None;
    };
    if variable(previous_condition) != Some(previous)
        || !previous_else.is_empty()
        || member_store(set_previous_next, previous, next_offset)
            .is_none_or(|value| variable(value) != Some(cell))
        || !adjacent_test(merge_previous_condition, previous, cell, size_offset)
        || !merge_previous_else.is_empty()
        || member_store(grow_previous, previous, size_offset)
            .is_none_or(|value| !member_sum(value, previous, previous, cell, size_offset))
        || member_store(repair_previous_next, previous, next_offset)
            .is_none_or(|value| variable(value) != Some(next))
        || !matches!(repair_following_if,
            Statement::If {
                condition,
                then_body,
                else_body,
            } if variable(condition) == Some(next)
                && single_member_store(then_body, next, previous_offset, previous)
                && else_body.is_empty())
        || variable(return_list) != Some(list)
    {
        return None;
    }

    Some(CoalescingFreeListInsert {
        previous_offset,
        next_offset,
        size_offset,
    })
}

impl Generator {
    pub(crate) fn try_coalescing_free_list_insert(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let CoalescingFreeListInsert {
            previous_offset,
            next_offset,
            size_offset,
        } = shape;
        self.output.pre_scheduled = true;

        let loop_body = self.fresh_label();
        let loop_condition = self.fresh_label();
        let after_loop = self.fresh_label();
        let after_next_merge = self.fresh_label();
        let after_next_repair = self.fresh_label();
        let return_cell = self.fresh_label();

        self.output.instructions.push(Instruction::AddImmediate {
            d: 7,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.emit_branch_to(loop_condition);
        self.bind_label(loop_body);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 4, b: 7 });
        self.emit_branch_conditional_to(4, 1, after_loop);
        self.output
            .instructions
            .push(Instruction::move_register(6, 7));
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 7,
            offset: next_offset,
        });
        self.bind_label(loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_body);

        self.bind_label(after_loop);
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 4,
            offset: next_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 4,
            offset: previous_offset,
        });
        self.emit_branch_conditional_to(12, 2, after_next_merge);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 7,
            offset: previous_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 4,
            offset: size_offset,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 4, b: 5 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 7 });
        self.emit_branch_conditional_to(4, 2, after_next_merge);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: size_offset,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: size_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 7,
            a: 7,
            offset: next_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 4,
            offset: next_offset,
        });
        self.emit_branch_conditional_to(12, 2, after_next_repair);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 7,
            offset: previous_offset,
        });
        self.bind_label(after_next_repair);
        self.bind_label(after_next_merge);

        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, return_cell);
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 6,
            offset: next_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 6,
            offset: size_offset,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 6, b: 5 });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 4,
                condition_bit: 2,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: size_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 7, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: size_offset,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 6,
            offset: next_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 7,
            offset: previous_offset,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.bind_label(return_cell);
        self.output
            .instructions
            .push(Instruction::move_register(3, 4));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
