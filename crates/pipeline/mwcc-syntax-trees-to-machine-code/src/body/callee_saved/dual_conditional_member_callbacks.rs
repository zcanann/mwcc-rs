//! Two guarded callbacks after direct calls through a shared member alias.
//!
//! The entry object and its member-derived alias both survive the direct-call
//! prefix. MWCC keeps them in r30/r31, then reuses each callback load in r12 for
//! both the null check and the indirect call.

#[allow(unused_imports)]
use super::*;

struct DualConditionalMemberCallbacks<'a> {
    alias_offset: i16,
    direct_calls: [&'a str; 2],
    callback_offsets: [i16; 2],
}

impl Generator {
    pub(crate) fn try_dual_conditional_member_callbacks(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.general_register_of(&function.parameters[0].name)? != 3
        {
            return Ok(false);
        }

        const ALIAS: u8 = 31;
        const OBJECT: u8 = 30;
        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![ALIAS, OBJECT];
        self.output.pre_scheduled = true;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: ALIAS,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: OBJECT,
            a: 1,
            offset: 16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(OBJECT, 3));
        self.output.instructions.push(Instruction::LoadWord {
            d: ALIAS,
            a: 3,
            offset: shape.alias_offset,
        });
        for direct_call in shape.direct_calls {
            self.output
                .instructions
                .push(Instruction::move_register(3, ALIAS));
            self.record_relocation(RelocationKind::Rel24, direct_call);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: direct_call.to_string(),
            });
        }
        for callback_offset in shape.callback_offsets {
            self.output.instructions.push(Instruction::LoadWord {
                d: 12,
                a: ALIAS,
                offset: callback_offset,
            });
            self.output
                .instructions
                .push(Instruction::CompareLogicalWordImmediate {
                    a: 12,
                    immediate: 0,
                });
            let callback_done = self.fresh_label();
            self.emit_branch_conditional_to(12, 2, callback_done);
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: OBJECT,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
            self.bind_label(callback_done);
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: ALIAS,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: OBJECT,
            a: 1,
            offset: 16,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}

fn recognize(function: &Function) -> Option<DualConditionalMemberCallbacks<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [object] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(object.parameter_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let [alias] = function.locals.as_slice() else {
        return None;
    };
    if alias.is_static || alias.is_volatile || alias.array_length.is_some() {
        return None;
    }
    let Expression::Member {
        base: alias_base,
        offset: alias_offset,
        index_stride: None,
        ..
    } = alias.initializer.as_ref()?
    else {
        return None;
    };
    if !variable(alias_base, &object.name) {
        return None;
    }
    let [first_call, second_call, first_callback, second_callback] =
        function.statements.as_slice()
    else {
        return None;
    };
    let first_call = direct_alias_call(first_call, &alias.name)?;
    let second_call = direct_alias_call(second_call, &alias.name)?;
    let first_callback = guarded_callback(first_callback, &alias.name, &object.name)?;
    let second_callback = guarded_callback(second_callback, &alias.name, &object.name)?;
    Some(DualConditionalMemberCallbacks {
        alias_offset: i16::try_from(*alias_offset).ok()?,
        direct_calls: [first_call, second_call],
        callback_offsets: [
            i16::try_from(first_callback).ok()?,
            i16::try_from(second_callback).ok()?,
        ],
    })
}

fn direct_alias_call<'a>(statement: &'a Statement, alias: &str) -> Option<&'a str> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    matches!(arguments.as_slice(), [argument] if variable(argument, alias)).then_some(name)
}

fn guarded_callback(statement: &Statement, alias: &str, object: &str) -> Option<u32> {
    let Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            },
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if !matches!(right.as_ref(), Expression::IntegerLiteral(0)) || !else_body.is_empty() {
        return None;
    }
    let callback_offset = member_offset(left, alias)?;
    let [Statement::Expression(Expression::CallThrough { target, arguments })] =
        then_body.as_slice()
    else {
        return None;
    };
    (member_offset(target, alias) == Some(callback_offset)
        && matches!(arguments.as_slice(), [argument] if variable(argument, object)))
    .then_some(callback_offset)
}

fn member_offset(expression: &Expression, base: &str) -> Option<u32> {
    let Expression::Member {
        base: member_base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    variable(member_base, base).then_some(*offset)
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}
