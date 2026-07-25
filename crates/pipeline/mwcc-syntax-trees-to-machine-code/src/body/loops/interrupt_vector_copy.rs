//! Mask-selected interrupt-vector copy loops.
//!
//! The loop calls a small ordinary helper that MWCC expands at the hot site;
//! that helper in turn expands an address translator. Five values survive
//! different portions of the resulting two-call region. Treating either
//! expansion independently loses those joint lifetimes, so this module
//! validates and lowers the complete transaction.

#[allow(unused_imports)]
use super::*;

struct AddressTranslation<'a> {
    base: &'a str,
    state: &'a str,
    state_offset: i16,
    window_size: i16,
}

struct CopyHelper<'a> {
    vector_table: &'a str,
    copy: &'a str,
    flush: &'a str,
    byte_count: i16,
}

struct VectorLoop<'a> {
    translation: AddressTranslation<'a>,
    offsets: &'a str,
    copy_helper: &'a str,
    counter_bound: i16,
    mask_address: u16,
}

fn var(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn literal(expression: &Expression, expected: i64) -> bool {
    constant_value(expression) == Some(expected)
}

fn same_scalar(left: &Expression, right: &Expression) -> bool {
    match (left, right) {
        (Expression::Variable(left), Expression::Variable(right)) => left == right,
        _ => constant_value(left)
            .zip(constant_value(right))
            .is_some_and(|(left, right)| left == right),
    }
}

fn translated_address<'a>(
    expression: &'a Expression,
    input: &Expression,
) -> Option<AddressTranslation<'a>> {
    let expression = match expression {
        Expression::Cast { operand, .. } => operand.as_ref(),
        other => other,
    };
    let Expression::Conditional {
        condition,
        when_true,
        when_false,
        ..
    } = expression
    else {
        return None;
    };
    if !same_scalar(when_true, input) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: lower_bound,
        right: upper_and_enabled,
    } = condition.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: lower_input,
        right: lower_base,
    } = lower_bound.as_ref()
    else {
        return None;
    };
    if !same_scalar(lower_input, input) {
        return None;
    }
    let Expression::Variable(base) = lower_base.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: upper_bound,
        right: enabled,
    } = upper_and_enabled.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: upper_input,
        right: upper_limit,
    } = upper_bound.as_ref()
    else {
        return None;
    };
    if !same_scalar(upper_input, input) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: upper_base,
        right: window_size,
    } = upper_limit.as_ref()
    else {
        return None;
    };
    if !var(upper_base, base) {
        return None;
    }
    let window_size = i16::try_from(constant_value(window_size)?).ok()?;
    let Expression::Binary {
        operator: BinaryOperator::NotEqual,
        left: enabled_bits,
        right: enabled_zero,
    } = enabled.as_ref()
    else {
        return None;
    };
    if !literal(enabled_zero, 0) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: state_member,
        right: enabled_mask,
    } = enabled_bits.as_ref()
    else {
        return None;
    };
    if !literal(enabled_mask, 3) {
        return None;
    }
    let Expression::Member {
        base: state_base,
        offset: state_offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = state_member.as_ref()
    else {
        return None;
    };
    let Expression::Variable(state) = state_base.as_ref() else {
        return None;
    };
    let state_offset = i16::try_from(*state_offset).ok()?;

    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left: masked,
        right: segment,
    } = when_false.as_ref()
    else {
        return None;
    };
    if !literal(segment, 0x8000_0000) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: masked_input,
        right: address_mask,
    } = masked.as_ref()
    else {
        return None;
    };
    if !same_scalar(masked_input, input) || !literal(address_mask, 0x3fff_ffff) {
        return None;
    }

    Some(AddressTranslation {
        base,
        state,
        state_offset,
        window_size,
    })
}

fn same_translation(left: &AddressTranslation<'_>, right: &AddressTranslation<'_>) -> bool {
    left.base == right.base
        && left.state == right.state
        && left.state_offset == right.state_offset
        && left.window_size == right.window_size
}

