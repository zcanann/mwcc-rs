//! Fixed-count XOR folds over adjacent 64-bit records.
//!
//! The legacy CARD routine is a useful pair-register stress case: it validates
//! an index, acquires a control block, XOR-reduces four big-endian `u64` values,
//! stores the pair through an output pointer, and releases the block.  Keep its
//! recognition and measured build-163 schedule out of the scalar long-long
//! dispatcher.

use super::*;

struct SerialFoldPlan<'a> {
    channel: &'a str,
    output: &'a str,
    card: &'a str,
    result: &'a str,
    id: &'a str,
    code: &'a str,
    index: &'a str,
    get_control: &'a str,
    put_control: &'a str,
    channel_count: i16,
    fatal_result: i16,
    work_area_offset: i16,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn ordinary(local: &LocalDeclaration, expected: Type) -> bool {
    local.declared_type == expected
        && local.initializer.is_none()
        && !local.is_volatile
        && !local.is_static
        && local.array_length.is_none()
}

fn assigned<'a>(expression: &'a Expression, target: &str) -> Option<&'a Expression> {
    let Expression::Assign {
        target: found,
        value,
    } = expression
    else {
        return None;
    };
    variable(found, target).then_some(value)
}

fn multiplication_by(expression: &Expression, factor: i64, variable_name: &str) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return false;
    };
    (constant_value(left) == Some(factor) && variable(right, variable_name))
        || (constant_value(right) == Some(factor) && variable(left, variable_name))
}

fn classify(function: &Function) -> Option<SerialFoldPlan<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 2
        || function.locals.len() != 5
    {
        return None;
    }
    let channel = &function.parameters[0];
    let output = &function.parameters[1];
    if channel.parameter_type != Type::Int
        || output.parameter_type != Type::Pointer(Pointee::UnsignedLongLong)
    {
        return None;
    }
    let [card, result, id, code, index] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(card.declared_type, Type::StructPointer { .. })
        || !ordinary(result, Type::Int)
        || !matches!(id.declared_type, Type::StructPointer { .. })
        || !ordinary(code, Type::UnsignedLongLong)
        || !ordinary(index, Type::Int)
        || card.initializer.is_some()
        || card.is_volatile
        || card.is_static
        || card.array_length.is_some()
        || id.initializer.is_some()
        || id.is_volatile
        || id.is_static
        || id.array_length.is_some()
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
    let [validation, acquire, acquire_guard, id_assignment, fold, output_store] = statements else {
        return None;
    };

    let Statement::If {
        condition:
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand: valid_range,
            },
        then_body,
        else_body,
    } = validation
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left: lower_bound,
        right: upper_bound,
    } = valid_range.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LessEqual,
        left: lower_zero,
        right: lower_channel,
    } = lower_bound.as_ref()
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: upper_channel,
        right: upper_count,
    } = upper_bound.as_ref()
    else {
        return None;
    };
    let [Statement::Return(Some(fatal))] = then_body.as_slice() else {
        return None;
    };
    let channel_count = i16::try_from(constant_value(upper_count)?).ok()?;
    let fatal_result = i16::try_from(constant_value(fatal)?).ok()?;
    if !else_body.is_empty()
        || constant_value(lower_zero) != Some(0)
        || !variable(lower_channel, &channel.name)
        || !variable(upper_channel, &channel.name)
    {
        return None;
    }

    let Statement::Assign {
        name: result_target,
        value:
            Expression::Call {
                name: get_control,
                arguments: get_arguments,
            },
    } = acquire
    else {
        return None;
    };
    if result_target != &result.name
        || !matches!(get_arguments.as_slice(), [first, Expression::AddressOf { operand }]
            if variable(first, &channel.name) && variable(operand, &card.name))
    {
        return None;
    }
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::Less,
                left: tested_result,
                right: result_zero,
            },
        then_body: result_body,
        else_body: result_else,
    } = acquire_guard
    else {
        return None;
    };
    if !variable(tested_result, &result.name)
        || constant_value(result_zero) != Some(0)
        || !result_else.is_empty()
        || !matches!(result_body.as_slice(), [Statement::Return(Some(value))]
            if variable(value, &result.name))
    {
        return None;
    }

    let Statement::Assign {
        name: id_target,
        value: Expression::Cast {
            operand: work_area, ..
        },
    } = id_assignment
    else {
        return None;
    };
    let Expression::Member {
        base: work_area_card,
        offset: work_area_offset,
        index_stride: None,
        ..
    } = work_area.as_ref()
    else {
        return None;
    };
    if id_target != &id.name || !variable(work_area_card, &card.name) {
        return None;
    }
    let work_area_offset = i16::try_from(*work_area_offset).ok()?;

    let Statement::Loop {
        kind: LoopKind::For,
        initializer:
            Some(Expression::Comma {
                left: init_code,
                right: init_index,
            }),
        condition: Some(loop_condition),
        step: Some(loop_step),
        body: loop_body,
    } = fold
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Less,
        left: condition_index,
        right: iteration_count,
    } = loop_condition
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left: step_index,
        right: step_one,
    } = assigned(loop_step, &index.name)?
    else {
        return None;
    };
    if constant_value(assigned(init_code, &code.name)?) != Some(0)
        || constant_value(assigned(init_index, &index.name)?) != Some(0)
        || !variable(condition_index, &index.name)
        || constant_value(iteration_count) != Some(4)
        || !variable(step_index, &index.name)
        || constant_value(step_one) != Some(1)
    {
        return None;
    }
    let [Statement::Assign {
        name: code_target,
        value:
            Expression::Binary {
                operator: BinaryOperator::BitXor,
                left: prior_code,
                right: loaded_pair,
            },
    }] = loop_body.as_slice()
    else {
        return None;
    };
    let Expression::Dereference { pointer } = loaded_pair.as_ref() else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::Pointer(Pointee::UnsignedLongLong),
        operand: pair_address,
    } = pointer.as_ref()
    else {
        return None;
    };
    let Expression::AddressOf {
        operand: serial_index,
    } = pair_address.as_ref()
    else {
        return None;
    };
    let Expression::Index {
        base: serial,
        index: byte_index,
    } = serial_index.as_ref()
    else {
        return None;
    };
    let Expression::MemberAddress {
        base: serial_id,
        offset: 0,
        element: Pointee::UnsignedChar,
        index_stride: None,
    } = serial.as_ref()
    else {
        return None;
    };
    if code_target != &code.name
        || !variable(prior_code, &code.name)
        || !variable(serial_id, &id.name)
        || !multiplication_by(byte_index, 8, &index.name)
    {
        return None;
    }

    let Statement::Store {
        target: Expression::Dereference {
            pointer: stored_output,
        },
        value: stored_code,
    } = output_store
    else {
        return None;
    };
    let Expression::Call {
        name: put_control,
        arguments: put_arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    if !variable(stored_output, &output.name)
        || !variable(stored_code, &code.name)
        || !matches!(put_arguments.as_slice(), [first, second]
            if variable(first, &card.name) && constant_value(second) == Some(0))
    {
        return None;
    }

    Some(SerialFoldPlan {
        channel: &channel.name,
        output: &output.name,
        card: &card.name,
        result: &result.name,
        id: &id.name,
        code: &code.name,
        index: &index.name,
        get_control,
        put_control,
        channel_count,
        fatal_result,
        work_area_offset,
    })
}

