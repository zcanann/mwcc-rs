//! Conditional removal of one indexed entry from a global aggregate queue.

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

fn compare_member_constant(
    expression: &Expression,
    operator: BinaryOperator,
    global: &str,
) -> Option<(u16, i64)> {
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if *actual != operator {
        return None;
    }
    if let (Some(offset), Some(value)) =
        (member_offset(left, global), constant_value(right))
    {
        Some((offset, value))
    } else {
        Some((member_offset(right, global)?, constant_value(left)?))
    }
}

fn member_update(
    statement: &Statement,
    operator: BinaryOperator,
    global: &str,
    amount: i64,
) -> Option<u16> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let target_offset = member_offset(target, global)?;
    let Expression::Binary {
        operator: actual,
        left,
        right,
    } = value
    else {
        return None;
    };
    (*actual == operator
        && member_offset(left, global) == Some(target_offset)
        && constant_value(right) == Some(amount))
    .then_some(target_offset)
}

struct PredecrementPopPlan<'a> {
    global: &'a str,
    acquire: &'a str,
    copy: &'a str,
    release: &'a str,
    count_offset: i16,
    next_offset: i16,
    array_offset: i16,
    stride: i16,
    wrap_at: i16,
}

fn emit_predecrement(generator: &mut Generator, plan: &PredecrementPopPlan<'_>) {
    generator.non_leaf = true;
    generator.callee_saved = vec![31, 30, 29];
    generator.frame_size = 32;
    generator.output.pre_scheduled = true;
    generator.output.symbol_order = vec![
        plan.global.to_string(),
        plan.acquire.to_string(),
        plan.copy.to_string(),
        plan.release.to_string(),
    ];

    generator
        .output
        .instructions
        .push(Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -32,
        });
    generator
        .output
        .instructions
        .push(Instruction::MoveFromLinkRegister { d: 0 });
    generator.record_relocation(RelocationKind::Addr16Ha, plan.global);
    generator
        .output
        .instructions
        .push(Instruction::load_immediate_shifted(4, 0));
    generator.output.instructions.push(Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 36,
    });
    generator.output.instructions.push(Instruction::StoreWord {
        s: 31,
        a: 1,
        offset: 28,
    });
    generator.output.instructions.push(Instruction::StoreWord {
        s: 30,
        a: 1,
        offset: 24,
    });
    generator
        .output
        .instructions
        .push(Instruction::load_immediate(30, 0));
    generator.output.instructions.push(Instruction::StoreWord {
        s: 29,
        a: 1,
        offset: 20,
    });
    generator
        .output
        .instructions
        .push(Instruction::move_register(29, 3));
    generator.record_relocation(RelocationKind::Addr16Lo, plan.global);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 3,
        a: 4,
        immediate: 0,
    });
    generator.record_relocation(RelocationKind::Rel24, plan.acquire);
    generator
        .output
        .instructions
        .push(Instruction::BranchAndLink {
            target: plan.acquire.to_string(),
        });

    generator.record_relocation(RelocationKind::Addr16Ha, plan.global);
    generator
        .output
        .instructions
        .push(Instruction::load_immediate_shifted(3, 0));
    generator.record_relocation(RelocationKind::Addr16Lo, plan.global);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 31,
        a: 3,
        immediate: 0,
    });
    generator.output.instructions.push(Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: plan.count_offset,
    });
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
    let no_entry = generator.fresh_label();
    generator.emit_branch_conditional_to(4, 1, no_entry);

    generator.output.instructions.push(Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: plan.next_offset,
    });
    generator
        .output
        .instructions
        .push(Instruction::move_register(3, 29));
    generator
        .output
        .instructions
        .push(Instruction::load_immediate(5, plan.stride));
    generator
        .output
        .instructions
        .push(Instruction::MultiplyImmediate {
            d: 0,
            a: 0,
            immediate: plan.stride,
        });
    generator
        .output
        .instructions
        .push(Instruction::Add { d: 4, a: 31, b: 0 });
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 4,
        a: 4,
        immediate: plan.array_offset,
    });
    generator.record_relocation(RelocationKind::Rel24, plan.copy);
    generator
        .output
        .instructions
        .push(Instruction::BranchAndLink {
            target: plan.copy.to_string(),
        });

    generator.output.instructions.push(Instruction::LoadWord {
        d: 3,
        a: 31,
        offset: plan.next_offset,
    });
    generator.output.instructions.push(Instruction::LoadWord {
        d: 4,
        a: 31,
        offset: plan.count_offset,
    });
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 1,
    });
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 3,
        a: 4,
        immediate: -1,
    });
    generator.output.instructions.push(Instruction::StoreWord {
        s: 0,
        a: 31,
        offset: plan.next_offset,
    });
    generator
        .output
        .instructions
        .push(Instruction::CompareWordImmediate {
            a: 0,
            immediate: plan.wrap_at,
        });
    generator.output.instructions.push(Instruction::StoreWord {
        s: 3,
        a: 31,
        offset: plan.count_offset,
    });
    let no_wrap = generator.fresh_label();
    generator.emit_branch_conditional_to(4, 2, no_wrap);
    generator
        .output
        .instructions
        .push(Instruction::load_immediate(0, 0));
    generator.output.instructions.push(Instruction::StoreWord {
        s: 0,
        a: 31,
        offset: plan.next_offset,
    });
    generator.bind_label(no_wrap);
    generator
        .output
        .instructions
        .push(Instruction::load_immediate(30, 1));
    generator.bind_label(no_entry);

    generator.record_relocation(RelocationKind::Addr16Ha, plan.global);
    generator
        .output
        .instructions
        .push(Instruction::load_immediate_shifted(3, 0));
    generator.record_relocation(RelocationKind::Addr16Lo, plan.global);
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 3,
        a: 3,
        immediate: 0,
    });
    generator.record_relocation(RelocationKind::Rel24, plan.release);
    generator
        .output
        .instructions
        .push(Instruction::BranchAndLink {
            target: plan.release.to_string(),
        });
    generator.output.instructions.push(Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 36,
    });
    generator
        .output
        .instructions
        .push(Instruction::move_register(3, 30));
    for (register, offset) in [(31, 28), (30, 24), (29, 20)] {
        generator.output.instructions.push(Instruction::LoadWord {
            d: register,
            a: 1,
            offset,
        });
    }
    generator
        .output
        .instructions
        .push(Instruction::MoveToLinkRegister { s: 0 });
    generator.output.instructions.push(Instruction::AddImmediate {
        d: 1,
        a: 1,
        immediate: 32,
    });
    generator
        .output
        .instructions
        .push(Instruction::BranchToLinkRegister);
    generator.output.anonymous_label_bump += 6;
}

