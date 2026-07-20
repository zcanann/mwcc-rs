//! Virtual collection scans with an early predicate return.
//!
//! This family occurs in small C++ container facades: an index and `this` survive
//! virtual count/lookup calls, the looked-up pointer is null-checked, and a direct
//! predicate supplies an early boolean return. It owns the complete loop schedule;
//! the generic statement emitter deliberately does not guess at cross-call liveness.

#[allow(unused_imports)]
use super::*;

struct VirtualCollectionScan<'a> {
    vptr_offset: i16,
    count_slot: i16,
    lookup_slot: i16,
    predicate: &'a str,
}

impl Generator {
    pub(crate) fn try_virtual_collection_scan(&mut self, function: &Function) -> Compilation<bool> {
        let Some(scan) = recognize(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || self.behavior.frame_convention != FrameConvention::Predecrement
        {
            return Ok(false);
        }

        self.emit_virtual_collection_scan(&scan);
        Ok(true)
    }

    fn emit_virtual_collection_scan(&mut self, scan: &VirtualCollectionScan<'_>) {
        const INDEX: u8 = 31;
        const OBJECT: u8 = 30;

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![INDEX, OBJECT];

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: INDEX,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(INDEX, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: OBJECT,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(OBJECT, 3));

        let body = self.fresh_label();
        let increment = self.fresh_label();
        let condition = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_to(condition);

        self.bind_label(body);
        self.output
            .instructions
            .push(Instruction::move_register(3, OBJECT));
        self.output
            .instructions
            .push(Instruction::move_register(4, INDEX));
        self.emit_virtual_call_from_object(OBJECT, scan.vptr_offset, scan.lookup_slot);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, increment);
        self.record_relocation(RelocationKind::Rel24, scan.predicate);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: scan.predicate.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 3,
                clear: 24,
            });
        self.emit_branch_conditional_to(12, 2, increment);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 1));
        self.emit_branch_to(epilogue);

        self.bind_label(increment);
        self.output.instructions.push(Instruction::AddImmediate {
            d: INDEX,
            a: INDEX,
            immediate: 1,
        });

        self.bind_label(condition);
        self.output
            .instructions
            .push(Instruction::move_register(3, OBJECT));
        self.emit_virtual_call_from_object(OBJECT, scan.vptr_offset, scan.count_slot);
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: INDEX, b: 3 });
        self.emit_branch_conditional_to(12, 0, body);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, 0));

        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: INDEX,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: OBJECT,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }

    fn emit_virtual_call_from_object(&mut self, object: u8, vptr_offset: i16, slot: i16) {
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: object,
            offset: vptr_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 12,
            offset: slot,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
    }
}

fn recognize(function: &Function) -> Option<VirtualCollectionScan<'_>> {
    if function.return_type != Type::UnsignedChar
        || function.return_expression.as_ref().and_then(constant_value) != Some(0)
    {
        return None;
    }
    let [this] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(this.parameter_type, Type::StructPointer { .. }) {
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

    let Expression::Assign { target, value } = initializer else {
        return None;
    };
    let Expression::Variable(index_name) = target.as_ref() else {
        return None;
    };
    if constant_value(value) != Some(0) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !variable(left, index_name) {
        return None;
    }
    let Expression::VirtualCall {
        object: count_object,
        vptr_offset: count_vptr,
        slot_offset: count_slot,
        arguments: count_arguments,
        ..
    } = right.as_ref()
    else {
        return None;
    };
    if !variable(count_object, &this.name) || !count_arguments.is_empty() {
        return None;
    }
    let Expression::Assign {
        target: step_target,
        value: step_value,
    } = step
    else {
        return None;
    };
    if !variable(step_target, index_name)
        || !matches!(step_value.as_ref(), Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if variable(left, index_name) && constant_value(right) == Some(1))
    {
        return None;
    }

    let [Statement::Assign {
        name: item_name,
        value: lookup,
    }, Statement::If {
        condition: predicate_condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    let Expression::VirtualCall {
        object: lookup_object,
        vptr_offset: lookup_vptr,
        slot_offset: lookup_slot,
        arguments: lookup_arguments,
        ..
    } = lookup
    else {
        return None;
    };
    if count_vptr != lookup_vptr
        || !variable(lookup_object, &this.name)
        || !matches!(lookup_arguments.as_slice(), [argument] if variable(argument, index_name))
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: item_test,
        right: predicate_call,
    } = predicate_condition
    else {
        return None;
    };
    let Expression::Call {
        name: predicate,
        arguments: predicate_arguments,
    } = predicate_call.as_ref()
    else {
        return None;
    };
    if !variable(item_test, item_name)
        || !matches!(predicate_arguments.as_slice(), [argument] if variable(argument, item_name))
        || !else_body.is_empty()
        || !matches!(then_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(1))
    {
        return None;
    }
    let index_local = function
        .locals
        .iter()
        .find(|local| local.name == *index_name)?;
    let item_local = function
        .locals
        .iter()
        .find(|local| local.name == *item_name)?;
    if index_local.declared_type != Type::UnsignedInt
        || !matches!(
            item_local.declared_type,
            Type::Pointer(_) | Type::StructPointer { .. }
        )
    {
        return None;
    }

    Some(VirtualCollectionScan {
        vptr_offset: i16::try_from(*count_vptr).ok()?,
        count_slot: i16::try_from(*count_slot).ok()?,
        lookup_slot: i16::try_from(*lookup_slot).ok()?,
        predicate,
    })
}

fn variable(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(candidate) if candidate == name)
}
