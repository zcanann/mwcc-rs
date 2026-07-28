//! Allocate and append one node to a singly linked list.
//!
//! The allocated node lives in an address-taken frame slot while both incoming
//! pointers survive the allocator call. Legacy MWCC then rotates a pointer-to-
//! pointer cursor through the list and schedules the success return ahead of
//! the link/count stores. Owning the whole transaction here keeps frame layout,
//! saved-register assignment, and the rotated loop coherent.

#[allow(unused_imports)]
use super::*;

struct LinkedListAppend<'a> {
    allocator: &'a str,
    item_size_offset: i16,
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

fn classify(function: &Function) -> Option<LinkedListAppend<'_>> {
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
    let [size, allocated_node, cursor, next] = function.locals.as_slice() else {
        return None;
    };
    if size.declared_type != Type::Int
        || allocated_node.declared_type != Type::Pointer(Pointee::Int)
        || cursor.declared_type != Type::Pointer(Pointee::Int)
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

    let [size_assignment, allocation_guard, clear_node, publish_item, cursor_assignment, walk] =
        function.statements.as_slice()
    else {
        return None;
    };
    let Statement::Assign {
        name: size_name,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: size_member,
                right: node_header_size,
            },
    } = size_assignment
    else {
        return None;
    };
    let (item_size_offset, item_size_type) = member_of(size_member, &list.name)?;
    let node_header_size = i16::try_from(constant_value(node_header_size)?).ok()?;
    if size_name != &size.name || item_size_type != Type::Int || node_header_size <= 0 {
        return None;
    }

    let Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: allocator_call,
            },
        then_body: allocation_failure,
        else_body: allocation_else,
    } = allocation_guard
    else {
        return None;
    };
    let Expression::Call {
        name: allocator,
        arguments: allocator_arguments,
    } = allocator_call.as_ref()
    else {
        return None;
    };
    if !allocation_else.is_empty()
        || !matches!(
            allocation_failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
        || !matches!(
            allocator_arguments.as_slice(),
            [Expression::AddressOf { operand }, size_argument]
                if var(operand, &allocated_node.name) && var(size_argument, &size.name)
        )
    {
        return None;
    }

    let Statement::Store {
        target: clear_target,
        value: clear_value,
    } = clear_node
    else {
        return None;
    };
    if !dereferences_casted_var(clear_target, &allocated_node.name)
        || !is_constant(clear_value, 0)
    {
        return None;
    }
    let Statement::Store {
        target: publish_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: published_node,
                right: published_header_size,
            },
    } = publish_item
    else {
        return None;
    };
    if !dereferences_casted_var(publish_target, &item_out.name)
        || !casted_var(published_node, &allocated_node.name)
        || constant_value(published_header_size) != Some(i64::from(node_header_size))
    {
        return None;
    }

    let Statement::Assign {
        name: cursor_name,
        value: Expression::AddressOf { operand: head_member },
    } = cursor_assignment
    else {
        return None;
    };
    let (head_offset, _) = member_of(head_member, &list.name)?;
    if cursor_name != &cursor.name {
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
    let [load_next, append_if, advance] = walk_body.as_slice() else {
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
                left: tested_next,
                right: null_next,
            },
        then_body: append_body,
        else_body,
    } = append_if
    else {
        return None;
    };
    if !var(tested_next, &next.name) || !is_constant(null_next, 0) || !else_body.is_empty() {
        return None;
    }
    let [link_node, increment_count, success] = append_body.as_slice() else {
        return None;
    };
    if !matches!(
        link_node,
        Statement::Store { target, value }
            if dereferences_casted_var(target, &cursor.name)
                && var(value, &allocated_node.name)
    ) || !matches!(
        success,
        Statement::Return(Some(value)) if is_constant(value, 1)
    ) {
        return None;
    }
    let Statement::Store {
        target: count_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: count_value,
                right: increment,
            },
    } = increment_count
    else {
        return None;
    };
    let (item_count_offset, count_type) = member_of(count_target, &list.name)?;
    if member_of(count_value, &list.name) != Some((item_count_offset, count_type))
        || count_type != Type::Int
        || !is_constant(increment, 1)
    {
        return None;
    }

    Some(LinkedListAppend {
        allocator,
        item_size_offset: i16::try_from(item_size_offset).ok()?,
        item_count_offset: i16::try_from(item_count_offset).ok()?,
        head_offset: i16::try_from(head_offset).ok()?,
        node_header_size,
    })
}

impl Generator {
    pub(crate) fn try_linked_list_append(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.saved_gpr_epilogue_style
                != mwcc_versions::SavedGprEpilogueStyle::LinkRegisterBeforeFinalSaved
        {
            return Ok(false);
        }
        self.emit_linked_list_append(&shape);
        Ok(true)
    }

    fn emit_linked_list_append(&mut self, shape: &LinkedListAppend<'_>) {
        const LIST: u8 = 30;
        const ITEM_OUT: u8 = 31;
        const NODE_SLOT: i16 = 16;
        let allocation_succeeded = self.fresh_label();
        let loop_body = self.fresh_label();
        let loop_condition = self.fresh_label();
        let loop_advance = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![ITEM_OUT, LIST];
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
                s: ITEM_OUT,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: ITEM_OUT,
                a: 4,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: LIST,
                a: 1,
                offset: 24,
            },
            Instruction::move_register(LIST, 3),
            Instruction::LoadWord {
                d: 5,
                a: 3,
                offset: shape.item_size_offset,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: NODE_SLOT,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 5,
                immediate: shape.node_header_size,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.allocator);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.allocator.to_owned(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, allocation_succeeded);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);

        self.bind_label(allocation_succeeded);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::load_immediate(0, 0),
            Instruction::AddImmediate {
                d: 4,
                a: LIST,
                immediate: shape.head_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: shape.node_header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: ITEM_OUT,
                offset: 0,
            },
        ]);
        self.emit_branch_to(loop_condition);

        self.bind_label(loop_body);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, loop_advance);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: NODE_SLOT,
            },
            Instruction::load_immediate(3, 1),
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 4,
                a: LIST,
                offset: shape.item_count_offset,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 1,
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
            .push(Instruction::move_register(4, 0));
        self.bind_label(loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
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
                d: ITEM_OUT,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: LIST,
                a: 1,
                offset: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
