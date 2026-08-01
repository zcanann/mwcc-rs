//! Recursive validation against a fixed memory-map table.
//!
//! The validator finds an overlapping map entry, checks byte-sized access
//! permissions, and recursively validates portions before and after the entry.
//! Legacy optimized MWCC collapses the fixed-count loop while retaining two
//! frontend branch nodes and uses `r5` as both result state and recursive arg3.

#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
struct MemoryMapValidation {
    map: String,
    stride: u32,
    start_offset: i16,
    end_offset: i16,
    readable_offset: i16,
    writeable_offset: i16,
    readable_option: u16,
    writeable_option: u16,
    entry_count: i64,
    invalid_error: i64,
    recursive: String,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn strip_casts(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

fn indexed_member<'a>(expression: &'a Expression, index: &str) -> Option<(&'a str, u32, u32)> {
    let Expression::Member {
        base,
        offset,
        index_stride: Some(stride),
        ..
    } = strip_casts(expression)
    else {
        return None;
    };
    let Expression::Index { base, index: actual_index } = base.as_ref() else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    variable(actual_index, index).then_some((global, *offset, *stride))
}

fn direct_call(expression: &Expression) -> Option<(&str, &[Expression])> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    Some((name, arguments))
}

fn comparison_constant(
    expression: &Expression,
    name: &str,
    operator: BinaryOperator,
) -> Option<i64> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    (*actual == operator && variable(strip_casts(left), name)).then(|| constant_value(right))?
}

fn recursive_assignment<'a>(
    statement: &'a Statement,
    result: &str,
) -> Option<(&'a str, &'a [Expression])> {
    let Statement::Assign { name, value } = statement else {
        return None;
    };
    (name == result).then(|| direct_call(value))?
}