impl Generator {
    pub(crate) fn try_long_long_serial_fold(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.lookup_general(plan.channel) != Some(3)
            || self.lookup_general(plan.output) != Some(4)
        {
            return Ok(false);
        }

        // Keep the recognized role names live in the plan: they are part of the
        // semantic proof above even though the measured schedule assigns fixed
        // physical homes after recognition.
        let _ = (plan.card, plan.result, plan.id, plan.code, plan.index);
        self.output.pre_scheduled = true;
        self.frame_size = 32;
        self.non_leaf = true;
        self.callee_saved = vec![31];

        let fatal = self.fresh_label();
        let valid = self.fresh_label();
        let acquired = self.fresh_label();
        let done = self.fresh_label();

        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
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
        self.emit_branch_conditional_to(12, 0, fatal); // blt
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: 3,
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
            d: 4,
            a: 1,
            immediate: 16,
        });
        self.record_relocation(RelocationKind::Rel24, plan.get_control);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.get_control.into(),
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, acquired); // bge
        self.emit_branch_to(done);

        self.bind_label(acquired);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(7, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 3,
            offset: plan.work_area_offset,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, 0));
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 5,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::Xor { a: 7, s: 7, b: 3 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 5,
            immediate: 8,
        });
        self.output
            .instructions
            .push(Instruction::Xor { a: 6, s: 6, b: 0 });
        for offset in [8i16, 16, 24] {
            self.output
                .instructions
                .push(Instruction::LoadWord { d: 0, a: 5, offset });
            self.output.instructions.push(Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: 4,
            });
            self.output
                .instructions
                .push(Instruction::Xor { a: 6, s: 6, b: 0 });
            self.output
                .instructions
                .push(Instruction::Xor { a: 7, s: 7, b: 3 });
            if offset != 24 {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 3,
                    a: 5,
                    immediate: offset + 8,
                });
            }
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 7,
            a: 31,
            offset: 4,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 6,
            a: 31,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 16,
        });
        self.record_relocation(RelocationKind::Rel24, plan.put_control);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.put_control.into(),
        });

        self.bind_label(done);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
