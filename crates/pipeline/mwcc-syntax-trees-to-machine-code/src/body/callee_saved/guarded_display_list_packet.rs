//! Guarded two-command display-list packet emission.
//!
//! This family keeps the actor, graph, and packet cursor in r29-r31 across
//! matrix and render-state calls. Macro-expanded source contains several empty
//! do/while sentinels and dead bookkeeping locals; recognition removes only
//! those inert statements before matching the packet transaction.

#[allow(unused_imports)]
use super::*;

struct GuardedDisplayListPacket<'a> {
    initialized_offset: i16,
    graph_offset: i16,
    matrix_offset: i16,
    position_offset: i16,
    head_offset: i16,
    matrix_put: &'a str,
    position_zero: &'a str,
    texture_setup: &'a str,
    matrix_to_packet: &'a str,
    first_command: u32,
    second_command: u32,
    model: &'a str,
}

impl Generator {
    pub(crate) fn try_guarded_display_list_packet(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![29, 30, 31];
        self.output.pre_scheduled = true;

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_29");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_29".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 3));
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.initialized_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, epilogue);

        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 4,
            offset: shape.graph_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: shape.matrix_offset,
        });
        self.emit_direct_call(shape.matrix_put);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: shape.position_offset,
        });
        self.emit_direct_call(shape.position_zero);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 29,
            offset: shape.initialized_offset,
        });
        self.emit_direct_call(shape.texture_setup);

        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 31,
            offset: shape.head_offset,
        });
        let (first_high, first_low) = crate::expressions::split_address(shape.first_command);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, first_high));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: first_low,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 30));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 30,
            immediate: 8,
        });
        self.emit_direct_call(shape.matrix_to_packet);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 29,
            offset: 4,
        });

        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        let (second_high, second_low) = crate::expressions::split_address(shape.second_command);
        debug_assert_eq!(second_low, 0);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, second_high));
        self.emit_address_high(3, shape.model);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.model);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 30,
            immediate: 8,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 31,
            offset: shape.head_offset,
        });

        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_29");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_29".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_direct_call(&mut self, target: &str) {
        self.record_relocation(RelocationKind::Rel24, target);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: target.to_string(),
        });
    }
}

fn recognize(function: &Function) -> Option<GuardedDisplayListPacket<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [actor, game] = function.parameters.as_slice() else {
        return None;
    };
    let alias = function.locals.iter().find(|local| {
        matches!(local.initializer.as_ref(), Some(Expression::Cast { operand, .. })
            if matches!(operand.as_ref(), Expression::Variable(name) if name == &actor.name))
    })?;
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            },
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() || constant_value(right) != Some(1) {
        return None;
    }
    let initialized_offset = member_offset(left, &alias.name)?;
    let meaningful: Vec<&Statement> = then_body
        .iter()
        .filter(|statement| !inert_macro_statement(statement, then_body, &function.locals))
        .collect();
    let [graph_assign, graph_alias_assign, matrix_put, position_zero, clear_initialized, texture_setup, cursor_assign, first_packet_assign, first_command_store, first_matrix_store, second_packet_assign, second_command_store, model_store, cursor_commit] =
        meaningful.as_slice()
    else {
        return None;
    };

    let (graph, graph_value) = assignment(graph_assign)?;
    let graph_offset = member_offset(graph_value, &game.name)?;
    let (graph_alias, graph_alias_value) = assignment(graph_alias_assign)?;
    if !is_variable(graph_alias_value, graph) {
        return None;
    }
    let (matrix_put, matrix_arguments) = call_statement(matrix_put)?;
    let [matrix_argument] = matrix_arguments else {
        return None;
    };
    let matrix_offset = addressed_member_offset(matrix_argument, &alias.name)?;
    let (position_zero, position_arguments) = call_statement(position_zero)?;
    let [position_argument] = position_arguments else {
        return None;
    };
    let position_offset = addressed_member_offset(position_argument, &alias.name)?;
    let (clear_target, clear_offset, clear_value) = member_store(clear_initialized)?;
    if !is_variable(clear_target, &alias.name)
        || clear_offset != initialized_offset
        || constant_value(clear_value) != Some(0)
    {
        return None;
    }
    let (texture_setup, texture_arguments) = call_statement(texture_setup)?;
    if !matches!(texture_arguments, [argument] if is_variable(argument, graph)) {
        return None;
    }

    let (cursor, cursor_value) = assignment(cursor_assign)?;
    let (cursor_graph_alias, aggregate_offset, field_offset) = graph_head(cursor_value)?;
    if cursor_graph_alias != graph_alias {
        return None;
    }
    let head_offset = aggregate_offset.checked_add(field_offset)?;
    let (first_packet, first_cursor) = poststep_assignment(first_packet_assign)?;
    if first_cursor != cursor {
        return None;
    }
    let (first_target, first_offset, first_value) = member_store(first_command_store)?;
    if !is_variable(first_target, first_packet) || first_offset != 0 {
        return None;
    }
    let first_command = unsigned_constant(first_value)?;
    let (first_matrix_target, first_matrix_offset, first_matrix_value) =
        member_store(first_matrix_store)?;
    if !is_variable(first_matrix_target, first_packet) || first_matrix_offset != 4 {
        return None;
    }
    let Expression::Cast { operand, .. } = first_matrix_value else {
        return None;
    };
    let Expression::Call {
        name: matrix_to_packet,
        arguments: matrix_arguments,
    } = operand.as_ref()
    else {
        return None;
    };
    if !matches!(matrix_arguments.as_slice(), [argument] if is_variable(argument, graph)) {
        return None;
    }

    let (second_packet, second_cursor) = poststep_assignment(second_packet_assign)?;
    if second_cursor != cursor {
        return None;
    }
    let (second_target, second_offset, second_value) = member_store(second_command_store)?;
    if !is_variable(second_target, second_packet) || second_offset != 0 {
        return None;
    }
    let second_command = unsigned_constant(second_value)?;
    if crate::expressions::split_address(second_command).1 != 0 {
        return None;
    }
    let (model_target, model_offset, model_value) = member_store(model_store)?;
    if !is_variable(model_target, second_packet) || model_offset != 4 {
        return None;
    }
    let Expression::Cast { operand, .. } = model_value else {
        return None;
    };
    let Expression::Variable(model) = operand.as_ref() else {
        return None;
    };
    let (commit_target, commit_offset, commit_value) = member_store(cursor_commit)?;
    let (commit_graph_alias, commit_aggregate) = addressed_aggregate(commit_target)?;
    if commit_offset != field_offset
        || commit_graph_alias != graph_alias
        || commit_aggregate != aggregate_offset
        || !matches!(commit_value, Expression::Cast { operand, .. } if is_variable(operand, cursor))
    {
        return None;
    }

    Some(GuardedDisplayListPacket {
        initialized_offset: i16::try_from(initialized_offset).ok()?,
        graph_offset: i16::try_from(graph_offset).ok()?,
        matrix_offset: i16::try_from(matrix_offset).ok()?,
        position_offset: i16::try_from(position_offset).ok()?,
        head_offset: i16::try_from(head_offset).ok()?,
        matrix_put,
        position_zero,
        texture_setup,
        matrix_to_packet,
        first_command,
        second_command,
        model,
    })
}

