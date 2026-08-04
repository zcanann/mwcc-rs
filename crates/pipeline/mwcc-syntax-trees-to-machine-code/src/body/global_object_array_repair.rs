//! Repair matching objects while walking a fixed global object array.
//!
//! Build 163 retains the array base and a byte-offset induction variable across
//! the calls in the selected arm. The generic loop keeps only the source index
//! and rematerializes `base + index * stride` on every iteration; this owner
//! exposes the complete strength-reduced lifetime and its dense saved frame.

#[allow(unused_imports)]
use super::*;

struct GlobalObjectArrayRepair<'a> {
    array: &'a str,
    count: u32,
    stride: i16,
    match_offset: i16,
    owner_offset: i16,
    list_offset: i16,
    stop: &'a str,
    cut: &'a str,
    append: &'a str,
}

impl Generator {
    pub(crate) fn try_global_object_array_repair(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || !self.behavior.schedule_latency_slots
            || !self.behavior.use_lmw_stmw
            || self.global_array_sizes.get(plan.array).copied()
                != Some(plan.count * u32::try_from(plan.stride).ok().unwrap_or(0))
            || plan.count > u32::from(u16::MAX)
        {
            return Ok(false);
        }

        const MATCH: u8 = 27;
        const OBJECT: u8 = 28;
        const INDEX: u8 = 29;
        const BASE: u8 = 30;
        const OFFSET: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 40;
        self.callee_saved = vec![OFFSET, BASE, INDEX, OBJECT, MATCH];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(4, plan.array);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -40 },
            Instruction::StoreMultipleWord { s: MATCH, a: 1, offset: 20 },
            Instruction::AddImmediate { d: MATCH, a: 3, immediate: 0 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, plan.array);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: BASE, a: 4, immediate: 0 },
            Instruction::load_immediate(INDEX, 0),
            Instruction::load_immediate(OFFSET, 0),
            Instruction::Add { d: 3, a: BASE, b: OFFSET },
            Instruction::LoadWord { d: 0, a: 3, offset: plan.match_offset },
            Instruction::AddImmediate { d: OBJECT, a: 3, immediate: 0 },
            Instruction::CompareLogicalWord { a: 0, b: MATCH },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 24,
            },
            Instruction::move_register(3, OBJECT),
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.stop);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.stop.to_owned(),
        });
        self.output.instructions.push(Instruction::move_register(3, OBJECT));
        self.record_relocation(RelocationKind::Rel24, plan.cut);
        self.output.instructions.extend([
            Instruction::BranchAndLink { target: plan.cut.to_owned() },
            Instruction::CompareWordImmediate { a: 3, immediate: -1 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 24,
            },
            Instruction::LoadWord { d: 3, a: OBJECT, offset: plan.owner_offset },
            Instruction::AddImmediate { d: 4, a: OBJECT, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: plan.list_offset },
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.append);
        self.output.instructions.extend([
            Instruction::BranchAndLink { target: plan.append.to_owned() },
            Instruction::AddImmediate { d: INDEX, a: INDEX, immediate: 1 },
            Instruction::AddImmediate { d: OFFSET, a: OFFSET, immediate: plan.stride },
            Instruction::CompareLogicalWordImmediate {
                a: INDEX,
                immediate: plan.count as u16,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 9,
            },
            Instruction::LoadMultipleWord { d: MATCH, a: 1, offset: 20 },
            Instruction::LoadWord { d: 0, a: 1, offset: 44 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 40 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<GlobalObjectArrayRepair<'_>> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    let [index_local, object_local] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Void
        || !matches!(parameter.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || index_local.declared_type != Type::UnsignedInt
        || !matches!(object_local.declared_type, Type::StructPointer { .. })
        || index_local.initializer.is_some()
        || object_local.initializer.is_some()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
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
    if !assigns_constant(initializer, &index_local.name, 0)
        || !increments_by_one(step, &index_local.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: compared_index,
        right: count,
    } = condition
    else {
        return None;
    };
    if variable(compared_index) != Some(index_local.name.as_str()) {
        return None;
    }
    let count = u32::try_from(constant_value(count)?).ok()?;

    let [Statement::Assign { name, value: object_address }, Statement::If {
        condition: match_condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if name != &object_local.name || !else_body.is_empty() {
        return None;
    }
    let Expression::AddressOf { operand: array_element } = object_address else {
        return None;
    };
    let (array, element_index) = indexed_variable(array_element)?;
    if variable(element_index) != Some(index_local.name.as_str()) {
        return None;
    }
    let stride = match object_local.declared_type {
        Type::StructPointer { element_size } => i16::try_from(element_size).ok()?,
        _ => return None,
    };

    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: matched_member,
        right: matched_parameter,
    } = match_condition
    else {
        return None;
    };
    let (matched_base, match_offset) = member(matched_member)?;
    if variable(matched_base) != Some(object_local.name.as_str())
        || variable(matched_parameter) != Some(parameter.name.as_str())
    {
        return None;
    }
    let [stop_statement, Statement::If {
        condition: cut_condition,
        then_body: append_body,
        else_body: cut_else,
    }] = then_body.as_slice()
    else {
        return None;
    };
    let (stop, stop_args) = direct_call_statement(stop_statement)?;
    if !single_variable_argument(stop_args, &object_local.name) || !cut_else.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left: cut_call,
        right: failure,
    } = cut_condition
    else {
        return None;
    };
    let Expression::Call { name: cut, arguments: cut_args } = cut_call.as_ref() else {
        return None;
    };
    if !single_variable_argument(cut_args, &object_local.name)
        || constant_value(failure) != Some(-1)
    {
        return None;
    }
    let [append_statement] = append_body.as_slice() else {
        return None;
    };
    let (append, append_args) = direct_call_statement(append_statement)?;
    let [Expression::AddressOf { operand: list }, appended_object] = append_args else {
        return None;
    };
    if variable(appended_object) != Some(object_local.name.as_str()) {
        return None;
    }
    let (manager, list_offset) = member(list)?;
    let (manager_base, owner_offset) = member(manager)?;
    if variable(manager_base) != Some(object_local.name.as_str()) {
        return None;
    }

    Some(GlobalObjectArrayRepair {
        array,
        count,
        stride,
        match_offset: i16::try_from(match_offset).ok()?,
        owner_offset: i16::try_from(owner_offset).ok()?,
        list_offset: i16::try_from(list_offset).ok()?,
        stop,
        cut,
        append,
    })
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn indexed_variable(expression: &Expression) -> Option<(&str, &Expression)> {
    let Expression::Index { base, index } = expression else { return None; };
    Some((variable(base)?, index))
}

fn member(expression: &Expression) -> Option<(&Expression, u32)> {
    let Expression::Member { base, offset, index_stride: None, .. } = expression else {
        return None;
    };
    Some((base, *offset))
}

fn direct_call_statement(statement: &Statement) -> Option<(&str, &[Expression])> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    Some((name, arguments))
}

fn single_variable_argument(arguments: &[Expression], expected: &str) -> bool {
    matches!(arguments, [argument] if variable(argument) == Some(expected))
}

fn assigns_constant(expression: &Expression, name: &str, expected: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(expected))
}

fn increments_by_one(expression: &Expression, name: &str) -> bool {
    let Expression::Assign { target, value } = expression else { return false; };
    matches!(value.as_ref(), Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } if variable(target) == Some(name)
        && variable(left) == Some(name)
        && constant_value(right) == Some(1))
}
