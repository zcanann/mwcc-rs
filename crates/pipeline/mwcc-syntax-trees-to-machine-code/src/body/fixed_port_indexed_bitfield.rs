//! Indexed state-word bitfield updates followed by fixed-port command/data writes.

#[allow(unused_imports)]
use super::*;

struct IndexedUpdate<'a> {
    parameter: &'a str,
    preserve_mask: u32,
    shift: u8,
}

fn indexed_access<'a>(expression: &'a Expression, alias: &str) -> Option<(u32, &'a str)> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::MemberAddress {
        base,
        offset,
        element: Pointee::UnsignedInt,
        ..
    } = base.as_ref()
    else {
        return None;
    };
    let Expression::Variable(index) = index.as_ref() else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == alias)
        .then_some((*offset, index.as_str()))
}

fn recognize_update<'a>(
    statement: &'a Statement,
    alias: &str,
    index_name: &str,
    member_offset: u32,
) -> Option<IndexedUpdate<'a>> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (target_offset, target_index) = indexed_access(target, alias)?;
    if target_offset != member_offset || target_index != index_name {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left: old_value,
        right: preserve,
    } = left.as_ref()
    else {
        return None;
    };
    let (old_offset, old_index) = indexed_access(old_value, alias)?;
    if old_offset != member_offset || old_index != index_name {
        return None;
    }
    let preserve_mask = constant_value(preserve)? as u32;
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
    (shift <= 24).then_some(IndexedUpdate {
        parameter,
        preserve_mask,
        shift,
    })
}

fn fixed_port_store(statement: &Statement) -> Option<(u32, Type, &Expression)> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset: 0,
                member_type,
                index_stride: None,
            },
        value,
    } = statement
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
    /// Lower two masked inserts into `state->words[index]`, followed by a fixed-port command and
    /// the updated word. Build 163 keeps two copies of the indexed address in r7/r8 so each update
    /// can retain its own load/store chain while the global base remains available for the flag.
    pub(crate) fn try_fixed_port_indexed_bitfield_update(
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
        let [index_parameter, first_parameter, second_parameter] = function.parameters.as_slice()
        else {
            return Ok(false);
        };
        if index_parameter.parameter_type != Type::Int
            || first_parameter.parameter_type != Type::UnsignedChar
            || second_parameter.parameter_type != Type::UnsignedChar
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
        let Statement::Store { target, .. } = first_statement else {
            return Ok(false);
        };
        let Some((member_offset, target_index)) = indexed_access(target, &alias.name) else {
            return Ok(false);
        };
        if target_index != index_parameter.name {
            return Ok(false);
        }
        let (Some(first), Some(second)) = (
            recognize_update(
                first_statement,
                &alias.name,
                &index_parameter.name,
                member_offset,
            ),
            recognize_update(
                second_statement,
                &alias.name,
                &index_parameter.name,
                member_offset,
            ),
        ) else {
            return Ok(false);
        };
        if first.parameter != first_parameter.name || second.parameter != second_parameter.name {
            return Ok(false);
        }
        let Some((first_begin, first_end)) = rlwinm_mask(first.preserve_mask as i64) else {
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
        let Some((data_offset, data_index)) = indexed_access(data_value, &alias.name) else {
            return Ok(false);
        };
        if data_port != port || data_offset != member_offset || data_index != index_parameter.name {
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
            (i16::try_from(member_offset), i16::try_from(*flag_offset))
        else {
            return Ok(false);
        };

        let port_high = (port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = port as u16 as i16;
        self.output.pre_scheduled = true;
        self.evaluate(&Expression::Variable(global.clone()), global_type, 6)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 3, s: 3, shift: 2 });
        self.output
            .instructions
            .push(Instruction::Add { d: 7, a: 6, b: 3 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 7,
            offset: member_displacement,
        });
        self.output
            .instructions
            .push(Instruction::Add { d: 8, a: 6, b: 3 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 0,
            shift: 0,
            begin: first_begin,
            end: first_end,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 4,
            shift: first.shift,
            begin: 24 - first.shift,
            end: 31 - first.shift,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 3, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 7,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 5,
            shift: second.shift,
            begin: 24 - second.shift,
            end: 31 - second.shift,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: command,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 8,
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
            a: 5,
            s: 5,
            shift: 0,
            begin: second_begin,
            end: second_end,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 8,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 3,
            a: 4,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 8,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 4,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 6,
            offset: flag_displacement,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