fn inert_macro_statement(
    statement: &Statement,
    body: &[Statement],
    locals: &[LocalDeclaration],
) -> bool {
    if matches!(statement, Statement::Loop {
        kind: LoopKind::DoWhile,
        condition: Some(condition),
        body,
        ..
    } if body.is_empty() && constant_value(condition) == Some(0))
    {
        return true;
    }
    let dead_int_local = |name: &str| {
        locals.iter().any(|local| local.name == name && local.declared_type == Type::Int)
            && body.iter().any(|candidate| matches!(candidate,
                Statement::Expression(Expression::Cast { target_type: Type::Void, operand })
                    if is_variable(operand, name)))
    };
    matches!(statement, Statement::Assign { name, value }
        if constant_value(value) == Some(0) && dead_int_local(name))
        || matches!(statement,
            Statement::Expression(Expression::Cast { target_type: Type::Void, operand })
                if matches!(operand.as_ref(), Expression::Variable(name) if dead_int_local(name)))
}

fn assignment(statement: &Statement) -> Option<(&str, &Expression)> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    Some((name, value))
}

fn call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn member_store(statement: &Statement) -> Option<(&Expression, u32, &Expression)> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                ..
            },
        value,
    } = statement
    else {
        return None;
    };
    Some((base, *offset, value))
}

fn member_offset(expression: &Expression, base_name: &str) -> Option<u32> {
    let Expression::Member { base, offset, .. } = expression else {
        return None;
    };
    is_variable(base, base_name).then_some(*offset)
}

fn addressed_member_offset(expression: &Expression, base_name: &str) -> Option<u32> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    member_offset(operand, base_name)
}

fn graph_head(expression: &Expression) -> Option<(&str, u32, u32)> {
    let expression = match expression {
        Expression::Cast { operand, .. } => operand.as_ref(),
        other => other,
    };
    let Expression::Member {
        base,
        offset: field_offset,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::AddressOf { operand } = base.as_ref() else {
        return None;
    };
    let Expression::Member {
        base,
        offset: aggregate_offset,
        ..
    } = operand.as_ref()
    else {
        return None;
    };
    let Expression::Variable(graph_alias) = base.as_ref() else {
        return None;
    };
    Some((graph_alias, *aggregate_offset, *field_offset))
}

fn addressed_aggregate(expression: &Expression) -> Option<(&str, u32)> {
    let Expression::AddressOf { operand } = expression else {
        return None;
    };
    let Expression::Member { base, offset, .. } = operand.as_ref() else {
        return None;
    };
    let Expression::Variable(graph_alias) = base.as_ref() else {
        return None;
    };
    Some((graph_alias, *offset))
}

fn poststep_assignment(statement: &Statement) -> Option<(&str, &str)> {
    let (target, value) = assignment(statement)?;
    let Expression::Cast { operand, .. } = value else {
        return None;
    };
    let Expression::PostStep {
        target: source,
        operator: BinaryOperator::Add,
    } = operand.as_ref()
    else {
        return None;
    };
    let Expression::Variable(source) = source.as_ref() else {
        return None;
    };
    Some((target, source))
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(candidate) if candidate == name)
}

fn unsigned_constant(expression: &Expression) -> Option<u32> {
    match expression {
        Expression::IntegerLiteral(value) => Some(*value as u32),
        Expression::Cast { operand, .. } => unsigned_constant(operand),
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let left = unsigned_constant(left)?;
            let right = unsigned_constant(right)?;
            match operator {
                BinaryOperator::BitOr => Some(left | right),
                BinaryOperator::BitAnd => Some(left & right),
                BinaryOperator::ShiftLeft => Some(left.wrapping_shl(right)),
                BinaryOperator::ShiftRight => Some(left.wrapping_shr(right)),
                BinaryOperator::Add => Some(left.wrapping_add(right)),
                BinaryOperator::Subtract => Some(left.wrapping_sub(right)),
                _ => None,
            }
        }
        _ => None,
    }
}