fn recognize(function: &Function) -> Option<MemoryMapValidation> {
    let [address, length, option] = function.parameters.as_slice() else {
        return None;
    };
    if function.return_type != Type::Int
        || address.parameter_type != Type::Pointer(Pointee::Int)
        || length.parameter_type != Type::UnsignedInt
        || option.parameter_type != Type::Int
        || !function.guards.is_empty()
        || function.locals.len() != 4
    {
        return None;
    }
    let error = function.locals.iter().find(|local| {
        local.declared_type == Type::Int
            && local.initializer.is_some()
            && !local.is_static
    })?;
    let invalid_error = constant_value(error.initializer.as_ref()?)?;
    let start = function.locals.iter().find(|local| {
        local.declared_type == Type::Pointer(Pointee::UnsignedChar)
            && local.initializer.is_none()
            && local.is_const
            && !local.is_static
    })?;
    let end = function.locals.iter().find(|local| {
        local.name != start.name
            && local.declared_type == Type::Pointer(Pointee::UnsignedChar)
            && local.initializer.is_none()
            && local.is_const
            && !local.is_static
    })?;
    let index = function.locals.iter().find(|local| {
        local.name != error.name
            && local.declared_type == Type::Int
            && local.initializer.is_none()
            && !local.is_static
    })?;

    let [start_assignment, end_assignment, overflow_guard, map_loop] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(start_assignment,
        Statement::Assign {
            name,
            value: Expression::Cast { target_type: Type::Pointer(Pointee::UnsignedChar), operand },
        } if name == &start.name && variable(operand, &address.name))
    {
        return None;
    }
    let Statement::Assign {
        name: assigned_end,
        value:
            Expression::Binary {
                operator: BinaryOperator::Add,
                left: end_base,
                right: length_minus_one,
            },
    } = end_assignment
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: end_length,
        right: one,
    } = length_minus_one.as_ref()
    else {
        return None;
    };
    if assigned_end != &end.name
        || !variable(strip_casts(end_base), &address.name)
        || !variable(end_length, &length.name)
        || constant_value(one) != Some(1)
    {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: overflow_end,
                right: overflow_start,
            },
        then_body: overflow_body,
        else_body: overflow_else,
    } = overflow_guard
    else {
        return None;
    };
    if !overflow_else.is_empty()
        || !variable(overflow_end, &end.name)
        || !variable(overflow_start, &start.name)
        || !matches!(overflow_body.as_slice(), [Statement::Return(Some(value))]
            if constant_value(value) == Some(invalid_error))
    {
        return None;
    }

    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(Expression::Assign { target: initialized_index, value: initial_index }),
        condition: Some(loop_condition),
        step: Some(Expression::Assign { target: stepped_index, value: step_value }),
        body: loop_body,
    } = map_loop
    else {
        return None;
    };
    let entry_count = comparison_constant(loop_condition, &index.name, BinaryOperator::Less)?;
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: step_index,
        right: step_one,
    } = step_value.as_ref()
    else {
        return None;
    };
    if !variable(initialized_index, &index.name)
        || constant_value(initial_index) != Some(0)
        || !variable(stepped_index, &index.name)
        || !variable(step_index, &index.name)
        || constant_value(step_one) != Some(1)
    {
        return None;
    }
    let [overlap_guard] = loop_body.as_slice() else {
        return None;
    };
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: upper_overlap,
                right: lower_overlap,
            },
        then_body: overlap_body,
        else_body: overlap_else,
    } = overlap_guard
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LessEqual,
        left: overlap_start,
        right: map_end_expression,
    } = upper_overlap.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: overlap_end,
        right: map_start_expression,
    } = lower_overlap.as_ref()
    else {
        return None;
    };
    let (map, end_offset, stride) = indexed_member(map_end_expression, &index.name)?;
    let (start_map, start_offset, start_stride) = indexed_member(map_start_expression, &index.name)?;
    if !overlap_else.is_empty()
        || !variable(overlap_start, &start.name)
        || !variable(overlap_end, &end.name)
        || start_map != map
        || start_stride != stride
    {
        return None;
    }
    let [permission_guard, Statement::Break] = overlap_body.as_slice() else {
        return None;
    };

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left: readable_guard,
                right: writeable_guard,
            },
        then_body: denied_body,
        else_body: allowed_body,
    } = permission_guard
    else {
        return None;
    };
    let [Statement::Assign { name: denied_result, value: denied_value }] = denied_body.as_slice()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: readable_option_test,
        right: readable_member_test,
    } = readable_guard.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: writeable_option_test,
        right: writeable_member_test,
    } = writeable_guard.as_ref()
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand: readable_member,
    } = readable_member_test.as_ref()
    else {
        return None;
    };
    let Expression::Unary {
        operator: UnaryOperator::LogicalNot,
        operand: writeable_member,
    } = writeable_member_test.as_ref()
    else {
        return None;
    };
    let (read_map, readable_offset, read_stride) = indexed_member(readable_member, &index.name)?;
    let (write_map, writeable_offset, write_stride) = indexed_member(writeable_member, &index.name)?;
    let readable_option = u16::try_from(comparison_constant(
        readable_option_test,
        &option.name,
        BinaryOperator::Equal,
    )?)
    .ok()?;
    let writeable_option = u16::try_from(comparison_constant(
        writeable_option_test,
        &option.name,
        BinaryOperator::Equal,
    )?)
    .ok()?;
    if denied_result != &error.name
        || constant_value(denied_value) != Some(invalid_error)
        || read_map != map
        || write_map != map
        || read_stride != stride
        || write_stride != stride
    {
        return None;
    }

    let [clear_error, before_guard, after_guard] = allowed_body.as_slice() else {
        return None;
    };
    if !matches!(clear_error,
        Statement::Assign { name, value }
            if name == &error.name && constant_value(value) == Some(0))
    {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: before_start,
                right: before_map_start,
            },
        then_body: before_body,
        else_body: before_else,
    } = before_guard
    else {
        return None;
    };
    let [before_call] = before_body.as_slice() else {
        return None;
    };
    let (recursive, before_arguments) = recursive_assignment(before_call, &error.name)?;
    if !before_else.is_empty()
        || !variable(before_start, &start.name)
        || indexed_member(before_map_start, &index.name)? != (map, start_offset, stride)
        || !matches!(before_arguments, [call_start, _, call_option]
            if variable(call_start, &start.name) && variable(call_option, &option.name))
    {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: prior_success,
                right: after_range,
            },
        then_body: after_body,
        else_body: after_else,
    } = after_guard
    else {
        return None;
    };
    let [after_call] = after_body.as_slice() else {
        return None;
    };
    let (after_recursive, after_arguments) = recursive_assignment(after_call, &error.name)?;
    if !after_else.is_empty()
        || comparison_constant(prior_success, &error.name, BinaryOperator::Equal) != Some(0)
        || !matches!(after_range.as_ref(), Expression::Binary { operator: BinaryOperator::Greater, left, right }
            if variable(left, &end.name)
                && indexed_member(right, &index.name) == Some((map, end_offset, stride)))
        || after_recursive != recursive
        || !matches!(after_arguments, [call_start, _, call_option]
            if indexed_member(call_start, &index.name) == Some((map, end_offset, stride))
                && variable(call_option, &option.name))
        || !matches!(function.return_expression.as_ref(),
            Some(Expression::Variable(returned)) if returned == &error.name)
    {
        return None;
    }

    Some(MemoryMapValidation {
        map: map.into(),
        stride,
        start_offset: i16::try_from(start_offset).ok()?,
        end_offset: i16::try_from(end_offset).ok()?,
        readable_offset: i16::try_from(readable_offset).ok()?,
        writeable_offset: i16::try_from(writeable_offset).ok()?,
        readable_option,
        writeable_option,
        entry_count,
        invalid_error,
        recursive: recursive.into(),
    })
}

impl Generator {
    pub(crate) fn try_memory_map_validation(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.optimization != mwcc_versions::Optimization::O4
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        let Some(validation) = recognize(function) else {
            return Ok(false);
        };
        if validation.entry_count != 1
            || validation.stride != 16
            || !self.globals.contains_key(&validation.map)
        {
            return Ok(false);
        }
        self.emit_memory_map_validation(validation);
        Ok(true)
    }