impl Generator {
    /// Lower a lock/copy/update/unlock queue pop. Build 163 assigns the queue
    /// base, output pointer, success/head value, and count pointer to r31..r28.
    /// The success home is deliberately reused as the head-field pointer only
    /// on the taken path, matching the source-range allocator.
    pub(crate) fn try_global_aggregate_pop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Int
        {
            return Ok(false);
        }
        let [output] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(output.parameter_type, Type::StructPointer { .. } | Type::Pointer(_)) {
            return Ok(false);
        }
        let [status] = function.locals.as_slice() else {
            return Ok(false);
        };
        if status.declared_type != Type::Int
            || status.array_length.is_some()
            || status.is_static
            || status.initializer.as_ref().and_then(constant_value) != Some(0)
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &status.name)
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
        if !else_body.is_empty() {
            return Ok(false);
        }
        let Some((count_offset, zero)) =
            compare_member_constant(condition, BinaryOperator::Less, global)
        else {
            return Ok(false);
        };
        if zero != 0 {
            return Ok(false);
        }
        let [copy, decrement, increment, wrap, success] = then_body.as_slice() else {
            return Ok(false);
        };
        let Statement::Expression(Expression::Call {
            name: copy_callee,
            arguments,
        }) = copy
        else {
            return Ok(false);
        };
        let [Expression::Variable(copy_output), Expression::AddressOf { operand }] =
            arguments.as_slice()
        else {
            return Ok(false);
        };
        if copy_output != &output.name {
            return Ok(false);
        }
        let Expression::Index { base, index } = operand.as_ref() else {
            return Ok(false);
        };
        let Expression::Member {
            base: array_owner,
            offset: array_offset,
            member_type: Type::Struct { size: stride, .. },
            index_stride: None,
        } = base.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(array_owner.as_ref(), Expression::Variable(name) if name == global) {
            return Ok(false);
        }
        let Some(next_offset) = member_offset(index, global) else {
            return Ok(false);
        };
        if member_update(decrement, BinaryOperator::Subtract, global, 1) != Some(count_offset)
            || member_update(increment, BinaryOperator::Add, global, 1) != Some(next_offset)
        {
            return Ok(false);
        }
        let Statement::If {
            condition: wrap_condition,
            then_body: wrap_body,
            else_body: wrap_else,
        } = wrap
        else {
            return Ok(false);
        };
        let Some((wrap_offset, wrap_at)) =
            compare_member_constant(wrap_condition, BinaryOperator::Equal, global)
        else {
            return Ok(false);
        };
        let [Statement::Store {
            target: wrap_target,
            value: wrap_value,
        }] = wrap_body.as_slice()
        else {
            return Ok(false);
        };
        if !wrap_else.is_empty()
            || wrap_offset != next_offset
            || member_offset(wrap_target, global) != Some(next_offset)
            || constant_value(wrap_value) != Some(0)
            || !matches!(success, Statement::Assign { name, value }
                if name == &status.name && constant_value(value) == Some(1))
        {
            return Ok(false);
        }
        let (count_offset, next_offset, array_offset, stride, wrap_at) = match (
            i16::try_from(count_offset),
            i16::try_from(next_offset),
            i16::try_from(*array_offset),
            i16::try_from(*stride),
            i16::try_from(wrap_at),
        ) {
            (Ok(count), Ok(next), Ok(array), Ok(stride), Ok(wrap)) => {
                (count, next, array, stride, wrap)
            }
            _ => return Ok(false),
        };

        if self.behavior.frame_convention == FrameConvention::Predecrement {
            let Some(copy) = self.inline_summaries.fixed_size_copy(copy_callee).cloned() else {
                return Ok(false);
            };
            if copy.byte_count != stride {
                return Ok(false);
            }
            emit_predecrement(
                self,
                &PredecrementPopPlan {
                    global,
                    acquire: acquire_callee,
                    copy: &copy.callee,
                    release: release_callee,
                    count_offset,
                    next_offset,
                    array_offset,
                    stride,
                    wrap_at,
                },
            );
            return Ok(true);
        }
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return Ok(false);
        }

        self.non_leaf = true;
        self.callee_saved = vec![31, 30, 29, 28];
        self.frame_size = 24;
        self.output.pre_scheduled = true;
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
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 28,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 28,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, acquire_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: acquire_callee.to_string(),
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 30,
            a: 31,
            immediate: count_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: count_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        let no_entry = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, no_entry);

        self.output.instructions.push(Instruction::AddImmediate {
            d: 29,
            a: 31,
            immediate: next_offset,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: next_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 28,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::MultiplyImmediate {
            d: 0,
            a: 0,
            immediate: stride,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 31, b: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: array_offset,
        });
        self.record_relocation(RelocationKind::Rel24, copy_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: copy_callee.clone(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -1,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 30,
            offset: 0,
        });
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
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 29,
            offset: 0,
        });
        self.output.instructions.push(Instruction::CompareWordImmediate {
            a: 0,
            immediate: wrap_at,
        });
        let no_wrap = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, no_wrap);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 29,
            offset: 0,
        });
        self.bind_label(no_wrap);
        self.output
            .instructions
            .push(Instruction::load_immediate(29, 1));
        self.bind_label(no_entry);

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
            .push(Instruction::move_register(3, 29));
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
        // The taken queue-pop diamond consumes eight legacy anonymous ordinals;
        // two are represented by the bound machine labels above.
        self.output.anonymous_label_bump += 6;
        Ok(true)
    }
}
