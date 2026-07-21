//! Endian-dependent scalar reads through a temporary stack image.
//!
//! The selected destination is either the caller's scalar pointer or a local
//! byte array. After the bulk read, a successful little-endian path copies the
//! temporary bytes to the caller in reverse order. Widths 2/4/8 share one frame.

#[allow(unused_imports)]
use super::*;

struct EndianStackUnpack<'a> {
    flag: &'a str,
    callee: &'a str,
    width: u8,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn cast_of(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Cast { operand, .. } if var(operand, name))
        || var(expression, name)
}

fn classify<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<EndianStackUnpack<'a>> {
    if function.return_type != Type::Int || !function.guards.is_empty() {
        return None;
    }
    let [buffer, data] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        buffer.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) {
        return None;
    }
    let width = match data.parameter_type {
        Type::Pointer(Pointee::UnsignedShort) => 2,
        Type::Pointer(Pointee::UnsignedInt) => 4,
        Type::Pointer(Pointee::UnsignedLongLong) => 8,
        _ => return None,
    };
    let [error, selected, bytes, swapped] = function.locals.as_slice() else {
        return None;
    };
    if error.declared_type != Type::Int
        || error.initializer.is_some()
        || selected.declared_type != Type::Pointer(Pointee::UnsignedChar)
        || bytes.declared_type != Type::Pointer(Pointee::UnsignedChar)
        || swapped.declared_type != Type::UnsignedChar
        // Some SDK sources accidentally declare this as `sizeof(pointer)` even
        // for a wider pointee; the explicit reversed stores below are the
        // authoritative extent and mwcc reserves enough linkage scratch space.
        || swapped.array_length.is_none()
        || !matches!(function.return_expression.as_ref(), Some(value) if var(value, &error.name))
    {
        return None;
    }
    let [select, read, reverse] = function.statements.as_slice() else {
        return None;
    };
    let Statement::If {
        condition: Expression::Variable(flag),
        then_body,
        else_body,
    } = select
    else {
        return None;
    };
    if !globals.contains_key(flag)
        || !matches!(then_body.as_slice(), [Statement::Assign { name, value }]
            if name == &selected.name && cast_of(value, &data.name))
        || !matches!(else_body.as_slice(), [Statement::Assign { name, value }]
            if name == &selected.name && var(value, &swapped.name))
    {
        return None;
    }
    let Statement::Assign {
        name: error_name,
        value: Expression::Call {
            name: callee,
            arguments,
        },
    } = read
    else {
        return None;
    };
    if error_name != &error.name
        || !matches!(arguments.as_slice(), [call_buffer, call_data, call_width]
            if var(call_buffer, &buffer.name) && cast_of(call_data, &selected.name)
                && constant_value(call_width) == Some(i64::from(width)))
    {
        return None;
    }
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = reverse
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(condition, Expression::Binary {
            operator: BinaryOperator::LogicalAnd, left, right
        } if matches!(left.as_ref(), Expression::Unary {
                operator: UnaryOperator::LogicalNot, operand
            } if var(operand, flag))
            && matches!(right.as_ref(), Expression::Binary {
                operator: BinaryOperator::Equal, left, right
            } if var(left, &error.name) && constant_value(right) == Some(0)))
    {
        return None;
    }
    let [Statement::Assign {
        name: bytes_name,
        value: bytes_value,
    }, stores @ ..] = then_body.as_slice()
    else {
        return None;
    };
    if bytes_name != &bytes.name
        || !cast_of(bytes_value, &data.name)
        || stores.len() != usize::from(width)
    {
        return None;
    }
    for (destination, statement) in stores.iter().enumerate() {
        if !matches!(statement, Statement::Store {
            target: Expression::Index { base, index },
            value: Expression::Index { base: source, index: source_index },
        } if var(base, &bytes.name) && constant_value(index) == Some(destination as i64)
            && var(source, &selected.name)
            && constant_value(source_index)
                == Some(i64::from(width) - 1 - destination as i64))
        {
            return None;
        }
    }
    Some(EndianStackUnpack {
        flag,
        callee,
        width,
    })
}

impl Generator {
    pub(crate) fn try_endian_stack_unpack(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function, &self.globals) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        {
            return Ok(false);
        }
        const SELECTED: u8 = 31;
        const OUTPUT: u8 = 30;
        let temporary = self.fresh_label();
        let selected = self.fresh_label();
        let epilogue = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![SELECTED, OUTPUT];
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, plan.flag);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0,
            });
        self.output.instructions.extend([
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
                s: SELECTED,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: OUTPUT,
                a: 1,
                offset: 16,
            },
            Instruction::AddImmediate {
                d: OUTPUT,
                a: 4,
                immediate: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Addr16Lo, plan.flag);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 5,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, temporary); // beq
        self.output
            .instructions
            .push(Instruction::move_register(SELECTED, OUTPUT));
        self.emit_branch_to(selected);
        self.bind_label(temporary);
        self.output.instructions.push(Instruction::AddImmediate {
            d: SELECTED,
            a: 1,
            immediate: 8,
        });
        self.bind_label(selected);
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 4,
                a: SELECTED,
                immediate: 0,
            },
            Instruction::load_immediate(5, i16::from(plan.width)),
        ]);
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_string(),
        });
        self.record_relocation(RelocationKind::Addr16Ha, plan.flag);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0,
            });
        self.record_relocation(RelocationKind::Addr16Lo, plan.flag);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 4,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, epilogue); // bne
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, epilogue); // bne
        for destination in 0..plan.width {
            self.output.instructions.extend([
                Instruction::LoadByteZero {
                    d: 0,
                    a: SELECTED,
                    offset: i16::from(plan.width - 1 - destination),
                },
                Instruction::StoreByte {
                    s: 0,
                    a: OUTPUT,
                    offset: i16::from(destination),
                },
            ]);
        }
        self.bind_label(epilogue);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: SELECTED,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: OUTPUT,
                a: 1,
                offset: 16,
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
