//! Paired bitfield updates followed by fixed-port command/data writes.

#[allow(unused_imports)]
use super::*;

struct BitfieldUpdate<'a> {
    parameter: &'a str,
    preserve_mask: u32,
    shift: u8,
}

fn recognize_update<'a>(
    statement: &'a Statement,
    base_name: &str,
    member_offset: u32,
) -> Option<BitfieldUpdate<'a>> {
    let Statement::Store {
        target:
            Expression::Member {
                base: target_base,
                offset: target_offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        value:
            Expression::Binary {
                operator: BinaryOperator::BitOr,
                left,
                right,
            },
    } = statement
    else {
        return None;
    };
    if *target_offset != member_offset
        || !matches!(target_base.as_ref(), Expression::Variable(name) if name == base_name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: old_value,
        right: mask,
    } = left.as_ref()
    else {
        return None;
    };
    if !matches!(old_value.as_ref(), Expression::Member { base, offset, member_type: Type::UnsignedInt, index_stride: None }
        if *offset == member_offset
            && matches!(base.as_ref(), Expression::Variable(name) if name == base_name))
    {
        return None;
    }
    let preserve_mask = constant_value(mask)? as u32;
    contiguous_mask((!preserve_mask) as i64)?;

    let Expression::Binary {
        operator: BinaryOperator::ShiftLeft,
        left: inserted,
        right: shift,
    } = right.as_ref()
    else {
        return None;
    };
    let inserted = match inserted.as_ref() {
        Expression::Cast { operand, .. } => operand.as_ref(),
        expression => expression,
    };
    let Expression::Variable(parameter) = inserted else {
        return None;
    };
    let shift = constant_value(shift).and_then(|value| u8::try_from(value).ok())?;
    (shift <= 31).then_some(BitfieldUpdate {
        parameter,
        preserve_mask,
        shift,
    })
}

fn fixed_port_store(statement: &Statement) -> Option<(u32, Type, &Expression)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let Expression::Member {
        base,
        offset: 0,
        member_type,
        index_stride: None,
    } = target
    else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::StructPointer { .. },
        operand,
    } = base.as_ref()
    else {
        return None;
    };
    let address = constant_value(operand).and_then(|value| u32::try_from(value).ok())?;
    Some((address, *member_type, value))
}

impl Generator {
    /// Lower an SDK register update consisting of two masked inserts into one state word, followed
    /// by an 8-bit command and the updated 32-bit word on a fixed port. The second insert is
    /// prepared before the first member load, matching build 163's latency schedule.
    pub(crate) fn try_fixed_port_bitfield_update(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_expression.is_some()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        let [first_parameter, second_parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if first_parameter.parameter_type != Type::UnsignedChar
            || second_parameter.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let [alias] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(alias.declared_type, Type::StructPointer { .. })
            || alias.array_length.is_some()
            || alias.is_static
        {
            return Ok(false);
        }
        let Some(Expression::Variable(global)) = alias.initializer.as_ref() else {
            return Ok(false);
        };
        let Some(&global_type) = self.globals.get(global.as_str()) else {
            return Ok(false);
        };

        let [first_statement, second_statement, command_statement, data_statement, flag_statement] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: first_target_base,
                    offset: member_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            ..
        } = first_statement
        else {
            return Ok(false);
        };
        if !matches!(first_target_base.as_ref(), Expression::Variable(name) if name == &alias.name)
        {
            return Ok(false);
        }
        let first = recognize_update(first_statement, &alias.name, *member_offset);
        let second = recognize_update(second_statement, &alias.name, *member_offset);
        let (Some(first), Some(second)) = (first, second) else {
            return Ok(false);
        };
        if first.parameter != first_parameter.name
            || second.parameter != second_parameter.name
        {
            return Ok(false);
        }
        let Some((first_begin, first_end)) = rlwinm_mask(first.preserve_mask as i64) else {
            return Ok(false);
        };
        let Some((first_insert_begin, first_insert_end)) =
            contiguous_mask((!first.preserve_mask) as i64)
        else {
            return Ok(false);
        };
        let Some((second_begin, second_end)) = rlwinm_mask(second.preserve_mask as i64) else {
            return Ok(false);
        };

        let Some((port, Type::UnsignedChar, command_value)) =
            fixed_port_store(command_statement)
        else {
            return Ok(false);
        };
        let Some(command) = constant_value(command_value).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some((data_port, Type::UnsignedInt, data_value)) = fixed_port_store(data_statement)
        else {
            return Ok(false);
        };
        if data_port != port
            || !matches!(data_value, Expression::Member { base, offset, member_type: Type::UnsignedInt, index_stride: None }
                if *offset == *member_offset
                    && matches!(base.as_ref(), Expression::Variable(name) if name == &alias.name))
        {
            return Ok(false);
        }
        let Statement::Store {
            target:
                Expression::Member {
                    base: flag_base,
                    offset: flag_offset,
                    member_type: Type::UnsignedShort,
                    index_stride: None,
                },
            value: flag_value,
        } = flag_statement
        else {
            return Ok(false);
        };
        if !matches!(flag_base.as_ref(), Expression::Variable(name) if name == &alias.name)
            || constant_value(flag_value) != Some(0)
        {
            return Ok(false);
        }
        let (Ok(member_displacement), Ok(flag_displacement)) =
            (i16::try_from(*member_offset), i16::try_from(*flag_offset))
        else {
            return Ok(false);
        };

        let port_high = (port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = port as u16 as i16;
        self.output.pre_scheduled = true;
        self.evaluate(&Expression::Variable(global.clone()), global_type, 7)?;
        self.output.instructions.push(Instruction::ShiftLeftImmediate {
            a: 6,
            s: 4,
            shift: second.shift,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: command,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: member_displacement,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: port_high,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin: first_begin,
            end: first_end,
        });
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 0,
                s: 3,
                shift: first.shift,
                begin: first_insert_begin,
                end: first_insert_end,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 3,
            shift: 0,
            begin: second_begin,
            end: second_end,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 3, s: 3, b: 6 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 7,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 5,
            a: 4,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 7,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 7,
            offset: flag_displacement,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
