//! Callee-saved dense switch dispatchers.
//!
//! This owner recognizes a semantic call-dispatch shape and delegates the table
//! mechanics to `switch.rs`. It owns only live-home assignment, trace-call
//! scheduling, and the enclosing frame.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_switch_call_dispatcher(&mut self, function: &Function) -> Compilation<bool> {
        if function.guards.len() != 0
            || function.return_type != Type::Int
            || self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.global_addressing != GlobalAddressing::Absolute
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(parameter.parameter_type, Type::StructPointer { .. } | Type::Pointer(_)) {
            return Ok(false);
        }
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            || local.initializer.is_some()
            || local.array_length.is_some()
            || local.is_static
            || !matches!(
                function.return_expression.as_ref(),
                Some(Expression::Variable(name)) if name == &local.name
            )
        {
            return Ok(false);
        }
        let [
            Statement::Assign {
                name: initialized,
                value: Expression::IntegerLiteral(initial_value),
            },
            Statement::Expression(Expression::Call {
                name: setup_callee,
                arguments: setup_arguments,
            }),
            Statement::Expression(Expression::Call {
                name: trace_callee,
                arguments: first_trace_arguments,
            }),
            Statement::Switch {
                scrutinee,
                arms,
                default: None,
            },
            Statement::Expression(Expression::Call {
                name: final_trace_callee,
                arguments: final_trace_arguments,
            }),
        ] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if initialized != &local.name
            || !(i16::MIN as i64..=i16::MAX as i64).contains(initial_value)
            || !matches!(setup_arguments.as_slice(), [Expression::Variable(name), Expression::IntegerLiteral(value)] if name == &parameter.name && (i16::MIN as i64..=i16::MAX as i64).contains(value))
            || trace_callee != final_trace_callee
        {
            return Ok(false);
        }
        let [
            Expression::IntegerLiteral(trace_level),
            Expression::StringLiteral(first_string),
            first_member,
        ] = first_trace_arguments.as_slice()
        else {
            return Ok(false);
        };
        let [
            Expression::IntegerLiteral(final_trace_level),
            Expression::StringLiteral(final_string),
            Expression::Variable(final_value),
        ] = final_trace_arguments.as_slice()
        else {
            return Ok(false);
        };
        let Some(member_offset) = byte_member_offset(first_member, &parameter.name) else {
            return Ok(false);
        };
        if byte_member_offset(scrutinee, &parameter.name) != Some(member_offset)
            || trace_level != final_trace_level
            || final_value != &local.name
            || !(i16::MIN as i64..=i16::MAX as i64).contains(trace_level)
            || first_string == final_string
        {
            return Ok(false);
        }
        let mut call_arms = Vec::with_capacity(arms.len());
        for arm in arms {
            let mwcc_syntax_trees::ArmBody::Statements(statements) = &arm.body else {
                return Ok(false);
            };
            let [Statement::Assign {
                name,
                value:
                    Expression::Call {
                        name: callee,
                        arguments,
                    },
            }] = statements.as_slice()
            else {
                return Ok(false);
            };
            if name != &local.name
                || arm.falls_through
                || !matches!(arguments.as_slice(), [Expression::Variable(name)] if name == &parameter.name)
            {
                return Ok(false);
            }
            call_arms.push((arm.value, callee.clone()));
        }
        if call_arms.len() < 7 {
            return Ok(false);
        }

        let Expression::IntegerLiteral(setup_value) = setup_arguments[1] else {
            unreachable!()
        };
        let first_string_index = self.intern_string_literal(first_string);
        let final_string_index = self.intern_string_literal(final_string);
        let final_string_offset: usize = self.output.string_literals
            [..final_string_index]
            .iter()
            .map(|bytes| bytes.len() + 1)
            .sum();
        if final_string_offset > i16::MAX as usize {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![31, 30];
        self.output.pre_scheduled = true;
        self.output.packed_string_literals = true;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::load_immediate(4, setup_value as i16));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(31, *initial_value as i16));
        self.output.instructions.push(Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 8,
        });
        self.output
            .instructions
            .push(Instruction::move_register(30, 3));
        self.emit_dispatcher_call(setup_callee);

        let first_placeholder = format!("@@str{first_string_index}");
        self.record_relocation(RelocationKind::Addr16Ha, &first_placeholder);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output.instructions.push(Instruction::LoadByteZero {
            d: 5,
            a: 30,
            offset: member_offset,
        });
        self.record_relocation(RelocationKind::Addr16Lo, &first_placeholder);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, *trace_level as i16));
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.emit_dispatcher_call(trace_callee);

        self.output.instructions.push(Instruction::LoadByteZero {
            d: 0,
            a: 30,
            offset: member_offset,
        });
        self.emit_assignment_call_jump_table(0, &call_arms, 30, 31)?;

        let final_placeholder = format!("@@str{final_string_index}");
        self.record_relocation(RelocationKind::Addr16Ha, &final_placeholder);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, 0));
        self.output
            .instructions
            .push(Instruction::move_register(5, 31));
        self.record_relocation(RelocationKind::Addr16Lo, &final_placeholder);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, *final_trace_level as i16));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: final_string_offset as i16,
        });
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.emit_dispatcher_call(final_trace_callee);
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn emit_dispatcher_call(&mut self, callee: &str) {
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.to_string(),
        });
    }
}

fn byte_member_offset(expression: &Expression, parameter: &str) -> Option<i16> {
    let Expression::Index { base, index } = expression else {
        return None;
    };
    let Expression::MemberAddress {
        base,
        offset,
        element: Pointee::UnsignedChar,
        ..
    } = base.as_ref()
    else {
        return None;
    };
    let Expression::Variable(name) = base.as_ref() else {
        return None;
    };
    let Expression::IntegerLiteral(index) = index.as_ref() else {
        return None;
    };
    if name != parameter {
        return None;
    }
    i64::from(*offset)
        .checked_add(*index)
        .and_then(|value| i16::try_from(value).ok())
}
