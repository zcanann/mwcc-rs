//! Reuse one structure member across two computed call arguments.
//!
//! DVD-style reads commonly pass `address + transferred` and
//! `offset + transferred` together. Build 163 loads `transferred` once into a
//! register shared by both additions and fills the surrounding latency slots
//! with the other independent member loads.

use super::*;

struct RepeatedMemberAddArguments<'a> {
    base: &'a str,
    first: &'a Expression,
    middle: &'a Expression,
    third: &'a Expression,
    repeated: &'a Expression,
    callback: &'a str,
}

fn direct_member_base(expression: &Expression) -> Option<&str> {
    if let Expression::Cast {
        target_type,
        operand,
    } = expression
    {
        if target_type.width() != 32 {
            return None;
        }
        return direct_member_base(operand);
    }
    let Expression::Member {
        base,
        member_type,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    if member_type.width() != 32 || matches!(member_type, Type::Float) {
        return None;
    }
    let Expression::Variable(name) = base.as_ref() else {
        return None;
    };
    Some(name)
}

fn is_pointer_member_value(expression: &Expression) -> bool {
    match expression {
        Expression::Member {
            member_type: Type::Pointer(_) | Type::StructPointer { .. },
            ..
        } => true,
        Expression::Cast {
            target_type: Type::Pointer(_) | Type::StructPointer { .. },
            operand,
        } => is_pointer_member_value(operand),
        _ => false,
    }
}

fn add_operands(expression: &Expression) -> Option<(&Expression, &Expression)> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression
    else {
        return None;
    };
    Some((left, right))
}

fn recognize(arguments: &[Expression]) -> Option<RepeatedMemberAddArguments<'_>> {
    let [first_sum, middle, third_sum, Expression::Variable(callback)] = arguments else {
        return None;
    };
    let (first_left, first_right) = add_operands(first_sum)?;
    let (third_left, third_right) = add_operands(third_sum)?;
    let (first, repeated, third) = if structurally_equal(first_right, third_right) {
        (first_left, first_right, third_left)
    } else if structurally_equal(first_right, third_left) {
        (first_left, first_right, third_right)
    } else if structurally_equal(first_left, third_right) {
        (first_right, first_left, third_left)
    } else if structurally_equal(first_left, third_left) {
        (first_right, first_left, third_right)
    } else {
        return None;
    };
    let base = direct_member_base(repeated)?;
    if !is_pointer_member_value(first)
        || [first, middle, third]
            .iter()
            .any(|expression| direct_member_base(expression) != Some(base))
    {
        return None;
    }
    Some(RepeatedMemberAddArguments {
        base,
        first,
        middle,
        third,
        repeated,
        callback,
    })
}

impl Generator {
    pub(crate) fn try_emit_repeated_member_add_arguments(
        &mut self,
        arguments: &[Expression],
        direct_call: bool,
    ) -> Compilation<bool> {
        let Some(plan) = recognize(arguments) else {
            return Ok(false);
        };
        if !direct_call
            || !self.behavior.schedule_latency_slots
            || !self.call_return_types.contains_key(plan.callback)
            || self.globals.contains_key(plan.callback)
            || self.locations.contains_key(plan.callback)
        {
            return Ok(false);
        }
        let base = self.general_register_of(plan.base)?;
        let first_argument = Eabi::FIRST_GENERAL_ARGUMENT;
        let callback_argument = first_argument + 3;
        let base_is_first_argument = base == first_argument;
        if !base_is_first_argument {
            self.avoid_virtual_general(
                base,
                &[
                    first_argument,
                    first_argument + 1,
                    first_argument + 2,
                    callback_argument,
                ],
            );
        }

        let callback_high = if base_is_first_argument {
            first_argument + 1
        } else {
            first_argument
        };
        self.emit_address_high(callback_high, plan.callback);
        self.record_relocation(RelocationKind::Addr16Lo, plan.callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: callback_argument,
            a: callback_high,
            immediate: 0,
        });

        let repeated = if base_is_first_argument {
            first_argument + 4
        } else {
            first_argument + 2
        };
        let first = if base_is_first_argument {
            first_argument + 2
        } else {
            first_argument
        };
        self.evaluate_general(plan.repeated, repeated)?;
        self.evaluate_general(plan.first, first)?;
        self.evaluate_general(plan.third, GENERAL_SCRATCH)?;
        if base_is_first_argument {
            self.evaluate_general(plan.middle, first_argument + 1)?;
        }
        self.output.instructions.push(Instruction::Add {
            d: first_argument,
            a: first,
            b: repeated,
        });
        if !base_is_first_argument {
            self.evaluate_general(plan.middle, first_argument + 1)?;
        }
        self.output.instructions.push(Instruction::Add {
            d: first_argument + 2,
            a: GENERAL_SCRATCH,
            b: repeated,
        });
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("transfer".into())),
            offset,
            member_type,
            index_stride: None,
        }
    }

    fn add(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn recognizes_one_member_shared_by_two_add_arguments() {
        let transferred = member(4, Type::UnsignedInt);
        let arguments = vec![
            add(
                member(0, Type::Pointer(mwcc_syntax_trees::Pointee::Char)),
                transferred.clone(),
            ),
            member(8, Type::UnsignedInt),
            add(member(12, Type::UnsignedInt), transferred),
            Expression::Variable("complete".into()),
        ];

        let plan = recognize(&arguments).expect("repeated transferred member");
        assert_eq!(plan.base, "transfer");
        assert_eq!(plan.callback, "complete");
        assert!(structurally_equal(
            plan.repeated,
            &member(4, Type::UnsignedInt)
        ));
    }
}
