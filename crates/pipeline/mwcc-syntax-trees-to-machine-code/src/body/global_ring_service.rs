//! Service live entries in a fixed-capacity global pointer ring.
//!
//! The retained BSS anchor is page-adjusted once, and every ring access uses a
//! low-half section displacement from the same computed slot address.  Owning
//! the transaction keeps that high/low address contract explicit and prevents
//! independent expression lowering from rematerializing the ring bases.

#[allow(unused_imports)]
use super::*;

struct GlobalRingService<'a> {
    count: &'a str,
    top: &'a str,
    pointer_array: &'a str,
    age_array: &'a str,
    entry: &'a str,
    countdown_offset: i16,
    callback_offset: i16,
    callback_reason: i16,
    mask: u32,
}

impl Generator {
    pub(crate) fn try_global_ring_service(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        let ring_bytes = (plan.mask + 1) * 4;
        let Some(anchor_symbol) = self.data_section_anchor.as_ref().and_then(|anchor| {
            (anchor.anchor_symbol == "...bss.0"
                && anchor.symbols.contains(plan.pointer_array)
                && anchor.symbols.contains(plan.age_array))
            .then(|| anchor.anchor_symbol.clone())
        }) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || !self.behavior.schedule_latency_slots
            || plan.mask != 31
            || self.global_array_sizes.get(plan.pointer_array).copied() != Some(ring_bytes)
            || self.global_array_sizes.get(plan.age_array).copied() != Some(ring_bytes)
            || !matches!(self.globals.get(plan.count), Some(Type::UnsignedInt))
            || !matches!(self.globals.get(plan.top), Some(Type::UnsignedInt))
        {
            return Ok(false);
        }

        if let Some(anchor) = self.data_section_anchor.as_mut() {
            anchor.register = Some(30);
        }
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![31, 30];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.emit_address_high(4, &anchor_symbol);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::load_immediate(3, 0),
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -24 },
            Instruction::StoreMultipleWord { s: 30, a: 1, offset: 16 },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, &anchor_symbol);
        self.output.instructions.push(Instruction::AddImmediate { d: 30, a: 4, immediate: 0 });
        self.record_relocation(RelocationKind::Rel24, plan.entry);
        self.output.instructions.extend([
            Instruction::BranchAndLink { target: plan.entry.to_owned() },
            Instruction::AddImmediateShifted { d: 30, a: 30, immediate: 1 },
            Instruction::load_immediate(31, 0),
            Instruction::Branch { target: 41 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.top);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 0, offset: 0 },
            Instruction::Add { d: 0, a: 0, b: 31 },
            Instruction::RotateAndMask { a: 0, s: 0, shift: 2, begin: 25, end: 29 },
            Instruction::Add { d: 5, a: 30, b: 0 },
        ]);
        self.record_data_section_symbol_displacement(plan.pointer_array);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 5, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::move_register(3, 0),
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 40,
            },
        ]);
        self.record_data_section_symbol_displacement(plan.age_array);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 4, a: 5, offset: 0 },
            Instruction::AddImmediate { d: 0, a: 4, immediate: 1 },
        ]);
        self.record_data_section_symbol_displacement(plan.age_array);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 0, a: 5, offset: 0 },
            Instruction::LoadWord { d: 4, a: 3, offset: plan.countdown_offset },
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 27,
            },
            Instruction::AddImmediate { d: 0, a: 4, immediate: -1 },
            Instruction::StoreWord { s: 0, a: 3, offset: plan.countdown_offset },
            Instruction::LoadWord { d: 0, a: 3, offset: plan.countdown_offset },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 40,
            },
            Instruction::LoadWord { d: 12, a: 3, offset: plan.callback_offset },
            Instruction::load_immediate(4, plan.callback_reason),
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.top);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 0, offset: 0 },
            Instruction::load_immediate(4, 0),
            Instruction::Add { d: 0, a: 0, b: 31 },
            Instruction::RotateAndMask { a: 0, s: 0, shift: 2, begin: 25, end: 29 },
            Instruction::Add { d: 3, a: 30, b: 0 },
        ]);
        self.record_data_section_symbol_displacement(plan.pointer_array);
        self.output.instructions.extend([
            Instruction::StoreWord { s: 4, a: 3, offset: 0 },
            Instruction::AddImmediate { d: 31, a: 31, immediate: 1 },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, plan.count);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 0, a: 0, offset: 0 },
            Instruction::CompareLogicalWord { a: 31, b: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 11 },
            Instruction::LoadWord { d: 0, a: 1, offset: 28 },
            Instruction::LoadMultipleWord { d: 30, a: 1, offset: 16 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<GlobalRingService<'_>> {
    let [index_local, object_local, _padding] = function.locals.as_slice() else {
        return None;
    };
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || index_local.declared_type != Type::UnsignedInt
        || !matches!(object_local.declared_type, Type::Pointer(_) | Type::StructPointer { .. })
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { name: entry, arguments: entry_arguments }), loop_statement, trailing] =
        function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(entry_arguments.as_slice(), [value] if constant_value(value) == Some(0))
        || !matches!(trailing, Statement::Loop { kind: LoopKind::DoWhile, condition: Some(value), body, .. }
            if constant_value(value) == Some(0) && body.is_empty())
    {
        return None;
    }
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = loop_statement
    else {
        return None;
    };
    let index = index_local.name.as_str();
    if !assigns_constant(initializer, index, 0) || !increments_by_one(step, index) {
        return None;
    }
    let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition else {
        return None;
    };
    if variable(left) != Some(index) {
        return None;
    }
    let count = variable(right)?;
    let [Statement::Assign { name: object, value: pointer_slot }, Statement::If {
        condition: live_object,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if object != &object_local.name || variable(live_object) != Some(object) || !else_body.is_empty() {
        return None;
    }
    let (pointer_array, top, mask) = ring_slot(pointer_slot, index)?;
    let [age_update, decrement_guard, callback_guard] = then_body.as_slice() else {
        return None;
    };
    let (age_array, age_top, age_mask) = incremented_ring_slot(age_update, index)?;
    if top != age_top || mask != age_mask {
        return None;
    }
    let (countdown_offset, countdown_object) = positive_member_guard(decrement_guard)?;
    if countdown_object != object || !decrements_member(decrement_guard, object, countdown_offset) {
        return None;
    }
    let (callback_offset, callback_reason) = callback_and_clear(
        callback_guard,
        object,
        pointer_array,
        top,
        mask,
        index,
        countdown_offset,
    )?;
    Some(GlobalRingService {
        count,
        top,
        pointer_array,
        age_array,
        entry,
        countdown_offset,
        callback_offset,
        callback_reason,
        mask,
    })
}

fn ring_slot<'a>(expression: &'a Expression, index: &str) -> Option<(&'a str, &'a str, u32)> {
    let Expression::Index { base, index: ring_index } = expression else { return None };
    let array = variable(base)?;
    let Expression::Binary { operator: BinaryOperator::BitAnd, left: sum, right: mask } = ring_index.as_ref() else {
        return None;
    };
    let Expression::Binary { operator: BinaryOperator::Add, left: top, right: summed_index } = sum.as_ref() else {
        return None;
    };
    (variable(summed_index) == Some(index)).then_some((
        array,
        variable(top)?,
        u32::try_from(constant_value(mask)?).ok()?,
    ))
}

