//! Two-stream byte comparison loops with one call per stream.
//!
//! MWCC strength-reduces `left[i]` and `right[i]` into two loop-carried
//! pointers, keeps the first call result across the second call, and uses one
//! contiguous five-GPR `stmw`/`lmw` save range when that convention is enabled.

#[allow(unused_imports)]
use super::*;

struct DualIndexCallCompareLoop<'a> {
    callee: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn assigned(expression: &Expression, name: &str, value: i64) -> bool {
    matches!(
        expression,
        Expression::Assign { target, value: assigned }
            if variable(target, name) && constant_value(assigned) == Some(value)
    )
}

fn increment(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Assign { target, value }
            if variable(target, name)
                && matches!(
                    value.as_ref(),
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if variable(left, name) && constant_value(right) == Some(1)
                )
    )
}

fn pointer_alias<'a>(local: &'a LocalDeclaration, parameter: &str) -> Option<&'a str> {
    if !matches!(
        local.declared_type,
        Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
    ) || local.array_length.is_some()
        || local.is_static
        || local.is_volatile
    {
        return None;
    }
    let mut initializer = local.initializer.as_ref()?;
    while let Expression::Cast { operand, .. } = initializer {
        initializer = operand;
    }
    variable(initializer, parameter).then_some(local.name.as_str())
}

fn indexed_call<'a>(
    statement: &'a Statement,
    base: &str,
    index: &str,
) -> Option<(&'a str, &'a str)> {
    let Statement::Assign {
        name,
        value: Expression::Call {
            name: callee,
            arguments,
        },
    } = statement
    else {
        return None;
    };
    let [Expression::Index {
        base: actual_base,
        index: actual_index,
    }] = arguments.as_slice()
    else {
        return None;
    };
    (variable(actual_base, base) && variable(actual_index, index))
        .then_some((name.as_str(), callee.as_str()))
}

fn recognize(function: &Function) -> Option<DualIndexCallCompareLoop<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 3
        || function.locals.len() != 5
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [left_parameter, right_parameter, bound_parameter] = function.parameters.as_slice() else {
        return None;
    };
    if !matches!(
        left_parameter.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(
        right_parameter.parameter_type,
        Type::Pointer(_) | Type::StructPointer { .. }
    ) || !matches!(
        bound_parameter.parameter_type,
        Type::Int | Type::UnsignedInt
    ) {
        return None;
    }
    let [left_local, right_local, index_local, first_value_local, second_value_local] =
        function.locals.as_slice()
    else {
        return None;
    };
    let left = pointer_alias(left_local, &left_parameter.name)?;
    let right = pointer_alias(right_local, &right_parameter.name)?;
    if index_local.initializer.is_some()
        || first_value_local.initializer.is_some()
        || second_value_local.initializer.is_some()
        || !matches!(index_local.declared_type, Type::Int | Type::UnsignedInt)
        || first_value_local.declared_type != Type::Int
        || second_value_local.declared_type != Type::Int
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !assigned(initializer, &index_local.name, 0)
        || !matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } if variable(left, &index_local.name)
                && variable(right, &bound_parameter.name)
        )
        || !increment(step, &index_local.name)
    {
        return None;
    }
    let [first_call, second_call, Statement::If {
        condition: mismatch,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    let (first_value, first_callee) = indexed_call(first_call, left, &index_local.name)?;
    let (second_value, second_callee) = indexed_call(second_call, right, &index_local.name)?;
    let [Statement::Return(Some(Expression::Binary {
        operator: BinaryOperator::Subtract,
        left: returned_left,
        right: returned_right,
    }))] = then_body.as_slice()
    else {
        return None;
    };
    if first_value != first_value_local.name
        || second_value != second_value_local.name
        || first_callee != second_callee
        || !else_body.is_empty()
        || !matches!(
            mismatch,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if variable(left, first_value)
                && variable(right, second_value)
        )
        || !variable(returned_left, first_value)
        || !variable(returned_right, second_value)
    {
        return None;
    }
    Some(DualIndexCallCompareLoop {
        callee: first_callee,
    })
}

impl Generator {
    pub(crate) fn try_dual_index_call_compare_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.behavior.use_lmw_stmw
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let left = self.fresh_virtual_general_preferring(30);
        let right = self.fresh_virtual_general_preferring(31);
        let bound = self.fresh_virtual_general_preferring(27);
        let index = self.fresh_virtual_general_preferring(28);
        let first_value = self.fresh_virtual_general_preferring(29);

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![bound, index, first_value, left, right];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::StoreMultipleWord {
                s: bound,
                a: 1,
                offset: 12,
            },
            Instruction::move_register(bound, 5),
            Instruction::move_register(left, 3),
            Instruction::move_register(right, 4),
            Instruction::load_immediate(index, 0),
            Instruction::Branch { target: 21 },
            Instruction::LoadByteZero {
                d: 3,
                a: left,
                offset: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.extend([
            Instruction::BranchAndLink {
                target: shape.callee.to_string(),
            },
            Instruction::move_register(first_value, 3),
            Instruction::LoadByteZero {
                d: 3,
                a: right,
                offset: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.extend([
            Instruction::BranchAndLink {
                target: shape.callee.to_string(),
            },
            Instruction::CompareWord {
                a: first_value,
                b: 3,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 18,
            },
            Instruction::SubtractFrom {
                d: 3,
                a: 3,
                b: first_value,
            },
            Instruction::Branch { target: 24 },
            Instruction::AddImmediate {
                d: index,
                a: index,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: right,
                a: right,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: left,
                a: left,
                immediate: 1,
            },
            Instruction::CompareLogicalWord { a: index, b: bound },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 9,
            },
            Instruction::load_immediate(3, 0),
            Instruction::LoadMultipleWord {
                d: bound,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::BranchToLinkRegister,
        ]);
        let lowered = super::structured_loop_lowering::lower_structured_loops(
            function,
            &self.global_array_sizes,
        );
        self.output.anonymous_label_bump += super::structured::structured_hidden_label_count(
            &lowered
                .as_ref()
                .map_or(function.statements.as_slice(), |lowered| {
                    lowered.statements.as_slice()
                }),
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_requires_an_in_place_add_one() {
        let expression = Expression::Assign {
            target: Box::new(Expression::Variable("i".into())),
            value: Box::new(Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        };
        assert!(increment(&expression, "i"));
        assert!(!increment(&expression, "j"));
    }
}
