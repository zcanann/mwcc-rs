//! Guarded append to a fixed-capacity global pointer ring.
//!
//! Build 163 keeps the accepted object in one saved home, forms the shared
//! ring offset once, and advances the tail/count pair from one load each.  The
//! complete transaction owns those cross-statement identities; independent
//! array and scalar stores cannot recover them after expression lowering.

#[allow(unused_imports)]
use super::*;

struct BoundedGlobalRingEnqueue<'a> {
    count: &'a str,
    tail: &'a str,
    pointer_array: &'a str,
    age_array: &'a str,
    admission: &'a str,
    member_offset: i16,
    capacity: u16,
}

impl Generator {
    pub(crate) fn try_bounded_global_ring_enqueue(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        let ring_bytes = u32::from(plan.capacity) * 4;
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || !self.behavior.schedule_latency_slots
            || self.global_array_sizes.get(plan.pointer_array).copied() != Some(ring_bytes)
            || self.global_array_sizes.get(plan.age_array).copied() != Some(ring_bytes)
            || !matches!(self.globals.get(plan.count), Some(Type::UnsignedInt))
            || !matches!(self.globals.get(plan.tail), Some(Type::UnsignedInt))
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![31];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
            Instruction::StoreWord { s: 31, a: 1, offset: 28 },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.count);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 0, offset: 0 },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: plan.capacity,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 10,
            },
            Instruction::load_immediate(3, 0),
            Instruction::Branch { target: 39 },
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: plan.member_offset,
            },
            Instruction::ClearLeftImmediate { a: 3, s: 0, clear: 24 },
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.admission);
        self.output.instructions.extend([
            Instruction::BranchAndLink { target: plan.admission.to_owned() },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 17,
            },
            Instruction::load_immediate(3, 0),
            Instruction::Branch { target: 39 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.tail);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
        self.emit_address_high(4, plan.age_array);
        self.emit_address_high(3, plan.pointer_array);
        self.record_relocation(RelocationKind::Addr16Lo, plan.age_array);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::ShiftLeftImmediate { a: 6, s: 0, shift: 2 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, plan.pointer_array);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::Add { d: 3, a: 4, b: 6 },
            Instruction::load_immediate(5, 0),
            Instruction::StoreWord { s: 5, a: 3, offset: 0 },
            Instruction::Add { d: 3, a: 0, b: 6 },
            Instruction::StoreWord { s: 31, a: 3, offset: 0 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.tail);
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 0, offset: 0 });
        self.record_relocation(RelocationKind::EmbSda21, plan.count);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 3, a: 0, offset: 0 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 1 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.tail);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 0, a: 0, offset: 0 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 1 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.tail);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
        self.record_relocation(RelocationKind::EmbSda21, plan.count);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 3, a: 0, offset: 0 },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: plan.capacity,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 38,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.tail);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 5, a: 0, offset: 0 },
            Instruction::load_immediate(3, 1),
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::LoadWord { d: 31, a: 1, offset: 28 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<BoundedGlobalRingEnqueue<'_>> {
    let [parameter] = function.parameters.as_slice() else { return None };
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !matches!(parameter.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [full, admitted, clear_age, store_pointer, advance_tail, advance_count, wrap_tail, success, trailing] =
        function.statements.as_slice()
    else {
        return None;
    };
    let (count, capacity) = equality_variable_constant(return_zero_guard(full)?)?;
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: call,
        right: admission_zero,
    } = return_zero_guard(admitted)?
    else {
        return None;
    };
    if constant_value(admission_zero) != Some(0) {
        return None;
    }
    let Expression::Call { name: admission, arguments } = call.as_ref() else {
        return None;
    };
    let [Expression::Member { base, offset, .. }] = arguments.as_slice() else {
        return None;
    };
    if variable(base) != Some(parameter.name.as_str()) {
        return None;
    }
    let member_offset = i16::try_from(*offset).ok()?;
    let (age_array, tail) = indexed_store(clear_age, Some(0))?;
    let (pointer_array, pointer_tail, pointer_value) = indexed_store_value(store_pointer)?;
    if tail != pointer_tail || variable(pointer_value) != Some(parameter.name.as_str()) {
        return None;
    }
    if incremented_global(advance_tail)? != tail || incremented_global(advance_count)? != count {
        return None;
    }
    let Statement::If { condition, then_body, else_body } = wrap_tail else {
        return None;
    };
    if !else_body.is_empty()
        || equality_variable_constant(condition)? != (tail, capacity)
        || !matches!(then_body.as_slice(), [statement] if stores_constant(statement, tail, 0))
        || !matches!(success, Statement::Return(Some(value)) if constant_value(value) == Some(1))
        || !matches!(trailing, Statement::Loop { kind: LoopKind::DoWhile, condition: Some(value), body, .. }
            if constant_value(value) == Some(0) && body.is_empty())
    {
        return None;
    }
    let capacity = u16::try_from(capacity).ok()?;
    Some(BoundedGlobalRingEnqueue {
        count,
        tail,
        pointer_array,
        age_array,
        admission,
        member_offset,
        capacity,
    })
}

fn return_zero_guard(statement: &Statement) -> Option<&Expression> {
    let Statement::If { condition, then_body, else_body } = statement else { return None };
    (!then_body.is_empty() && else_body.is_empty()
        && matches!(then_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(0)))
    .then_some(condition)
}

fn equality_variable_constant(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Binary { operator: BinaryOperator::Equal, left, right } = expression else {
        return None;
    };
    Some((variable(left)?, constant_value(right)?))
}

fn indexed_store(statement: &Statement, expected: Option<i64>) -> Option<(&str, &str)> {
    let (array, index, value) = indexed_store_value(statement)?;
    expected.is_none_or(|expected| constant_value(value) == Some(expected))
        .then_some((array, index))
}

fn indexed_store_value(statement: &Statement) -> Option<(&str, &str, &Expression)> {
    let Statement::Store { target, value } = statement else { return None };
    let (array, index) = indexed_variable(target)?;
    Some((array, variable(index)?, value))
}

fn incremented_global(statement: &Statement) -> Option<&str> {
    let Statement::Store { target, value } = statement else { return None };
    let target = variable(target)?;
    let Expression::Binary { operator: BinaryOperator::Add, left, right } = value else {
        return None;
    };
    (variable(left) == Some(target) && constant_value(right) == Some(1)).then_some(target)
}

fn stores_constant(statement: &Statement, expected_target: &str, expected_value: i64) -> bool {
    matches!(statement, Statement::Store { target, value }
        if variable(target) == Some(expected_target) && constant_value(value) == Some(expected_value))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn indexed_variable(expression: &Expression) -> Option<(&str, &Expression)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    Some((variable(base)?, index))
}
