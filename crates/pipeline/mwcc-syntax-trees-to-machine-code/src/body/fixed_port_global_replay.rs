//! Replay one global state word to a fixed port and clear its sent flag.

#[allow(unused_imports)]
use super::*;

fn stripped(mut expression: &Expression) -> &Expression {
    while let Expression::Cast { operand, .. } = expression {
        expression = operand;
    }
    expression
}

impl Generator {
    pub(crate) fn try_fixed_port_global_replay(
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
        let [Statement::Loop {
            kind: LoopKind::DoWhile,
            condition: Some(condition),
            body,
            ..
        }, Statement::Store {
            target:
                Expression::Member {
                    base: flag_base,
                    offset: flag_offset,
                    member_type: Type::UnsignedShort,
                    index_stride: None,
                },
            value: flag_value,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Statement::Store {
            target: command_target,
            value: command,
        }, Statement::Store {
            target: data_target,
            value: data,
        }] = body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Variable(global) = flag_base.as_ref() else {
            return Ok(false);
        };
        let port_target = |target: &Expression, expected_type| {
            matches!(target, Expression::Member {
                base,
                offset: 0,
                member_type,
                index_stride: None,
            } if *member_type == expected_type
                && matches!(stripped(base), Expression::IntegerLiteral(value)
                    if *value as u32 == 0xcc00_8000))
        };
        let Expression::Member {
            base: data_base,
            offset: word_offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } = stripped(data)
        else {
            return Ok(false);
        };
        if constant_value(condition) != Some(0)
            || constant_value(command) != Some(0x61)
            || constant_value(flag_value) != Some(0)
            || !port_target(command_target, Type::UnsignedChar)
            || !port_target(data_target, Type::UnsignedInt)
            || !matches!(data_base.as_ref(), Expression::Variable(name) if name == global)
        {
            return Ok(false);
        }
        let (Ok(word_offset), Ok(flag_offset)) =
            (i16::try_from(*word_offset), i16::try_from(*flag_offset))
        else {
            return Ok(false);
        };
        let Some(&global_type) = self.globals.get(global.as_str()) else {
            return Ok(false);
        };

        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0x61));
        self.evaluate(&Expression::Variable(global.clone()), global_type, 4)?;
        self.output.instructions.extend([
            Instruction::load_immediate_shifted(5, 0xcc01u16 as i16),
            Instruction::StoreByte {
                s: 0,
                a: 5,
                offset: -32768,
            },
            Instruction::load_immediate(0, 0),
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: word_offset,
            },
            Instruction::StoreWord {
                s: 3,
                a: 5,
                offset: -32768,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 4,
                offset: flag_offset,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
