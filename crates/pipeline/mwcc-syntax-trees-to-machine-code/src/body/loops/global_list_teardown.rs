//! Free each node-owned payload, then free the global list itself.
//!
//! The node cursor remains in r31 across each destructor call. Legacy MWCC
//! advances it only after a successful call and reuses the same callee for the
//! final address-of-global teardown.

#[allow(unused_imports)]
use super::*;

struct GlobalListTeardown {
    global: String,
    callee: String,
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

fn is_constant(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn classify(function: &Function) -> Option<GlobalListTeardown> {
    if function.return_type != Type::Int
        || !function.parameters.is_empty()
        || !is_constant(function.return_expression.as_ref()?, 1)
    {
        return None;
    }
    let [cursor] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(cursor.declared_type, Type::Pointer(_))
        || cursor.initializer.is_some()
        || cursor.is_static
        || cursor.is_volatile
        || cursor.array_length.is_some()
    {
        return None;
    }
    let [Statement::Assign {
        name: cursor_name,
        value:
            Expression::Member {
                base: global_base,
                offset: head_offset,
                member_type: head_type,
                index_stride: None,
            },
    }, Statement::Loop {
        kind: LoopKind::While,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    let Expression::Variable(global) = global_base.as_ref() else {
        return None;
    };
    if cursor_name != &cursor.name
        || !matches!(head_type, Type::Pointer(_))
        || !matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if var(left, &cursor.name) && is_constant(right, 0)
        )
    {
        return None;
    }

    let [Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: free_call,
            },
        then_body: failure,
        else_body,
    }, Statement::Assign {
        name: advance_name,
        value: advance,
    }] = body.as_slice()
    else {
        return None;
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = free_call.as_ref()
    else {
        return None;
    };
    let [payload] = arguments.as_slice() else {
        return None;
    };
    let Expression::Cast {
        operand: payload, ..
    } = payload
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: node,
        right: header_size,
    } = payload.as_ref()
    else {
        return None;
    };
    let node_header_size = i16::try_from(constant_value(header_size)?).ok()?;
    if node_header_size <= 0
        || !casted_var(node, &cursor.name)
        || !else_body.is_empty()
        || !matches!(
            failure.as_slice(),
            [Statement::Return(Some(value))] if is_constant(value, 0)
        )
        || advance_name != &cursor.name
        || !matches!(
            advance,
            Expression::Dereference { pointer } if casted_var(pointer, &cursor.name)
        )
    {
        return None;
    }

    let [guard] = function.guards.as_slice() else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand: final_call,
    } = &guard.condition
    else {
        return None;
    };
    let Expression::Call {
        name: final_callee,
        arguments: final_arguments,
    } = final_call.as_ref()
    else {
        return None;
    };
    if final_callee != callee
        || !is_constant(&guard.value, 0)
        || !matches!(
            final_arguments.as_slice(),
            [Expression::AddressOf { operand }]
                if matches!(operand.as_ref(), Expression::Variable(name) if name == global)
        )
    {
        return None;
    }

    Some(GlobalListTeardown {
        global: global.clone(),
        callee: callee.clone(),
        head_offset: i16::try_from(*head_offset).ok()?,
        node_header_size,
    })
}

impl Generator {
    pub(crate) fn try_global_list_teardown(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if !matches!(
            self.globals.get(&shape.global),
            Some(Type::Pointer(_) | Type::StructPointer { .. })
        ) || !self.frame_slots.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        self.emit_global_list_teardown(&shape);
        Ok(true)
    }

    fn emit_global_list_teardown(&mut self, shape: &GlobalListTeardown) {
        const CURSOR: u8 = 31;
        let loop_body = self.fresh_label();
        let loop_condition = self.fresh_label();
        let final_success = self.fresh_label();
        let epilogue = self.fresh_label();

        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![CURSOR];
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
                offset: -16,
            },
            Instruction::StoreWord {
                s: CURSOR,
                a: 1,
                offset: 12,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, &shape.global);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: CURSOR,
                a: 3,
                offset: shape.head_offset,
            },
        ]);
        self.emit_branch_to(loop_condition);
        self.bind_label(loop_body);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: CURSOR,
            immediate: shape.node_header_size,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        let advance = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, advance);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(advance);
        self.output.instructions.push(Instruction::LoadWord {
            d: CURSOR,
            a: CURSOR,
            offset: 0,
        });
        self.bind_label(loop_condition);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: CURSOR,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, loop_body);

        self.record_relocation(RelocationKind::EmbSda21, &shape.global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, &shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.clone(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, final_success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));
        self.emit_branch_to(epilogue);
        self.bind_label(final_success);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));

        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: CURSOR,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
