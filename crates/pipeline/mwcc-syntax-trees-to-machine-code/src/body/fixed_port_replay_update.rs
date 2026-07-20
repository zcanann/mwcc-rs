//! One state-field update followed by two fixed-port command/data pairs.

#[allow(unused_imports)]
use super::*;

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
    /// Lower `state->word = (state->word & M) | (byte << S)`, then emit
    /// `(command, constant)` and `(command, state->word)` to the same fixed port. Build 163 forms
    /// the narrow insert first, retains the state base in r6, and materializes the high-word
    /// constant only after storing the updated state member.
    pub(crate) fn try_fixed_port_replay_update(
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
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::UnsignedChar {
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

        let [update, command_a, constant_store, command_b, state_store] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: update_base,
                    offset: member_offset,
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                },
            value:
                Expression::Binary {
                    operator: BinaryOperator::BitOr,
                    left: preserved,
                    right: inserted,
                },
        } = update
        else {
            return Ok(false);
        };
        if !matches!(update_base.as_ref(), Expression::Variable(name) if name == &alias.name) {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: old_value,
            right: preserve_mask,
        } = preserved.as_ref()
        else {
            return Ok(false);
        };
        let Some(preserve_mask) = constant_value(preserve_mask) else {
            return Ok(false);
        };
        let Some((preserve_begin, preserve_end)) = rlwinm_mask(preserve_mask) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: insert_value,
            right: insert_shift,
        } = inserted.as_ref()
        else {
            return Ok(false);
        };
        let Some(insert_shift) = constant_value(insert_shift).and_then(|value| u8::try_from(value).ok())
        else {
            return Ok(false);
        };
        if insert_shift > 24
            || !matches!(old_value.as_ref(), Expression::Member { base, offset, member_type: Type::UnsignedInt, index_stride: None }
                if offset == member_offset
                    && matches!(base.as_ref(), Expression::Variable(name) if name == &alias.name))
            || !matches!(insert_value.as_ref(), Expression::Cast { operand, .. }
                if matches!(operand.as_ref(), Expression::Variable(name) if name == &parameter.name))
        {
            return Ok(false);
        }

        let Some((port, Type::UnsignedChar, command_a_value)) = fixed_port_store(command_a) else {
            return Ok(false);
        };
        let Some(command) = constant_value(command_a_value).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some((constant_port, Type::UnsignedInt, constant_value_expression)) =
            fixed_port_store(constant_store)
        else {
            return Ok(false);
        };
        let Some(replayed_constant) =
            constant_value(constant_value_expression).and_then(|value| u32::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some((command_b_port, Type::UnsignedChar, command_b_value)) =
            fixed_port_store(command_b)
        else {
            return Ok(false);
        };
        let Some((state_port, Type::UnsignedInt, state_value)) = fixed_port_store(state_store) else {
            return Ok(false);
        };
        if constant_port != port
            || command_b_port != port
            || state_port != port
            || constant_value(command_b_value) != Some(i64::from(command))
            || replayed_constant & 0xffff != 0
            || !matches!(state_value, Expression::Member { base, offset, member_type: Type::UnsignedInt, index_stride: None }
                if offset == member_offset
                    && matches!(base.as_ref(), Expression::Variable(name) if name == &alias.name))
        {
            return Ok(false);
        }
        let Ok(member_displacement) = i16::try_from(*member_offset) else {
            return Ok(false);
        };

        let port_high = (port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = port as u16 as i16;
        self.output.pre_scheduled = true;
        self.evaluate(&Expression::Variable(global.clone()), global_type, 6)?;
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 3,
            shift: insert_shift,
            begin: 24 - insert_shift,
            end: 31 - insert_shift,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: command,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 6,
            offset: member_displacement,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: port_high,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 5,
            shift: 0,
            begin: preserve_begin,
            end: preserve_end,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 6,
            offset: member_displacement,
        });
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: (replayed_constant >> 16) as u16 as i16,
            });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 3,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::StoreByte {
            s: 4,
            a: 3,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 6,
            offset: member_displacement,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: port_low,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