fn incremented_ring_slot<'a>(statement: &'a Statement, index: &str) -> Option<(&'a str, &'a str, u32)> {
    let Statement::Store { target, value: Expression::IndexedUpdateValue { value } } = statement else {
        return None;
    };
    let slot = ring_slot(target, index)?;
    let Expression::Binary { operator: BinaryOperator::Add, left, right } = value.as_ref() else {
        return None;
    };
    (structurally_equal(left, target) && constant_value(right) == Some(1)).then_some(slot)
}

fn positive_member_guard(statement: &Statement) -> Option<(i16, &str)> {
    let Statement::If { condition: Expression::Binary { operator: BinaryOperator::Greater, left, right }, else_body, .. } = statement else {
        return None;
    };
    if !else_body.is_empty() || constant_value(right) != Some(0) {
        return None;
    }
    let Expression::Member { base, offset, .. } = left.as_ref() else { return None };
    Some((i16::try_from(*offset).ok()?, variable(base)?))
}

fn decrements_member(statement: &Statement, object: &str, offset: i16) -> bool {
    let Statement::If { then_body, .. } = statement else { return false };
    let [Statement::Store { target, value: Expression::IndexedUpdateValue { value } }] = then_body.as_slice() else {
        return false;
    };
    let Expression::Member { base, offset: target_offset, .. } = target else { return false };
    let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = value.as_ref() else {
        return false;
    };
    variable(base) == Some(object)
        && i16::try_from(*target_offset).ok() == Some(offset)
        && structurally_equal(left, target)
        && constant_value(right) == Some(1)
}

fn callback_and_clear(
    statement: &Statement,
    object: &str,
    pointer_array: &str,
    top: &str,
    mask: u32,
    index: &str,
    countdown_offset: i16,
) -> Option<(i16, i16)> {
    let Statement::If { condition: Expression::Binary { operator: BinaryOperator::Equal, left, right }, then_body, else_body } = statement else {
        return None;
    };
    if !else_body.is_empty() || constant_value(right) != Some(0) {
        return None;
    }
    let Expression::Member { base, offset, .. } = left.as_ref() else { return None };
    if variable(base) != Some(object) || i16::try_from(*offset).ok() != Some(countdown_offset) {
        return None;
    }
    let [Statement::Expression(Expression::CallThrough { target, arguments }), Statement::Store { target: cleared, value }] = then_body.as_slice() else {
        return None;
    };
    let Expression::Member { base, offset: callback_offset, .. } = target.as_ref() else { return None };
    let [receiver, reason] = arguments.as_slice() else { return None };
    let (cleared_array, cleared_top, cleared_mask) = ring_slot(cleared, index)?;
    if variable(base) != Some(object)
        || variable(receiver) != Some(object)
        || cleared_array != pointer_array
        || cleared_top != top
        || cleared_mask != mask
        || constant_value(value) != Some(0)
    {
        return None;
    }
    Some((
        i16::try_from(*callback_offset).ok()?,
        i16::try_from(constant_value(reason)?).ok()?,
    ))
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable(operand),
        _ => None,
    }
}

fn assigns_constant(expression: &Expression, name: &str, expected: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(expected))
}

fn increments_by_one(expression: &Expression, name: &str) -> bool {
    let Expression::Assign { target, value } = expression else { return false };
    matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
        if variable(target) == Some(name)
            && variable(left) == Some(name)
            && constant_value(right) == Some(1))
}
