//! Status-guarded indexed call loops.
//!
//! These loops stop on either a nonzero callee result or a signed element count.
//! The element cursor, index, count, and forwarded context occupy r31..r28;
//! the status remains in r3 across the bottom-tested condition.

#[allow(unused_imports)]
use super::*;

struct StatusIndexedCall<'a> {
    callee: &'a str,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn assign_constant(expression: &Expression, name: &str, constant: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if var(target, name) && constant_value(value) == Some(constant))
}

fn classify(function: &Function) -> Option<StatusIndexedCall<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [context, data, count] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        context.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || data.parameter_type != Type::Pointer(Pointee::UnsignedInt)
        || count.parameter_type != Type::Int
    {
        return None;
    }
    let [status, index] = function.locals.as_slice() else {
        return None;
    };
    if status.declared_type != Type::Int
        || status.initializer.is_some()
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &status.name))
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(initializer, Expression::Comma { left, right }
        if assign_constant(left, &index.name, 0)
            && assign_constant(right, &status.name, 0))
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::LogicalAnd, left, right
        } if matches!(left.as_ref(), Expression::Binary {
                operator: BinaryOperator::Equal, left, right
            } if var(left, &status.name) && constant_value(right) == Some(0))
            && matches!(right.as_ref(), Expression::Binary {
                operator: BinaryOperator::Less, left, right
            } if var(left, &index.name) && var(right, &count.name)))
        || !matches!(step, Expression::Assign { target, value }
            if var(target, &index.name)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add, left, right
                } if var(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_status,
        value: Expression::Call {
            name: callee,
            arguments,
        },
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_status != &status.name
        || !matches!(arguments.as_slice(), [call_context, Expression::Index { base, index: call_index }]
            if var(call_context, &context.name) && var(base, &data.name)
                && var(call_index, &index.name))
    {
        return None;
    }
    Some(StatusIndexedCall { callee })
}

impl Generator {
    pub(crate) fn try_status_indexed_call_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        const CURSOR: u8 = 31;
        const INDEX: u8 = 30;
        const COUNT: u8 = 29;
        const CONTEXT: u8 = 28;
        let body = self.fresh_label();
        let condition = self.fresh_label();
        let exit = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![CURSOR, INDEX, COUNT, CONTEXT];
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
                offset: -24,
            },
            Instruction::StoreWord {
                s: CURSOR,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: INDEX,
                a: 1,
                offset: 16,
            },
            Instruction::load_immediate(INDEX, 0),
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: INDEX,
                shift: 2,
            },
            Instruction::StoreWord {
                s: COUNT,
                a: 1,
                offset: 12,
            },
            Instruction::Add {
                d: CURSOR,
                a: 4,
                b: 0,
            },
            Instruction::AddImmediate {
                d: COUNT,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: CONTEXT,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: CONTEXT,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate(3, 0),
        ]);
        self.emit_branch_to(condition);
        self.bind_label(body);
        self.output.instructions.extend([
            Instruction::move_register(3, CONTEXT),
            Instruction::LoadWord {
                d: 4,
                a: CURSOR,
                offset: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: INDEX,
                a: INDEX,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: CURSOR,
                a: CURSOR,
                immediate: 4,
            },
        ]);
        self.bind_label(condition);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, exit); // bne
        self.output
            .instructions
            .push(Instruction::CompareWord { a: INDEX, b: COUNT });
        self.emit_branch_conditional_to(12, 0, body); // blt
        self.bind_label(exit);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: CURSOR,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: INDEX,
                a: 1,
                offset: 16,
            },
            Instruction::LoadWord {
                d: COUNT,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: CONTEXT,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
