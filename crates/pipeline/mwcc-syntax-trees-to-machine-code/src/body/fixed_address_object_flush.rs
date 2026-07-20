//! Command/data writes through an MWCC absolute-address aggregate followed by a state clear.

#[allow(unused_imports)]
use super::*;

fn constant_address_member(statement: &Statement) -> Option<(u32, Type, &Expression)> {
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
    /// Lower an SDK fixed-object command/data pair followed by a narrow state clear. MWCC hoists
    /// the state-global load between the command literal and fixed-base materialization only when
    /// the port came from its absolute-address declaration syntax; an explicit pointer cast uses a
    /// different schedule and deliberately does not enter this owner.
    pub(crate) fn try_fixed_address_object_flush(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        let [command_store, data_store, clear_store] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some((port, Type::UnsignedChar, command_value)) =
            constant_address_member(command_store)
        else {
            return Ok(false);
        };
        let Some(command) = constant_value(command_value).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some((data_port, Type::UnsignedInt, data_value)) = constant_address_member(data_store)
        else {
            return Ok(false);
        };
        if data_port != port
            || !self
                .fixed_address_objects
                .values()
                .any(|&address| address == port)
        {
            return Ok(false);
        }
        let Expression::Member {
            base: data_base,
            offset: data_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } = data_value
        else {
            return Ok(false);
        };
        let Expression::Variable(global) = data_base.as_ref() else {
            return Ok(false);
        };
        let Some(&global_type) = self.globals.get(global.as_str()) else {
            return Ok(false);
        };
        let Statement::Store {
            target:
                Expression::Member {
                    base: clear_base,
                    offset: clear_offset,
                    member_type: Type::UnsignedShort,
                    index_stride: None,
                },
            value: clear_value,
        } = clear_store
        else {
            return Ok(false);
        };
        if !matches!(clear_base.as_ref(), Expression::Variable(name) if name == global)
            || constant_value(clear_value) != Some(0)
        {
            return Ok(false);
        }
        let (Ok(data_displacement), Ok(clear_displacement)) =
            (i16::try_from(*data_offset), i16::try_from(*clear_offset))
        else {
            return Ok(false);
        };
        let port_high = (port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = port as u16 as i16;

        self.output.pre_scheduled = true;
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: command,
        });
        self.evaluate(&Expression::Variable(global.clone()), global_type, 4)?;
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: port_high,
            });
        self.output.instructions.push(Instruction::StoreByte {
            s: 0,
            a: 5,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: data_displacement,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 5,
            offset: port_low,
        });
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 4,
            offset: clear_displacement,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
