//! Conditional member refresh followed by an object callback.
//!
//! The object pointer survives both the optional direct call and the mandatory
//! indirect call in one callee-saved register. The compared member value doubles
//! as the direct call's second argument.

#[allow(unused_imports)]
use super::*;

struct ConditionalMemberCallback<'a> {
    compare_left: i16,
    compare_right: i16,
    callback: i16,
    refresh: &'a str,
}

impl Generator {
    pub(crate) fn try_conditional_member_callback(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };

        const OBJECT: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![OBJECT];
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
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: OBJECT,
            a: 1,
            offset: 12,
        });
        self.output
            .instructions
            .push(Instruction::move_register(OBJECT, 3));
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 3,
            offset: shape.compare_left,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.compare_right,
        });
        self.output
            .instructions
            .push(Instruction::CompareWord { a: 4, b: 0 });
        let callback = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, callback);
        self.record_relocation(RelocationKind::Rel24, shape.refresh);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.refresh.to_string(),
        });

        self.bind_label(callback);
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: OBJECT,
            offset: shape.callback,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, OBJECT));
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 20,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: OBJECT,
            a: 1,
            offset: 12,
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
}

fn recognize(function: &Function) -> Option<ConditionalMemberCallback<'_>> {
    if function.return_type != Type::Void
        || function.return_expression.is_some()
        || !function.guards.is_empty()
    {
        return None;
    }
    let [object, ..] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(object.parameter_type, Type::Pointer(_) | Type::StructPointer { .. }) {
        return None;
    }
    let [alias] = function.locals.as_slice() else {
        return None;
    };
    let Expression::Cast { operand, .. } = alias.initializer.as_ref()? else {
        return None;
    };
    if !matches!(operand.as_ref(), Expression::Variable(name) if name == &object.name) {
        return None;
    }
    let [Statement::If {
        condition:
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            },
        then_body,
        else_body,
    }, Statement::Expression(Expression::CallThrough {
        target,
        arguments: callback_arguments,
    })] = function.statements.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty() {
        return None;
    }
    let compare_left = member_of(left, &alias.name)?;
    let compare_right = member_of(right, &alias.name)?;
    let [Statement::Expression(Expression::Call {
        name: refresh,
        arguments: refresh_arguments,
    })] = then_body.as_slice()
    else {
        return None;
    };
    if !matches!(refresh_arguments.as_slice(), [Expression::Variable(first), second]
        if first == &object.name && member_of(second, &alias.name) == Some(compare_left))
        || !matches!(callback_arguments.as_slice(), [Expression::Variable(first)] if first == &object.name)
    {
        return None;
    }
    let callback = member_of(target, &alias.name)?;
    Some(ConditionalMemberCallback {
        compare_left: i16::try_from(compare_left).ok()?,
        compare_right: i16::try_from(compare_right).ok()?,
        callback: i16::try_from(callback).ok()?,
        refresh,
    })
}

fn member_of(expression: &Expression, base_name: &str) -> Option<u32> {
    let Expression::Member { base, offset, .. } = expression else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == base_name).then_some(*offset)
}
