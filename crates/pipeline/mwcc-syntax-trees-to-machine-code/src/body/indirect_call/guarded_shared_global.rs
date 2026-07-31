//! Guarded member callbacks whose owner is also their sole argument.
//!
//! `if (current->callback) current->callback(current)` evaluates the global
//! pointer and callback once. The pointer remains in r3 while r12 carries the
//! tested callback down the true edge.

use super::*;

impl Generator {
    pub(crate) fn try_emit_guarded_shared_global_member_call(
        &mut self,
        condition: &Expression,
        then_body: &[Statement],
    ) -> Compilation<bool> {
        let Some((global, offset)) = recognize(condition, then_body, &self.globals) else {
            return Ok(false);
        };

        let switch_base = self
            .structured_shared_switch_global_value
            .as_ref()
            .filter(|(name, _)| name == global)
            .map(|(_, register)| *register);
        let base = if let Some(base) = self.condition_global_base(global)? {
            base
        } else if let Some(base) = switch_base {
            base
        } else {
            self.emit_global_load_value(global, Eabi::FIRST_GENERAL_ARGUMENT)?;
            Eabi::FIRST_GENERAL_ARGUMENT
        };
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: base,
            offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            });
        let done = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, done);
        if base != Eabi::FIRST_GENERAL_ARGUMENT {
            self.emit_integer_materialization_copy(
                Eabi::FIRST_GENERAL_ARGUMENT,
                base,
            );
        }
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.const_address_bases.clear();
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
        } else {
            self.emit_indirect_branch_and_link(12);
        }
        self.bind_label(done);
        Ok(true)
    }
}

fn recognize<'a>(
    condition: &'a Expression,
    then_body: &'a [Statement],
    globals: &std::collections::HashMap<String, Type>,
) -> Option<(&'a str, i16)> {
    let guarded_member = match condition {
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if matches!(right.as_ref(), Expression::IntegerLiteral(0)) => left.as_ref(),
        Expression::Member { .. } => condition,
        _ => return None,
    };
    let [Statement::Expression(Expression::CallThrough { target, arguments })] = then_body else {
        return None;
    };
    let (condition_base, condition_offset) = member(guarded_member)?;
    let (call_base, call_offset) = member(target)?;
    if condition_base != call_base
        || condition_offset != call_offset
        || !matches!(
            arguments.as_slice(),
            [Expression::Variable(argument)] if argument == condition_base
        )
        || !matches!(
            globals.get(condition_base),
            Some(Type::StructPointer { .. })
        )
    {
        return None;
    }
    Some((condition_base, condition_offset))
}

fn member(expression: &Expression) -> Option<(&str, i16)> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Pointer(_) | Type::StructPointer { .. },
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some((base, i16::try_from(*offset).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn callback(global: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(global.into())),
            offset,
            member_type: Type::Pointer(Pointee::UnsignedInt),
            index_stride: None,
        }
    }

    #[test]
    fn recognizes_a_guarded_callback_carried_with_its_global_owner() {
        let condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(callback("current", 40)),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let body = [Statement::Expression(Expression::CallThrough {
            target: Box::new(callback("current", 40)),
            arguments: vec![Expression::Variable("current".into())],
        })];
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        assert_eq!(
            recognize(&condition, &body, &globals),
            Some(("current", 40))
        );
    }

    #[test]
    fn rejects_a_call_through_a_different_member() {
        let condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(callback("current", 40)),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let body = [Statement::Expression(Expression::CallThrough {
            target: Box::new(callback("current", 44)),
            arguments: vec![Expression::Variable("current".into())],
        })];
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        assert_eq!(recognize(&condition, &body, &globals), None);
    }

    #[test]
    fn recognizes_a_bare_member_truth_test() {
        let condition = callback("current", 40);
        let body = [Statement::Expression(Expression::CallThrough {
            target: Box::new(callback("current", 40)),
            arguments: vec![Expression::Variable("current".into())],
        })];
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        assert_eq!(
            recognize(&condition, &body, &globals),
            Some(("current", 40))
        );
    }
}
