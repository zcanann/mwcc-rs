//! Counted byte appends composed with a verified skipped-inline helper.
//!
//! The helper summary proves capacity checking, post-incremented position,
//! byte storage, and length advancement. This owner then emits mwcc's fully
//! inlined leaf loop without depending on either function's source name.

#[allow(unused_imports)]
use super::*;

struct InlinedByteAppend<'a> {
    helper: &'a str,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn assigned(expression: &Expression, name: &str, value: i64) -> bool {
    matches!(expression, Expression::Assign { target, value: assigned }
        if var(target, name) && constant_value(assigned) == Some(value))
}

fn classify(function: &Function) -> Option<InlinedByteAppend<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data, count] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(buffer.parameter_type, Type::StructPointer { .. })
        || data.parameter_type != Type::Pointer(Pointee::UnsignedChar)
        || count.parameter_type != Type::Int
    {
        return None;
    }
    let [error, index] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || error.initializer.is_some()
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
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
    if !matches!(initializer, Expression::Comma { left, right }
        if assigned(left, &index.name, 0) && assigned(right, &error.name, 0))
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::LogicalAnd, left, right
        } if matches!(left.as_ref(), Expression::Binary {
                operator: BinaryOperator::Equal, left, right
            } if var(left, &error.name) && constant_value(right) == Some(0))
            && matches!(right.as_ref(), Expression::Binary {
                operator: BinaryOperator::Less, left, right
            } if var(left, &index.name) && var(right, &count.name)))
        || !matches!(step, Expression::Assign { target, value }
            if var(target, &index.name)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add, left, right
                } if var(left, &index.name) && constant_value(right) == Some(1)))
    {
        return None;
    }
    let [Statement::Assign {
        name: assigned_error,
        value: Expression::Call {
            name: helper,
            arguments,
        },
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_error != &error.name
        || !matches!(arguments.as_slice(), [call_buffer, Expression::Index { base, index: call_index }]
            if var(call_buffer, &buffer.name) && var(base, &data.name)
                && var(call_index, &index.name))
    {
        return None;
    }
    Some(InlinedByteAppend { helper })
}

impl Generator {
    pub(crate) fn try_inlined_byte_append_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        let Some(helper) = self.inline_summaries.byte_append(shape.helper).cloned() else {
            return Ok(false);
        };
        if !self.skipped_inline_names.contains(shape.helper) || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        const ERROR: u8 = 0;
        const INDEX: u8 = 9;
        const POSITION: u8 = 7;
        const BYTE: u8 = 8;
        const TEMP: u8 = 6;
        let condition = self.fresh_label();
        let body = self.fresh_label();
        let append = self.fresh_label();
        let iteration = self.fresh_label();
        let exit = self.fresh_label();
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::load_immediate(INDEX, 0),
            Instruction::load_immediate(ERROR, 0),
        ]);
        self.emit_branch_to(condition);
        self.bind_label(body);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: POSITION,
                a: 3,
                offset: helper.position_offset,
            },
            Instruction::LoadByteZero {
                d: BYTE,
                a: 4,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: POSITION,
                immediate: helper.capacity,
            },
        ]);
        self.emit_branch_conditional_to(12, 0, append); // blt
        self.output
            .instructions
            .push(Instruction::load_immediate(POSITION, helper.overflow));
        self.emit_branch_to(iteration);
        self.bind_label(append);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: TEMP,
                a: POSITION,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: ERROR,
                a: POSITION,
                immediate: helper.data_offset,
            },
            Instruction::StoreWord {
                s: TEMP,
                a: 3,
                offset: helper.position_offset,
            },
            Instruction::load_immediate(POSITION, 0),
            Instruction::StoreByteIndexed {
                s: BYTE,
                a: 3,
                b: ERROR,
            },
            Instruction::LoadWord {
                d: TEMP,
                a: 3,
                offset: helper.length_offset,
            },
            Instruction::AddImmediate {
                d: ERROR,
                a: TEMP,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: ERROR,
                a: 3,
                offset: helper.length_offset,
            },
        ]);
        self.bind_label(iteration);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: ERROR,
                a: POSITION,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: INDEX,
                a: INDEX,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 1,
            },
        ]);
        self.bind_label(condition);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: ERROR,
                immediate: 0,
            });
        self.emit_branch_conditional_to(4, 2, exit); // bne
        self.output
            .instructions
            .push(Instruction::CompareWord { a: INDEX, b: 5 });
        self.emit_branch_conditional_to(12, 0, body); // blt
        self.bind_label(exit);
        self.output
            .instructions
            .push(Instruction::move_register(3, ERROR));
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
