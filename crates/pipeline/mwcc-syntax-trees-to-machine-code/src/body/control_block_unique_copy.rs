//! Bounds-checked control-block transactions that copy one indexed unique code.
//!
//! This family keeps both incoming arguments across several calls, obtains a
//! stack-resident control block, locks an indexed record table, copies a fixed
//! byte range, unlocks it, and finally releases the control block.  Recognition
//! proves that complete transaction before the measured build-163 register and
//! call schedule is selected.

use super::*;

struct UniqueCopyPlan<'a> {
    channel: &'a str,
    output: &'a str,
    card: &'a str,
    result: &'a str,
    record_table: &'a str,
    acquire: &'a str,
    lock: &'a str,
    copy: &'a str,
    unlock: &'a str,
    release: &'a str,
    channel_count: i16,
    fatal_result: i16,
    record_stride: i16,
    code_offset: i16,
    code_size: i16,
    unlock_argument: i16,
    release_argument: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn ordinary(local: &LocalDeclaration) -> bool {
    local.initializer.is_none()
        && !local.is_volatile
        && !local.is_static
        && local.array_length.is_none()
}

fn return_value(body: &[Statement]) -> Option<&Expression> {
    let [Statement::Return(Some(value))] = body else {
        return None;
    };
    Some(value)
}

fn classify(function: &Function) -> Option<UniqueCopyPlan<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 2
        || function.locals.len() != 3
    {
        return None;
    }
    let [channel, output] = function.parameters.as_slice() else {
        return None;
    };
    if channel.parameter_type != Type::Int
        || !matches!(
            output.parameter_type,
            Type::Pointer(Pointee::UnsignedLongLong | Pointee::UnsignedChar)
        )
    {
        return None;
    }
    let [card, result, record_table] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(card.declared_type, Type::StructPointer { .. })
        || result.declared_type != Type::Int
        || !matches!(record_table.declared_type, Type::StructPointer { .. })
        || !ordinary(card)
        || !ordinary(result)
        || !ordinary(record_table)
    {
        return None;
    }

    let statements = match function.statements.as_slice() {
        [Statement::Expression(Expression::Cast {
            target_type: Type::Void,
            operand,
        }), rest @ ..]
            if constant_value(operand) == Some(0) =>
        {
            rest
        }
        statements => statements,
    };
    let [bounds, acquire_statement, acquire_guard, lock_statement, copy_statement, unlock_statement] =
        statements
    else {
        return None;
    };

    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left: below_zero,
                right: above_range,
            },
        then_body,
        else_body,
    } = bounds
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Greater,
        left: zero,
        right: lower_channel,
    } = below_zero.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::GreaterEqual,
        left: upper_channel,
        right: channel_count,
    } = above_range.as_ref()
    else {
        return None;
    };
    let fatal_result = i16::try_from(constant_value(return_value(then_body)?)?).ok()?;
    let channel_count = i16::try_from(constant_value(channel_count)?).ok()?;
    if !else_body.is_empty()
        || constant_value(zero) != Some(0)
        || !variable(lower_channel, &channel.name)
        || !variable(upper_channel, &channel.name)
    {
        return None;
    }

    let Statement::Assign {
        name: acquire_result,
        value:
            Expression::Call {
                name: acquire,
                arguments: acquire_arguments,
            },
    } = acquire_statement
    else {
        return None;
    };
    if acquire_result != &result.name
        || !matches!(acquire_arguments.as_slice(), [first, Expression::AddressOf { operand }]
            if variable(first, &channel.name) && variable(operand, &card.name))
    {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: tested_result,
                right: zero,
            },
        then_body,
        else_body,
    } = acquire_guard
    else {
        return None;
    };
    if !variable(tested_result, &result.name)
        || constant_value(zero) != Some(0)
        || !else_body.is_empty()
        || !variable(return_value(then_body)?, &result.name)
    {
        return None;
    }

    let Statement::Assign {
        name: locked_table,
        value:
            Expression::Call {
                name: lock,
                arguments: lock_arguments,
            },
    } = lock_statement
    else {
        return None;
    };
    if locked_table != &record_table.name || !lock_arguments.is_empty() {
        return None;
    }

    let Statement::Expression(Expression::Call {
        name: copy,
        arguments: copy_arguments,
    }) = copy_statement
    else {
        return None;
    };
    let [copy_destination, copy_source, copy_size] = copy_arguments.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: indexed_record,
        right: code_offset,
    } = copy_source
    else {
        return None;
    };
    let Expression::Index {
        base: record_array,
        index: record_index,
    } = indexed_record.as_ref()
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: table,
        offset: 0,
        element: Pointee::UnsignedChar,
        index_stride: Some(record_stride),
    } = record_array.as_ref()
    else {
        return None;
    };
    let record_stride = i16::try_from(*record_stride).ok()?;
    let code_offset = i16::try_from(constant_value(code_offset)?).ok()?;
    let code_size = i16::try_from(constant_value(copy_size)?).ok()?;
    if !variable(copy_destination, &output.name)
        || !variable(table, &record_table.name)
        || !variable(record_index, &channel.name)
    {
        return None;
    }

    let Statement::Expression(Expression::Call {
        name: unlock,
        arguments: unlock_arguments,
    }) = unlock_statement
    else {
        return None;
    };
    let [unlock_value] = unlock_arguments.as_slice() else {
        return None;
    };
    let unlock_argument = i16::try_from(constant_value(unlock_value)?).ok()?;

    let Expression::Call {
        name: release,
        arguments: release_arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    let [released_card, release_value] = release_arguments.as_slice() else {
        return None;
    };
    let release_argument = i16::try_from(constant_value(release_value)?).ok()?;
    if !variable(released_card, &card.name) {
        return None;
    }

    Some(UniqueCopyPlan {
        channel: &channel.name,
        output: &output.name,
        card: &card.name,
        result: &result.name,
        record_table: &record_table.name,
        acquire,
        lock,
        copy,
        unlock,
        release,
        channel_count,
        fatal_result,
        record_stride,
        code_offset,
        code_size,
        unlock_argument,
        release_argument,
    })
}

