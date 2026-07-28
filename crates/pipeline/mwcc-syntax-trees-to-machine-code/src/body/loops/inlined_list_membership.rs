//! List-item tests composed with a verified skipped-inline registry walk.

#[allow(unused_imports)]
use super::*;

struct InlinedListMembership<'a> {
    helper: &'a str,
    head_offset: i16,
    payload_offset: i16,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn classify(function: &Function) -> Option<InlinedListMembership<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || !matches!(function.return_expression.as_ref(), Some(value)
            if constant_value(value) == Some(0))
    {
        return None;
    }
    let [list, item] = function.parameters.as_slice() else {
        return None;
    };
    let [node] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(list.parameter_type, Type::StructPointer { .. })
        || !matches!(item.parameter_type, Type::Pointer(_))
        || !matches!(node.declared_type, Type::Pointer(_))
        || node.initializer.is_some()
    {
        return None;
    }
    let [guard, Statement::Assign {
        name: assigned_node,
        value:
            Expression::Member {
                base: head_base,
                offset: head_offset,
                index_stride: None,
                ..
            },
    }, Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(loop_condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left,
                right,
            },
        then_body,
        else_body,
    } = guard
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand: helper_call,
    } = left.as_ref()
    else {
        return None;
    };
    let Expression::Call {
        name: helper,
        arguments,
    } = helper_call.as_ref()
    else {
        return None;
    };
    if !matches!(arguments.as_slice(), [argument] if var(argument, &list.name))
        || !matches!(right.as_ref(), Expression::Binary {
            operator: BinaryOperator::Equal, left, right
        } if var(left, &item.name) && constant_value(right) == Some(0))
        || !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(0))
        || assigned_node != &node.name
        || !var(head_base, &list.name)
        || !matches!(loop_condition, Expression::Binary {
            operator: BinaryOperator::NotEqual, left, right
        } if var(left, &node.name) && constant_value(right) == Some(0))
    {
        return None;
    }
    let [match_guard, Statement::Assign {
        name: advanced_node,
        value:
            Expression::Dereference {
                pointer: next_pointer,
            },
    }] = body.as_slice()
    else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left: compared_item,
                right: payload,
            },
        then_body: match_body,
        else_body: match_else,
    } = match_guard
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: payload_node,
        right: payload_offset,
    } = payload.as_ref()
    else {
        return None;
    };
    let Expression::Cast {
        operand: payload_node, ..
    } = payload_node.as_ref()
    else {
        return None;
    };
    if !var(compared_item, &item.name)
        || !var(payload_node, &node.name)
        || !match_else.is_empty()
        || !matches!(match_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(1))
        || advanced_node != &node.name
        || !matches!(next_pointer.as_ref(), Expression::Cast { operand, .. }
            if var(operand, &node.name))
    {
        return None;
    }
    Some(InlinedListMembership {
        helper,
        head_offset: i16::try_from(*head_offset).ok()?,
        payload_offset: i16::try_from(constant_value(payload_offset)?).ok()?,
    })
}

impl Generator {
    pub(crate) fn try_inlined_list_membership(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let Some(helper) = self.inline_summaries.list_membership(shape.helper).cloned() else {
            return Ok(false);
        };
        if !self.skipped_inline_names.contains(shape.helper)
            || !self.frame_slots.is_empty()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || helper.head_offset != shape.head_offset
            || helper.payload_offset != shape.payload_offset
        {
            return Ok(false);
        }

        let helper_global_miss = self.fresh_label();
        let helper_loop_body = self.fresh_label();
        let helper_loop_next = self.fresh_label();
        let helper_loop_condition = self.fresh_label();
        let helper_done = self.fresh_label();
        let caller_fail = self.fresh_label();
        let caller_start = self.fresh_label();
        let caller_loop_body = self.fresh_label();
        let caller_loop_next = self.fresh_label();
        let caller_loop_condition = self.fresh_label();

        self.output.pre_scheduled = true;
        self.record_relocation(RelocationKind::Addr16Ha, &helper.registry);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, &helper.registry);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 5,
                a: 5,
                immediate: 0,
            },
            Instruction::CompareLogicalWord { a: 3, b: 5 },
        ]);
        self.emit_branch_conditional_to(4, 2, helper_global_miss);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(helper_done);
        self.bind_label(helper_global_miss);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: helper.head_offset,
        });
        self.emit_branch_to(helper_loop_condition);
        self.bind_label(helper_loop_body);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 5,
                immediate: helper.payload_offset,
            },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, helper_loop_next);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.emit_branch_to(helper_done);
        self.bind_label(helper_loop_next);
        self.output
            .instructions
            .push(Instruction::LoadWord { d: 5, a: 5, offset: 0 });
        self.bind_label(helper_loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, helper_loop_body);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.bind_label(helper_done);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, caller_fail);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, caller_start);
        self.bind_label(caller_fail);
        self.output.instructions.extend([
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
        ]);
        self.bind_label(caller_start);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.head_offset,
        });
        self.emit_branch_to(caller_loop_condition);
        self.bind_label(caller_loop_body);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: shape.payload_offset,
            },
            Instruction::CompareLogicalWord { a: 4, b: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, caller_loop_next);
        self.output.instructions.extend([
            Instruction::load_immediate(3, 1),
            Instruction::BranchToLinkRegister,
        ]);
        self.bind_label(caller_loop_next);
        self.output
            .instructions
            .push(Instruction::LoadWord { d: 3, a: 3, offset: 0 });
        self.bind_label(caller_loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, caller_loop_body);
        self.output.instructions.extend([
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
