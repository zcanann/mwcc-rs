//! Conditional insertion into an indexed global aggregate queue.

#[allow(unused_imports)]
use super::*;

fn global_address_call(statement: &Statement) -> Option<(&str, &str)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [Expression::AddressOf { operand }] = arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(global) = operand.as_ref() else {
        return None;
    };
    Some((name, global))
}

fn member_offset(expression: &Expression, global: &str) -> Option<u16> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == global)
        .then(|| u16::try_from(*offset).ok())?
}

fn member_increment(statement: &Statement, global: &str, expected: u16) -> bool {
    let Statement::Store { target, value } = statement else {
        return false;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = value
    else {
        return false;
    };
    member_offset(target, global) == Some(expected)
        && member_offset(left, global) == Some(expected)
        && constant_value(right) == Some(1)
}

impl Generator {
    /// Lower a lock/full-check/copy/update/unlock queue post. The legacy
    /// allocator preserves the input in r31, the queue base in r30, the count
    /// field address in r29, and the status in r28; r31 is reused for the
    /// selected entry after its incoming value has moved to the copy argument.
    pub(crate) fn try_global_aggregate_post(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Int
        {
            return Ok(false);
        }
        let [input] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(input.parameter_type, Type::StructPointer { .. } | Type::Pointer(_)) {
            return Ok(false);
        }
        let [result, index_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if result.declared_type != Type::Int
            || result.initializer.as_ref().and_then(constant_value) != Some(0)
            || index_local.declared_type != Type::Int
            || index_local.initializer.is_some()
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &result.name)
        {
            return Ok(false);
        }
        let [acquire, conditional, release] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some((acquire_callee, global)) = global_address_call(acquire) else {
            return Ok(false);
        };
        let Some((release_callee, release_global)) = global_address_call(release) else {
            return Ok(false);
        };
        if release_global != global
            || !matches!(self.globals.get(global), Some(Type::Struct { size, .. }) if *size > 8)
        {
            return Ok(false);
        }
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = conditional
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } = condition
        else {
            return Ok(false);
        };
        let (count_offset, capacity) = if let (Some(offset), Some(value)) =
            (member_offset(left, global), constant_value(right))
        {
            (offset, value)
        } else if let (Some(offset), Some(value)) =
            (member_offset(right, global), constant_value(left))
        {
            (offset, value)
        } else {
            return Ok(false);
        };
        let [Statement::Assign {
            name: full_result,
            value: full_value,
        }] = then_body.as_slice()
        else {
            return Ok(false);
        };
        if full_result != &result.name {
            return Ok(false);
        }
        let Some(full_error) = constant_value(full_value) else {
            return Ok(false);
        };
        let [index_assign, copy, entry_id_store, id_increment, repair, count_increment] =
            else_body.as_slice()
        else {
            return Ok(false);
        };

        let Statement::Assign {
            name: assigned_index,
            value:
                Expression::Binary {
                    operator: BinaryOperator::Modulo,
                    left: sum,
                    right: modulus,
                },
        } = index_assign
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Add,
            left: next_member,
            right: count_member,
        } = sum.as_ref()
        else {
            return Ok(false);
        };
        let Some(next_offset) = member_offset(next_member, global) else {
            return Ok(false);
        };
        if assigned_index != &index_local.name
            || member_offset(count_member, global) != Some(count_offset)
            || constant_value(modulus) != Some(capacity)
            || capacity != 2
        {
            return Ok(false);
        }

        let Statement::Expression(Expression::Call {
            name: copy_callee,
            arguments: copy_arguments,
        }) = copy
        else {
            return Ok(false);
        };
        let [Expression::AddressOf { operand: destination }, Expression::Variable(copy_input)] =
            copy_arguments.as_slice()
        else {
            return Ok(false);
        };
        if copy_input != &input.name {
            return Ok(false);
        }
        let Expression::Index {
            base: array_member,
            index: copy_index,
        } = destination.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Member {
            base: array_owner,
            offset: array_offset,
            member_type: Type::Struct { size: stride, .. },
            index_stride: None,
        } = array_member.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(array_owner.as_ref(), Expression::Variable(name) if name == global)
            || !matches!(copy_index.as_ref(), Expression::Variable(name) if name == &index_local.name)
        {
            return Ok(false);
        }

        let Statement::Store {
            target:
                Expression::Member {
                    base: entry,
                    offset: entry_id_offset,
                    index_stride: Some(entry_stride),
                    ..
                },
            value: id_value,
        } = entry_id_store
        else {
            return Ok(false);
        };
        let Expression::Index {
            base: entry_array,
            index: entry_index,
        } = entry.as_ref()
        else {
            return Ok(false);
        };
        let same_entry_array = matches!(entry_array.as_ref(), Expression::Member {
            base,
            offset,
            member_type: Type::Struct { size, .. },
            index_stride: None,
        } if *offset == *array_offset
            && *size == *stride
            && matches!(base.as_ref(), Expression::Variable(name) if name == global));
        if !same_entry_array
            || !matches!(entry_index.as_ref(), Expression::Variable(name) if name == &index_local.name)
            || *entry_stride != *stride
        {
            return Ok(false);
        }
        let Some(id_offset) = member_offset(id_value, global) else {
            return Ok(false);
        };
        if !member_increment(id_increment, global, id_offset)
            || !member_increment(count_increment, global, count_offset)
        {
            return Ok(false);
        }

        let Statement::If {
            condition: repair_condition,
            then_body: repair_body,
            else_body: repair_else,
        } = repair
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::Less,
            left: repaired_member,
            right: minimum,
        } = repair_condition
        else {
            return Ok(false);
        };
        let [Statement::Store {
            target: repaired_target,
            value: repaired_value,
        }] = repair_body.as_slice()
        else {
            return Ok(false);
        };
        let Some(minimum) = constant_value(minimum) else {
            return Ok(false);
        };
        if !repair_else.is_empty()
            || member_offset(repaired_member, global) != Some(id_offset)
            || member_offset(repaired_target, global) != Some(id_offset)
            || constant_value(repaired_value) != Some(minimum)
        {
            return Ok(false);
        }

        let (count_offset, next_offset, array_offset, stride, entry_id_offset, id_offset) = match (
            i16::try_from(count_offset),
            i16::try_from(next_offset),
            i16::try_from(*array_offset),
            i16::try_from(*stride),
            i16::try_from(*entry_id_offset),
            i16::try_from(id_offset),
        ) {
            (Ok(count), Ok(next), Ok(array), Ok(stride), Ok(entry_id), Ok(id)) => {
                (count, next, array, stride, entry_id, id)
            }
            _ => return Ok(false),
        };
        let (capacity, full_error, minimum) = match (
            i16::try_from(capacity),
            i16::try_from(full_error),
            i16::try_from(minimum),
        ) {
            (Ok(capacity), Ok(error), Ok(minimum)) => (capacity, error, minimum),
            _ => return Ok(false),
        };

        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29, 28];
        self.frame_size = 24;
        self.output.pre_scheduled = true;
        // This three-way transaction consumes nine anonymous optimizer
        // ordinals in the 1.1 lineage. Earlier speculative lowerings may have
        // recorded bumps before declining the function, so establish the
        // semantic owner's accounting explicitly.
        self.output.anonymous_label_bump = 9;
        self.output.post_constant_label_bump = 0;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, global);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 16,
        });
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 28,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(28, 0));
        self.record_relocation(RelocationKind::Rel24, acquire_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: acquire_callee.to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 30,
            immediate: count_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: count_offset,
        });
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 3,
            immediate: capacity,
        });
        let has_space = self.fresh_label();
        let finish_transaction = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, has_space);
        self.output
            .instructions
            .push(Instruction::load_immediate(28, full_error));
        self.emit_branch_to(finish_transaction);
        self.bind_label(has_space);

        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: next_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 31,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 0, a: 0, b: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 0, shift: 1 });
        self.output
            .instructions
            .push(Instruction::AddToZeroExtended { d: 3, a: 3 });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 3, s: 3, shift: 1 });
        self.output
            .instructions
            .push(Instruction::SubtractFromCarrying { d: 3, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MultiplyImmediate {
            d: 0,
            a: 3,
            immediate: stride,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 31, a: 30, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: array_offset,
        });
        self.record_relocation(RelocationKind::Rel24, copy_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: copy_callee.clone(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 30,
            immediate: id_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: id_offset,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: array_offset + entry_id_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: id_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: id_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: id_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate { a: 0, immediate: minimum as u16 });
        let id_valid = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, id_valid);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, minimum));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.bind_label(id_valid);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 29,
            offset: 0,
        });
        self.bind_label(finish_transaction);

        self.record_relocation(RelocationKind::Addr16Ha, global);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, release_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: release_callee.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 28));
        for (register, offset) in [(31, 20), (30, 16), (29, 12), (28, 8)] {
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: 1,
                offset,
            });
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