impl Generator {
    pub(crate) fn try_control_block_unique_copy(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.lookup_general(plan.channel) != Some(3)
            || self.lookup_general(plan.output) != Some(4)
        {
            return Ok(false);
        }
        let _ = (plan.card, plan.result, plan.record_table);

        self.output.pre_scheduled = true;
        self.frame_size = 32;
        self.non_leaf = true;
        self.callee_saved = vec![31, 30];

        let fatal = self.fresh_label();
        let valid = self.fresh_label();
        let acquired = self.fresh_label();
        let done = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
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
                offset: -32,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 4,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 24,
        });
        self.output
            .instructions
            .push(Instruction::OrRecord { a: 30, s: 3, b: 3 });
        self.emit_branch_conditional_to(12, 0, fatal); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 30,
                immediate: plan.channel_count,
            });
        self.emit_branch_conditional_to(12, 0, valid); // blt
        self.bind_label(fatal);
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.fatal_result));
        self.emit_branch_to(done);

        self.bind_label(valid);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 30,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 16,
        });
        self.record_relocation(RelocationKind::Rel24, plan.acquire);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.acquire.into(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, acquired); // bge
        self.emit_branch_to(done);

        self.bind_label(acquired);
        self.record_relocation(RelocationKind::Rel24, plan.lock);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.lock.into(),
        });
        self.output
            .instructions
            .push(Instruction::MultiplyImmediate {
                d: 4,
                a: 30,
                immediate: plan.record_stride,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: plan.code_offset,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 4, a: 0, b: 4 });
        self.output
            .instructions
            .push(Instruction::load_immediate(5, plan.code_size));
        self.record_relocation(RelocationKind::Rel24, plan.copy);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.copy.into(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.unlock_argument));
        self.record_relocation(RelocationKind::Rel24, plan.unlock);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.unlock.into(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, plan.release_argument));
        self.record_relocation(RelocationKind::Rel24, plan.release);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.release.into(),
        });

        self.bind_label(done);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