    fn emit_memory_map_validation(&mut self, validation: MemoryMapValidation) {
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![31, 30, 29];
        let body = self.fresh_label();
        let init_second = self.fresh_label();
        let loop_body = self.fresh_label();
        let writeable_check = self.fresh_label();
        let allowed = self.fresh_label();
        let denied = self.fresh_label();
        let after_before = self.fresh_label();
        let done = self.fresh_label();
        let epilogue = self.fresh_label();
        let emit_recursive = |generator: &mut Self| {
            generator.record_relocation(RelocationKind::Rel24, &validation.recursive);
            generator.output.instructions.push(Instruction::BranchAndLink {
                target: validation.recursive.clone(),
            });
        };

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreWord { s: 31, a: 1, offset: 20 },
            Instruction::StoreWord { s: 30, a: 1, offset: 16 },
            Instruction::StoreWord { s: 29, a: 1, offset: 12 },
            Instruction::move_register(30, 5),
            Instruction::Add { d: 31, a: 4, b: 3 },
            Instruction::AddImmediate { d: 31, a: 31, immediate: -1 },
            Instruction::CompareLogicalWord { a: 31, b: 3 },
        ]);
        self.load_integer_constant(5, validation.invalid_error);
        self.emit_branch_conditional_to(4, 0, body);
        self.load_integer_constant(3, validation.invalid_error);
        self.emit_branch_to(epilogue);

        self.bind_label(body);
        self.record_relocation(RelocationKind::Addr16Ha, &validation.map);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &validation.map);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::load_immediate(6, 0),
        ]);
        self.emit_branch_to(init_second);
        self.bind_label(init_second);
        self.emit_branch_to(loop_body);
        self.bind_label(loop_body);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 4, offset: validation.end_offset },
            Instruction::CompareLogicalWord { a: 3, b: 0 },
        ]);
        self.emit_branch_conditional_to(12, 1, done);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 4, offset: validation.start_offset },
            Instruction::CompareLogicalWord { a: 31, b: 0 },
        ]);
        self.emit_branch_conditional_to(12, 0, done);
        self.output.instructions.extend([
            Instruction::ClearLeftImmediate { a: 0, s: 30, clear: 24 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: validation.readable_option },
        ]);
        self.emit_branch_conditional_to(4, 2, writeable_check);
        self.output.instructions.extend([
            Instruction::ShiftLeftImmediate { a: 0, s: 6, shift: 4 },
            Instruction::Add { d: 4, a: 4, b: 0 },
            Instruction::LoadWord { d: 0, a: 4, offset: validation.readable_offset },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(12, 2, denied);

        self.bind_label(writeable_check);
        self.output.instructions.extend([
            Instruction::ClearLeftImmediate { a: 0, s: 30, clear: 24 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: validation.writeable_option },
        ]);
        self.emit_branch_conditional_to(4, 2, allowed);
        self.record_relocation(RelocationKind::Addr16Ha, &validation.map);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &validation.map);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::ShiftLeftImmediate { a: 0, s: 6, shift: 4 },
            Instruction::Add { d: 4, a: 4, b: 0 },
            Instruction::LoadWord { d: 0, a: 4, offset: validation.writeable_offset },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, allowed);
        self.bind_label(denied);
        self.load_integer_constant(5, validation.invalid_error);
        self.emit_branch_to(done);

        self.bind_label(allowed);
        self.record_relocation(RelocationKind::Addr16Ha, &validation.map);
        self.output.instructions.extend([
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::ShiftLeftImmediate { a: 29, s: 6, shift: 4 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &validation.map);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 0, a: 4, immediate: 0 },
            Instruction::Add { d: 4, a: 0, b: 29 },
            Instruction::LoadWord { d: 0, a: 4, offset: validation.start_offset },
            Instruction::load_immediate(5, 0),
            Instruction::CompareLogicalWord { a: 3, b: 0 },
        ]);
        self.emit_branch_conditional_to(4, 0, after_before);
        self.output.instructions.extend([
            Instruction::move_register(5, 30),
            Instruction::SubtractFrom { d: 4, a: 3, b: 0 },
        ]);
        emit_recursive(self);
        self.output.instructions.push(Instruction::move_register(5, 3));

        self.bind_label(after_before);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 5, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, done);
        self.record_relocation(RelocationKind::Addr16Ha, &validation.map);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 });
        self.record_relocation(RelocationKind::Addr16Lo, &validation.map);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::Add { d: 3, a: 0, b: 29 },
            Instruction::LoadWord { d: 3, a: 3, offset: validation.end_offset },
            Instruction::CompareLogicalWord { a: 31, b: 3 },
        ]);
        self.emit_branch_conditional_to(4, 1, done);
        self.output.instructions.extend([
            Instruction::move_register(5, 30),
            Instruction::SubtractFrom { d: 4, a: 3, b: 31 },
        ]);
        emit_recursive(self);
        self.output.instructions.push(Instruction::move_register(5, 3));
        self.emit_branch_to(done);

        self.bind_label(done);
        self.output.instructions.push(Instruction::move_register(3, 5));
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 31, a: 1, offset: 20 },
            Instruction::LoadWord { d: 30, a: 1, offset: 16 },
            Instruction::LoadWord { d: 29, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        self.output.anonymous_label_bump += 47;
    }
}
