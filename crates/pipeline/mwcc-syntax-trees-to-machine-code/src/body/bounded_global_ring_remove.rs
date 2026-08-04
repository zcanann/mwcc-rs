//! Counted search and removal from a power-of-two global pointer ring.
//!
//! Build 163 turns the loop bound into a CTR trip count, retains the ring base
//! and top index, and fuses `(top + i) & mask` with pointer scaling. Owning the
//! complete transaction also prevents the selected offset from being scaled a
//! second time on the removal path.

#[allow(unused_imports)]
use super::*;

struct BoundedGlobalRingRemove<'a> {
    count: &'a str,
    top: &'a str,
    array: &'a str,
    mask: u32,
}

impl Generator {
    pub(crate) fn try_bounded_global_ring_remove(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || !self.behavior.schedule_latency_slots
            || self.global_array_sizes.get(plan.array).copied()
                != Some((plan.mask + 1) * 4)
            || !matches!(self.globals.get(plan.count), Some(Type::UnsignedInt))
            || !matches!(self.globals.get(plan.top), Some(Type::UnsignedInt))
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.record_relocation(RelocationKind::EmbSda21, plan.count);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 0, offset: 0 });
        self.emit_address_high(4, plan.array);
        self.record_relocation(RelocationKind::Addr16Lo, plan.array);
        self.output.instructions.push(Instruction::AddImmediate { d: 5, a: 4, immediate: 0 });
        self.record_relocation(RelocationKind::EmbSda21, plan.top);
        self.output.instructions.extend([
            Instruction::LoadWord { d: 7, a: 0, offset: 0 },
            Instruction::load_immediate(8, 0),
            Instruction::MoveToCountRegister { s: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 23,
            },
            Instruction::Add { d: 0, a: 7, b: 8 },
            Instruction::RotateAndMask {
                a: 6,
                s: 0,
                shift: 2,
                begin: 25,
                end: 29,
            },
            Instruction::Add { d: 4, a: 5, b: 6 },
            Instruction::LoadWord { d: 0, a: 4, offset: 0 },
            Instruction::CompareLogicalWord { a: 0, b: 3 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 21,
            },
        ]);
        self.emit_address_high(3, plan.array);
        self.output.instructions.push(Instruction::load_immediate(5, 0));
        self.record_relocation(RelocationKind::Addr16Lo, plan.array);
        self.output.instructions.extend([
            Instruction::AddImmediate { d: 0, a: 3, immediate: 0 },
            Instruction::load_immediate(3, 1),
            Instruction::Add { d: 4, a: 0, b: 6 },
            Instruction::StoreWord { s: 5, a: 4, offset: 0 },
            Instruction::BranchToLinkRegister,
            Instruction::AddImmediate { d: 8, a: 8, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 8,
            },
            Instruction::load_immediate(3, 0),
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

fn classify(function: &Function) -> Option<BoundedGlobalRingRemove<'_>> {
    let [parameter] = function.parameters.as_slice() else {
        return None;
    };
    let [index_local, offset_local] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !matches!(parameter.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
        || index_local.declared_type != Type::UnsignedInt
        || offset_local.declared_type != Type::UnsignedInt
        || index_local.initializer.is_some()
        || offset_local.initializer.is_some()
        || !function.guards.is_empty()
        || constant_value(function.return_expression.as_ref()?) != Some(0)
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
    let index = index_local.name.as_str();
    let offset = offset_local.name.as_str();
    if !assigns_constant(initializer, index, 0)
        || !increments_by_one(step, index)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: condition_index,
        right: count,
    } = condition
    else {
        return None;
    };
    if variable(condition_index) != Some(index) {
        return None;
    }
    let count = variable(count)?;

    let [Statement::Assign { name, value: ring_offset }, Statement::If {
        condition: match_condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if name != offset || !else_body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: sum,
        right: mask,
    } = ring_offset
    else {
        return None;
    };
    let mask = u32::try_from(constant_value(mask)?).ok()?;
    if mask != 31 {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: top,
        right: summed_index,
    } = sum.as_ref()
    else {
        return None;
    };
    if variable(summed_index) != Some(index) {
        return None;
    }
    let top = variable(top)?;

    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left: element,
        right: compared_object,
    } = match_condition
    else {
        return None;
    };
    if variable(compared_object) != Some(parameter.name.as_str()) {
        return None;
    }
    let (array, element_index) = indexed_variable(element)?;
    if variable(element_index) != Some(offset) {
        return None;
    }
    let [Statement::Store { target, value }, Statement::Return(Some(return_value))] =
        then_body.as_slice()
    else {
        return None;
    };
    let (stored_array, stored_index) = indexed_variable(target)?;
    if stored_array != array
        || variable(stored_index) != Some(offset)
        || constant_value(value) != Some(0)
        || constant_value(return_value) != Some(1)
    {
        return None;
    }

    Some(BoundedGlobalRingRemove {
        count,
        top,
        array,
        mask,
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
    let Expression::Index { base, index } = expression else {
        return None;
    };
    Some((variable(base)?, index))
}

fn assigns_constant(expression: &Expression, name: &str, expected: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if variable(target) == Some(name) && constant_value(value) == Some(expected))
}

fn increments_by_one(expression: &Expression, name: &str) -> bool {
    let Expression::Assign { target, value } = expression else {
        return false;
    };
    matches!(value.as_ref(), Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } if variable(target) == Some(name)
        && variable(left) == Some(name)
        && constant_value(right) == Some(1))
}
