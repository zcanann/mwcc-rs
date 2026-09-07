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

/// A terminal command/data pair with an optional dirty-word OR and a narrow clear.
/// The plan borrows only the syntax tree so selection can finish before emission.
pub(crate) struct ObjectFlush<'a> {
    global: &'a str,
    global_type: Type,
    command: i16,
    port: u32,
    data_offset: i16,
    dirty: Option<(i16, u16)>,
    clear_offset: i16,
    pub(crate) statement_count: usize,
}

fn word_member(mut expression: &Expression) -> Option<(&str, i16)> {
    // SDK write macros retain word casts; a narrow cast still needs conversion.
    while let Expression::Cast {
        target_type: Type::Int | Type::UnsignedInt,
        operand,
    } = expression {
        expression = operand;
    }
    let Expression::Member {
        base,
        offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, i16::try_from(*offset).ok()?))
}

fn dirty_update(statement: &Statement, global: &str) -> Option<(i16, u16)> {
    let Statement::Store { target, value } = statement else {
        return None;
    };
    let (target_global, offset) = word_member(target)?;
    let value = match value {
        Expression::IndexedUpdateValue { value } => value.as_ref(),
        value => value,
    };
    let Expression::Binary {
        operator: BinaryOperator::BitOr,
        left,
        right,
    } = value
    else {
        return None;
    };
    if target_global != global || word_member(left)? != (global, offset) {
        return None;
    }
    Some((offset, u16::try_from(constant_value(right)?).ok()?))
}

impl Generator {
    /// Match only terminal stores; preceding structured control flow retains its
    /// own lowering. Absolute-address declaration provenance selects this
    /// schedule, while an ordinary pointer cast keeps general store emission.
    pub(crate) fn fixed_address_object_flush_tail<'a>(
        &self,
        statements: &'a [Statement],
    ) -> Option<ObjectFlush<'a>> {
        // Prefer the longer suffix so a dirty-word update cannot be discarded.
        for count in [4, 3] {
            let Some(start) = statements.len().checked_sub(count) else {
                continue;
            };
            if let Some(plan) = self.fixed_address_object_flush_plan(&statements[start..]) {
                return Some(plan);
            }
        }
        None
    }

    fn fixed_address_object_flush_plan<'a>(
        &self,
        statements: &'a [Statement],
    ) -> Option<ObjectFlush<'a>> {
        let (command_store, data_store, dirty_store, clear_store) = match statements {
            [command, data, dirty, clear] => (command, data, Some(dirty), clear),
            [command, data, clear] => (command, data, None, clear),
            _ => return None,
        };
        let (port, Type::UnsignedChar, command_value) = constant_address_member(command_store)?
        else {
            return None;
        };
        let command = i16::try_from(constant_value(command_value)?).ok()?;
        let (data_port, Type::UnsignedInt, data_value) = constant_address_member(data_store)?
        else {
            return None;
        };
        if data_port != port
            || !self
                .fixed_address_objects
                .values()
                .any(|&address| address == port)
        {
            return None;
        }
        let (global, data_offset) = word_member(data_value)?;
        let &global_type = self.globals.get(global)?;
        // Reusing the pointer would suppress required volatile reads. A local
        // with the same name must also keep ordinary expression resolution.
        if !matches!(global_type, Type::Pointer(_) | Type::StructPointer { .. })
            || self.volatile_globals.contains(global)
            || self.locations.contains_key(global)
        {
            return None;
        }
        let Statement::Store {
            target:
                Expression::Member {
                    base,
                    offset,
                    member_type: Type::UnsignedShort,
                    index_stride: None,
                },
            value,
        } = clear_store
        else {
            return None;
        };
        if !matches!(base.as_ref(), Expression::Variable(name) if name == global)
            || constant_value(value) != Some(0)
        {
            return None;
        }
        let dirty = match dirty_store {
            Some(statement) => Some(dirty_update(statement, global)?),
            None => None,
        };
        Some(ObjectFlush {
            global,
            global_type,
            command,
            port,
            data_offset,
            dirty,
            clear_offset: i16::try_from(*offset).ok()?,
            statement_count: statements.len(),
        })
    }

    pub(crate) fn try_fixed_address_object_flush(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || function.return_type != Type::Void
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let Some(plan) = self.fixed_address_object_flush_tail(&function.statements) else {
            return Ok(false);
        };
        if plan.statement_count != function.statements.len() {
            return Ok(false);
        }
        self.emit_fixed_address_object_flush(plan)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// Emit no terminator: both a whole-body owner and a structured leaf join
    /// can share this schedule and supply their own return handling.
    pub(crate) fn emit_fixed_address_object_flush(
        &mut self,
        plan: ObjectFlush<'_>,
    ) -> Compilation<()> {
        let port_high = (plan.port.wrapping_add(0x8000) >> 16) as u16 as i16;
        let port_low = plan.port as u16 as i16;
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::load_immediate(0, plan.command));
        self.evaluate(
            &Expression::Variable(plan.global.into()),
            plan.global_type,
            4,
        )?;
        self.output.instructions.extend([
            Instruction::load_immediate_shifted(5, port_high),
            Instruction::StoreByte {
                s: 0,
                a: 5,
                offset: port_low,
            },
            Instruction::load_immediate(0, 0),
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: plan.data_offset,
            },
            Instruction::StoreWord {
                s: 3,
                a: 5,
                offset: port_low,
            },
        ]);
        if let Some((offset, mask)) = plan.dirty {
            self.output.instructions.extend([
                Instruction::LoadWord { d: 3, a: 4, offset },
                Instruction::OrImmediate {
                    a: 3,
                    s: 3,
                    immediate: mask,
                },
                Instruction::StoreWord { s: 3, a: 4, offset },
            ]);
        }
        self.output.instructions.push(Instruction::StoreHalfword {
            s: 0,
            a: 4,
            offset: plan.clear_offset,
        });
        Ok(())
    }
}
