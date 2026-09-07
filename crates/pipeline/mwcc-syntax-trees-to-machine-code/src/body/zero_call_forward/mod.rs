//! Framed forwarding calls with one passthrough and trailing zero arguments.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_zero_call_forward(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_expression.is_some()
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::Int
            || self
                .locations
                .get(&parameter.name)
                .map(|location| location.register)
                != Some(3)
        {
            return Ok(false);
        }
        let statements = match function.statements.as_slice() {
            [Statement::Expression(Expression::Cast {
                target_type: Type::Void,
                operand,
            }), rest @ ..]
                if constant_value(operand) == Some(0) =>
            {
                rest
            }
            statements => statements,
        };
        let [Statement::Expression(Expression::Call {
            name: callee,
            arguments,
        })] = statements
        else {
            return Ok(false);
        };
        let [first, rest @ ..] = arguments.as_slice() else {
            return Ok(false);
        };
        if arguments.len() != 10
            || !matches!(first, Expression::Variable(name) if name == &parameter.name)
            || rest
                .iter()
                .any(|argument| constant_value(argument) != Some(0))
        {
            return Ok(false);
        }
        // This schedule owns ten word-class argument positions, not floating,
        // wide, reference, or aggregate marshaling, nor an indirect call.
        if self.locations.contains_key(callee)
            || self.globals.contains_key(callee)
            || self.call_parameter_types.get(callee).is_some_and(|types| {
                types.len() != arguments.len()
                    || types.first().is_some_and(|ty| ty.width() < 32)
                    || types.iter().any(|ty| {
                        !matches!(
                            ty,
                            Type::Char
                                | Type::UnsignedChar
                                | Type::Short
                                | Type::UnsignedShort
                                | Type::Int
                                | Type::UnsignedInt
                                | Type::Pointer(_)
                                | Type::StructPointer { .. }
                        )
                    })
            })
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved.clear();
        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::load_immediate(4, 0),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::load_immediate(0, 0),
            Instruction::load_immediate(5, 0),
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::load_immediate(6, 0),
            Instruction::load_immediate(7, 0),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::load_immediate(8, 0),
            Instruction::load_immediate(9, 0),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::load_immediate(10, 0),
        ]);
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.clone(),
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
