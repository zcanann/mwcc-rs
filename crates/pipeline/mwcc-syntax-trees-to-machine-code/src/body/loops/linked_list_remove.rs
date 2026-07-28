//! Unlink and free one node from a singly linked list.
//!
//! The address-taken next-node local, pointer-to-pointer cursor, allocator
//! call, and count update form one scheduling transaction in legacy MWCC.
//! Keeping that transaction here also prevents the generic expression
//! lowering from losing the loaded item pointer while it forms the node-data
//! address.

#[allow(unused_imports)]
use super::*;

struct LinkedListRemove<'a> {
    deallocator: &'a str,
    item_count_offset: i16,
    head_offset: i16,
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

fn dereferences_casted_var(expression: &Expression, expected: &str) -> bool {
    matches!(
        expression,
        Expression::Dereference { pointer } if casted_var(pointer, expected)
    )
}

fn member_of(expression: &Expression, base_name: &str) -> Option<(u32, Type)> {
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    var(base, base_name).then_some((*offset, *member_type))
}

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn is_void_zero(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }) if is_constant(operand, 0)
    )
}

fn classify(function: &Function) -> Option<LinkedListRemove<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 0)
    {
        return None;
    }
    let [list, item_out] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(list.parameter_type, Type::StructPointer { .. })
        || item_out.parameter_type != Type::Pointer(Pointee::Pointer)
    {
        return None;
    }
    let [cursor, next] = function.locals.as_slice() else {
        return None;
    };
    if cursor.declared_type != Type::Pointer(Pointee::Int)
        || next.declared_type != Type::Pointer(Pointee::Int)
        || function.locals.iter().any(|local| {
            local.initializer.is_some()
                || local.is_static
                || local.is_volatile
                || local.array_length.is_some()
        })
    {
        return None;
    }

    let [empty_guard, cursor_assignment, walk, no_inline @ ..] = function.statements.as_slice()
    else {
        return None;
    };
    if no_inline.is_empty() || !no_inline.iter().all(is_void_zero) {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: tested_head,
                right: null_head,
            },
        then_body: empty_body,
        else_body: empty_else,
    } = empty_guard
    else {
        return None;
    };
    let (head_offset, _) = member_of(tested_head, &list.name)?;
    if !is_constant(null_head, 0)
        || !empty_else.is_empty()
        || !matches!(
            empty_body.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
    {
        return None;
    }

    let Statement::Assign {
        name: cursor_name,
        value: Expression::AddressOf {
            operand: cursor_head,
        },
    } = cursor_assignment
    else {
        return None;
    };
    if cursor_name != &cursor.name || member_of(cursor_head, &list.name)?.0 != head_offset {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(walk_condition),
        step: None,
        body: walk_body,
    } = walk
    else {
        return None;
    };
    if !matches!(
        walk_condition,
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if var(left, &cursor.name) && is_constant(right, 0)
    ) {
        return None;
    }
    let [load_next, remove_if, advance] = walk_body.as_slice() else {
        return None;
    };
    if !matches!(
        load_next,
        Statement::Assign { name, value }
            if name == &next.name && dereferences_casted_var(value, &cursor.name)
    ) || !matches!(
        advance,
        Statement::Assign { name, value }
            if name == &cursor.name && var(value, &next.name)
    ) {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: published_item,
                right: node_data,
            },
        then_body: remove_body,
        else_body,
    } = remove_if
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: data_node,
        right: header_size,
    } = node_data.as_ref()
    else {
        return None;
    };
    let node_header_size = i16::try_from(constant_value(header_size)?).ok()?;
    if !dereferences_casted_var(published_item, &item_out.name)
        || !casted_var(data_node, &next.name)
        || node_header_size <= 0
        || !else_body.is_empty()
    {
        return None;
    }

    let [unlink, clear_item, free_guard, decrement_count, success] = remove_body.as_slice() else {
        return None;
    };
    if !matches!(
        unlink,
        Statement::Store { target, value }
            if dereferences_casted_var(target, &cursor.name)
                && dereferences_casted_var(value, &next.name)
    ) || !matches!(
        clear_item,
        Statement::Store { target, value }
            if dereferences_casted_var(target, &item_out.name) && is_constant(value, 0)
    ) || !matches!(
        success,
        Statement::Return(Some(value)) if is_constant(value, 1)
    ) {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: free_call,
            },
        then_body: free_failure,
        else_body: free_else,
    } = free_guard
    else {
        return None;
    };
    let Expression::Call {
        name: deallocator,
        arguments,
    } = free_call.as_ref()
    else {
        return None;
    };
    if !free_else.is_empty()
        || !matches!(
            free_failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
        || !matches!(
            arguments.as_slice(),
            [Expression::AddressOf { operand }] if var(operand, &next.name)
        )
    {
        return None;
    }

    let Statement::Store {
        target: count_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left: count_value,
                right: decrement,
            },
    } = decrement_count
    else {
        return None;
    };
    let (item_count_offset, count_type) = member_of(count_target, &list.name)?;
    if member_of(count_value, &list.name) != Some((item_count_offset, count_type))
        || count_type != Type::Int
        || !is_constant(decrement, 1)
    {
        return None;
    }

    Some(LinkedListRemove {
        deallocator,
        item_count_offset: i16::try_from(item_count_offset).ok()?,
        head_offset: i16::try_from(head_offset).ok()?,
        node_header_size,
    })
}

impl Generator {
    pub(crate) fn try_linked_list_remove(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }
        self.emit_linked_list_remove(&shape);
        Ok(true)
    }

    fn emit_linked_list_remove(&mut self, shape: &LinkedListRemove<'_>) {
        const LIST: u8 = 31;
        const NEXT_SLOT: i16 = 16;
        let head_nonempty = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_condition = self.fresh_label();
        let loop_advance = self.fresh_label();
        let free_succeeded = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![LIST];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: LIST,
                a: 1,
                offset: 28,
            },
            Instruction::move_register(LIST, 3),
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.head_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, head_nonempty);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(head_nonempty);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 6,
            a: LIST,
            immediate: shape.head_offset,
        });
        self.emit_branch_to(loop_condition);

        self.bind_label(loop_body);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 6,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: NEXT_SLOT,
            },
            Instruction::LoadWord {
                d: 5,
                a: 1,
                offset: NEXT_SLOT,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 5,
                immediate: shape.node_header_size,
            },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, loop_advance);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: 5,
                offset: 0,
            },
            Instruction::load_immediate(0, 0),
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: NEXT_SLOT,
            },
            Instruction::StoreWord {
                s: 5,
                a: 6,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.deallocator);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.deallocator.to_owned(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, free_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(free_succeeded);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: LIST,
                offset: shape.item_count_offset,
            },
            Instruction::load_immediate(3, 1),
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: -1,
            },
            Instruction::StoreWord {
                s: 0,
                a: LIST,
                offset: shape.item_count_offset,
            },
        ]);
        self.emit_branch_to(epilogue);

        self.bind_label(loop_advance);
        self.output
            .instructions
            .push(Instruction::move_register(6, 5));
        self.bind_label(loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, loop_body);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: LIST,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