fn classify_copy_helper(function: &Function) -> Option<(AddressTranslation<'_>, CopyHelper<'_>)> {
    if function.return_type != Type::Void
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [offset] = function.parameters.as_slice() else {
        return None;
    };
    if offset.parameter_type != Type::UnsignedInt {
        return None;
    }
    let [destination] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(
        destination.declared_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let translation = translated_address(
        destination.initializer.as_ref()?,
        &Expression::Variable(offset.name.clone()),
    )?;
    let [Statement::Expression(Expression::Call {
        name: copy,
        arguments: copy_arguments,
    }), Statement::Expression(Expression::Call {
        name: flush,
        arguments: flush_arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    let [copy_destination, source, copy_size] = copy_arguments.as_slice() else {
        return None;
    };
    let [flush_destination, flush_size] = flush_arguments.as_slice() else {
        return None;
    };
    if !var(copy_destination, &destination.name) || !var(flush_destination, &destination.name) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: vector_table,
        right: source_offset,
    } = source
    else {
        return None;
    };
    let Expression::Variable(vector_table) = vector_table.as_ref() else {
        return None;
    };
    if !var(source_offset, &offset.name) {
        return None;
    }
    let byte_count = i16::try_from(constant_value(copy_size)?).ok()?;
    if constant_value(flush_size) != Some(i64::from(byte_count)) {
        return None;
    }
    Some((
        translation,
        CopyHelper {
            vector_table,
            copy,
            flush,
            byte_count,
        },
    ))
}

fn classify_loop(function: &Function) -> Option<VectorLoop<'_>> {
    if function.return_type != Type::Void
        || !function.parameters.is_empty()
        || !function.guards.is_empty()
        || function.return_expression.is_some()
    {
        return None;
    }
    let [counter, mask] = function.locals.as_slice() else {
        return None;
    };
    if counter.declared_type != Type::Int
        || mask.declared_type != Type::UnsignedInt
        || counter.initializer.is_some()
        || mask.initializer.is_some()
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_mask,
        value: mask_value,
    }, Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if assigned_mask != &mask.name {
        return None;
    }
    let Expression::Dereference { pointer } = mask_value else {
        return None;
    };
    let input = match pointer.as_ref() {
        Expression::Cast { operand, .. } => match operand.as_ref() {
            Expression::Conditional { when_true, .. } => when_true.as_ref(),
            _ => return None,
        },
        _ => return None,
    };
    let mask_address = u16::try_from(constant_value(input)?).ok()?;
    let translation = translated_address(pointer, input)?;
    if !matches!(initializer,
        Expression::Assign { target, value }
            if var(target, &counter.name) && literal(value, 0))
        || !matches!(step,
            Expression::Assign { target, value }
                if var(target, &counter.name)
                    && matches!(value.as_ref(), Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if var(left, &counter.name) && literal(right, 1)))
    {
        return None;
    }
    let counter_bound = match condition {
        Expression::Binary {
            operator: BinaryOperator::LessEqual,
            left,
            right,
        } if var(left, &counter.name) => i16::try_from(constant_value(right)?).ok()?,
        _ => return None,
    };
    let [Statement::If {
        condition: selected,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: selected_mask,
        right: selected_bit,
    } = selected
    else {
        return None;
    };
    if !var(selected_mask, &mask.name)
        || !matches!(selected_bit.as_ref(), Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left,
            right,
        } if literal(left, 1) && var(right, &counter.name))
    {
        return None;
    }
    let [Statement::Expression(Expression::Call {
        name: copy_helper,
        arguments,
    })] = then_body.as_slice()
    else {
        return None;
    };
    let [Expression::Index { base, index }] = arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(offsets) = base.as_ref() else {
        return None;
    };
    if !var(index, &counter.name) {
        return None;
    }
    Some(VectorLoop {
        translation,
        offsets,
        copy_helper,
        counter_bound,
        mask_address,
    })
}

impl Generator {
    pub(crate) fn try_interrupt_vector_copy_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.behavior.use_lmw_stmw
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(expanded_caller) = self.inline_bodies.expand_calls(function) else {
            return Ok(false);
        };
        let Some(shape) = classify_loop(&expanded_caller) else {
            return Ok(false);
        };
        let Some(copy_source) = self.inline_bodies.composable_body(shape.copy_helper) else {
            return Ok(false);
        };
        let Some(expanded_copy) = self.inline_bodies.expand_calls(copy_source) else {
            return Ok(false);
        };
        let Some((copy_translation, copy)) = classify_copy_helper(&expanded_copy) else {
            return Ok(false);
        };
        if !same_translation(&shape.translation, &copy_translation)
            || shape.mask_address > i16::MAX as u16
            || shape.counter_bound < 0
            || copy.byte_count <= 0
        {
            return Ok(false);
        }

        const COUNTER: u8 = 27;
        const OFFSETS: u8 = 28;
        const DESTINATION: u8 = 29;
        const STATE: u8 = 30;
        const MASK: u8 = 31;

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![MASK, STATE, DESTINATION, OFFSETS, COUNTER];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;

        let initial_fallback = self.fresh_label();
        let initial_join = self.fresh_label();
        let loop_body = self.fresh_label();
        let skip_copy = self.fresh_label();
        let copy_fallback = self.fresh_label();
        let copy_join = self.fresh_label();

        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
        ]);
        self.record_relocation(RelocationKind::Addr16Ha, shape.translation.base);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.translation.base);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::StoreMultipleWord {
                s: COUNTER,
                a: 1,
                offset: 12,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: shape.mask_address,
            });
        self.emit_branch_conditional_to(12, 1, initial_fallback); // bgt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: shape.translation.window_size,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: shape.mask_address,
            });
        self.emit_branch_conditional_to(4, 1, initial_fallback); // ble
        self.record_relocation(RelocationKind::Addr16Ha, shape.translation.state);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.record_relocation(RelocationKind::Addr16Lo, shape.translation.state);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.translation.state_offset,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 30,
            });
        self.emit_branch_conditional_to(12, 2, initial_fallback); // beq
        self.output
            .instructions
            .push(Instruction::load_immediate(5, shape.mask_address as i16));
        self.emit_branch_to(initial_join);
        self.bind_label(initial_fallback);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, i16::MIN));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 3,
            immediate: shape.mask_address as i16,
        });
        self.bind_label(initial_join);

        self.record_relocation(RelocationKind::Addr16Ha, shape.offsets);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.record_relocation(RelocationKind::Addr16Ha, shape.translation.state);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: MASK,
            a: 5,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.offsets);
        self.output.instructions.push(Instruction::AddImmediate {
            d: OFFSETS,
            a: 4,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.translation.state);
        self.output.instructions.push(Instruction::AddImmediate {
            d: STATE,
            a: 3,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(COUNTER, 0));

        self.bind_label(loop_body);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 1));
        self.output.instructions.push(Instruction::ShiftLeftWord {
            a: 0,
            s: 0,
            b: COUNTER,
        });
        self.output.instructions.push(Instruction::AndRecord {
            a: 0,
            s: MASK,
            b: 0,
        });
        self.emit_branch_conditional_to(12, 2, skip_copy); // beq

        self.record_relocation(RelocationKind::Addr16Ha, shape.translation.base);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 6,
            a: OFFSETS,
            offset: 0,
        });
        self.record_relocation(RelocationKind::Addr16Lo, shape.translation.base);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 6, b: 3 });
        self.emit_branch_conditional_to(12, 0, copy_fallback); // blt
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: shape.translation.window_size,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 6, b: 0 });
        self.emit_branch_conditional_to(4, 0, copy_fallback); // bge
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: STATE,
            offset: shape.translation.state_offset,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 0,
                clear: 30,
            });
        self.emit_branch_conditional_to(12, 2, copy_fallback); // beq
        self.output
            .instructions
            .push(Instruction::move_register(DESTINATION, 6));
        self.emit_branch_to(copy_join);
        self.bind_label(copy_fallback);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 6,
                clear: 2,
            });
        self.output
            .instructions
            .push(Instruction::OrImmediateShifted {
                a: DESTINATION,
                s: 0,
                immediate: 0x8000,
            });
        self.bind_label(copy_join);

        self.record_relocation(RelocationKind::Addr16Ha, copy.vector_table);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::move_register(3, DESTINATION));
        self.record_relocation(RelocationKind::Addr16Lo, copy.vector_table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, copy.byte_count));
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 6 });
        self.record_relocation(RelocationKind::Rel24, copy.copy);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: copy.copy.into(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, DESTINATION));
        self.output
            .instructions
            .push(Instruction::load_immediate(4, copy.byte_count));
        self.record_relocation(RelocationKind::Rel24, copy.flush);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: copy.flush.into(),
        });

        self.bind_label(skip_copy);
        self.output.instructions.push(Instruction::AddImmediate {
            d: COUNTER,
            a: COUNTER,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: OFFSETS,
            a: OFFSETS,
            immediate: 4,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: COUNTER,
                immediate: shape.counter_bound,
            });
        self.emit_branch_conditional_to(4, 1, loop_body); // ble

        self.output
            .instructions
            .push(Instruction::LoadMultipleWord {
                d: COUNTER,
                a: 1,
                offset: 12,
            });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
