//! Status-guarded indexed call loops.
//!
//! These loops stop on either a nonzero callee result or a signed element count.
//! A call may consume the indexed word value or its address. Wide addressed
//! elements and word values use an advancing cursor; byte addresses preserve
//! mwcc's distinct base-plus-index schedule. The status remains in r3 across
//! the bottom-tested condition.

#[allow(unused_imports)]
use super::*;

struct StatusIndexedCall<'a> {
    callee: &'a str,
    argument: IndexedArgument,
}

#[derive(Clone, Copy)]
enum IndexedArgument {
    WordValue,
    Address { stride: u8 },
}

impl IndexedArgument {
    fn cursor_stride(self) -> Option<u8> {
        match self {
            Self::WordValue => Some(4),
            Self::Address { stride: 1 } => None,
            Self::Address { stride } => Some(stride),
        }
    }
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn assign_constant(expression: &Expression, name: &str, constant: i64) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if var(target, name) && constant_value(value) == Some(constant))
}

fn classify(function: &Function) -> Option<StatusIndexedCall<'_>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [context, data, count] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        context.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || count.parameter_type != Type::Int
    {
        return None;
    }
    let Type::Pointer(data_pointee) = data.parameter_type else {
        return None;
    };
    let [status, index] = function.locals.as_slice() else {
        return None;
    };
    if status.declared_type != Type::Int
        || status.initializer.is_some()
        || index.declared_type != Type::Int
        || index.initializer.is_some()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &status.name))
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
        if assign_constant(left, &index.name, 0)
            && assign_constant(right, &status.name, 0))
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::LogicalAnd, left, right
        } if matches!(left.as_ref(), Expression::Binary {
                operator: BinaryOperator::Equal, left, right
            } if var(left, &status.name) && constant_value(right) == Some(0))
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
        name: assigned_status,
        value: Expression::Call {
            name: callee,
            arguments,
        },
    }] = body.as_slice()
    else {
        return None;
    };
    if assigned_status != &status.name {
        return None;
    }
    let [call_context, indexed_argument] = arguments.as_slice() else {
        return None;
    };
    if !var(call_context, &context.name) {
        return None;
    }
    let indexed = |expression: &Expression| {
        matches!(expression, Expression::Index { base, index: call_index }
            if var(base, &data.name) && var(call_index, &index.name))
    };
    let argument = match indexed_argument {
        Expression::Index { .. }
            if data_pointee == Pointee::UnsignedInt && indexed(indexed_argument) =>
        {
            IndexedArgument::WordValue
        }
        Expression::AddressOf { operand }
            if indexed(operand) && matches!(data_pointee.size(), 1 | 2 | 4 | 8) =>
        {
            IndexedArgument::Address {
                stride: data_pointee.size(),
            }
        }
        _ => return None,
    };
    Some(StatusIndexedCall { callee, argument })
}

impl Generator {
    pub(crate) fn try_status_indexed_call_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        const HIGH: u8 = 31;
        const MIDDLE: u8 = 30;
        const LOW: u8 = 29;
        const CONTEXT: u8 = 28;
        let cursor_stride = shape.argument.cursor_stride();
        let body = self.fresh_label();
        let condition = self.fresh_label();
        let exit = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![HIGH, MIDDLE, LOW, CONTEXT];
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: HIGH,
                a: 1,
                offset: 20,
            },
        ]);
        if let Some(stride) = cursor_stride {
            self.output.instructions.extend([
                Instruction::StoreWord {
                    s: MIDDLE,
                    a: 1,
                    offset: 16,
                },
                Instruction::load_immediate(MIDDLE, 0),
                Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: MIDDLE,
                    shift: stride.trailing_zeros() as u8,
                },
                Instruction::StoreWord {
                    s: LOW,
                    a: 1,
                    offset: 12,
                },
                Instruction::Add {
                    d: HIGH,
                    a: 4,
                    b: 0,
                },
                Instruction::AddImmediate {
                    d: LOW,
                    a: 5,
                    immediate: 0,
                },
            ]);
        } else {
            self.output.instructions.extend([
                Instruction::load_immediate(HIGH, 0),
                Instruction::StoreWord {
                    s: MIDDLE,
                    a: 1,
                    offset: 16,
                },
                Instruction::AddImmediate {
                    d: MIDDLE,
                    a: 5,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: LOW,
                    a: 1,
                    offset: 12,
                },
                Instruction::AddImmediate {
                    d: LOW,
                    a: 4,
                    immediate: 0,
                },
            ]);
        }
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: CONTEXT,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: CONTEXT,
                a: 3,
                immediate: 0,
            },
            Instruction::load_immediate(3, 0),
        ]);
        self.emit_branch_to(condition);
        self.bind_label(body);
        self.output.instructions.push(match shape.argument {
            IndexedArgument::WordValue => Instruction::move_register(3, CONTEXT),
            IndexedArgument::Address { .. } => Instruction::AddImmediate {
                d: 3,
                a: CONTEXT,
                immediate: 0,
            },
        });
        self.output.instructions.push(match shape.argument {
            IndexedArgument::WordValue => Instruction::LoadWord {
                d: 4,
                a: HIGH,
                offset: 0,
            },
            IndexedArgument::Address { stride: 1 } => Instruction::Add {
                d: 4,
                a: LOW,
                b: HIGH,
            },
            IndexedArgument::Address { .. } => Instruction::AddImmediate {
                d: 4,
                a: HIGH,
                immediate: 0,
            },
        });
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        if let Some(stride) = cursor_stride {
            self.output.instructions.extend([
                Instruction::AddImmediate {
                    d: MIDDLE,
                    a: MIDDLE,
                    immediate: 1,
                },
                Instruction::AddImmediate {
                    d: HIGH,
                    a: HIGH,
                    immediate: i16::from(stride),
                },
            ]);
        } else {
            self.output.instructions.push(Instruction::AddImmediate {
                d: HIGH,
                a: HIGH,
                immediate: 1,
            });
        }
        self.bind_label(condition);
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, exit); // bne
        self.output.instructions.push(if cursor_stride.is_some() {
            Instruction::CompareWord { a: MIDDLE, b: LOW }
        } else {
            Instruction::CompareWord { a: HIGH, b: MIDDLE }
        });
        self.emit_branch_conditional_to(12, 0, body); // blt
        self.bind_label(exit);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: HIGH,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: MIDDLE,
                a: 1,
                offset: 16,
            },
            Instruction::LoadWord {
                d: LOW,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: CONTEXT,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
