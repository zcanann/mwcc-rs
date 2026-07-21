//! Endian-dependent scalar packing through address-taken stack storage.
//!
//! A 16/32/64-bit scalar is spilled in native byte order. A global endian flag
//! selects that frame image directly or a byte-reversed local array, and the
//! selected address is passed to a common buffer routine. The width controls
//! EABI parameter registers, frame size, spill width, and the unrolled copy.

#[allow(unused_imports)]
use super::*;

struct EndianStackPack<'a> {
    flag: &'a str,
    callee: &'a str,
    width: u8,
}

fn var(expression: &Expression, name: &str) -> bool {
    matches!(expression, Expression::Variable(found) if found == name)
}

fn address_of_name(expression: &Expression, name: &str) -> bool {
    match expression {
        Expression::Cast { operand, .. } => address_of_name(operand, name),
        Expression::AddressOf { operand } => var(operand, name),
        _ => false,
    }
}

fn classify<'a>(
    function: &'a Function,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<EndianStackPack<'a>> {
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
        Type::UnsignedShort => 2,
        Type::UnsignedInt => 4,
        Type::UnsignedLongLong => 8,
        _ => return None,
    };
    let [selected, bytes, swapped] = function.locals.as_slice() else {
        return None;
    };
    if !matches!(selected.declared_type, Type::Pointer(Pointee::UnsignedChar))
        || !matches!(bytes.declared_type, Type::Pointer(Pointee::UnsignedChar))
        || swapped.declared_type != Type::UnsignedChar
        || swapped.array_length != Some(width.into())
    {
        return None;
    }
    let [Statement::If {
        condition: Expression::Variable(flag),
        then_body,
        else_body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !globals.contains_key(flag) {
        return None;
    }
    let [Statement::Assign {
        name: selected_then,
        value: native_address,
    }] = then_body.as_slice()
    else {
        return None;
    };
    if selected_then != &selected.name || !address_of_name(native_address, &data.name) {
        return None;
    }
    let [Statement::Assign {
        name: bytes_name,
        value: bytes_address,
    }, Statement::Assign {
        name: selected_else,
        value: swapped_address,
    }, swaps @ ..] = else_body.as_slice()
    else {
        return None;
    };
    if bytes_name != &bytes.name
        || !address_of_name(bytes_address, &data.name)
        || selected_else != &selected.name
        || !var(swapped_address, &swapped.name)
        || swaps.len() != usize::from(width)
    {
        return None;
    }
    for (destination, statement) in swaps.iter().enumerate() {
        let Statement::Store {
            target: Expression::Index { base, index },
            value:
                Expression::Index {
                    base: source,
                    index: source_index,
                },
        } = statement
        else {
            return None;
        };
        if !var(base, &selected.name)
            || constant_value(index) != Some(destination as i64)
            || !var(source, &bytes.name)
            || constant_value(source_index) != Some(i64::from(width) - 1 - destination as i64)
        {
            return None;
        }
    }
    let Expression::Call {
        name: callee,
        arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    let [call_buffer, call_data, call_width] = arguments.as_slice() else {
        return None;
    };
    if !var(call_buffer, &buffer.name)
        || !matches!(call_data, Expression::Cast { operand, .. } if var(operand, &selected.name))
        || constant_value(call_width) != Some(i64::from(width))
    {
        return None;
    }
    Some(EndianStackPack {
        flag,
        callee,
        width,
    })
}

impl Generator {
    pub(crate) fn try_endian_stack_pack(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function, &self.globals) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
        {
            return Ok(false);
        }
        let frame_size = if plan.width == 8 { 24 } else { 16 };
        let global_base = if plan.width == 8 { 4 } else { 5 };
        let swap_offset = if plan.width == 8 { 16 } else { 12 };
        let swapped = self.fresh_label();
        let selected = self.fresh_label();
        self.non_leaf = true;
        self.frame_size = frame_size;
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, plan.flag);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: global_base,
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
                offset: -frame_size,
            },
        ]);
        match plan.width {
            2 => self.output.instructions.push(Instruction::StoreHalfword {
                s: 4,
                a: 1,
                offset: 8,
            }),
            4 => self.output.instructions.push(Instruction::StoreWord {
                s: 4,
                a: 1,
                offset: 8,
            }),
            8 => self.output.instructions.extend([
                Instruction::StoreWord {
                    s: 5,
                    a: 1,
                    offset: 8,
                },
                Instruction::StoreWord {
                    s: 6,
                    a: 1,
                    offset: 12,
                },
            ]),
            _ => unreachable!(),
        }
        self.record_relocation(RelocationKind::Addr16Lo, plan.flag);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: global_base,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, swapped); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 8,
        });
        self.emit_branch_to(selected);
        self.bind_label(swapped);
        for destination in 0..plan.width {
            self.output.instructions.push(Instruction::LoadByteZero {
                d: 0,
                a: 1,
                offset: 8 + i16::from(plan.width - 1 - destination),
            });
            if destination == 0 {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 4,
                    a: 1,
                    immediate: swap_offset,
                });
            }
            self.output.instructions.push(Instruction::StoreByte {
                s: 0,
                a: 1,
                offset: swap_offset + i16::from(destination),
            });
        }
        self.bind_label(selected);
        self.output
            .instructions
            .push(Instruction::load_immediate(5, i16::from(plan.width)));
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_string(),
        });
        self.output.instructions.extend([
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: frame_size,
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
