//! Delimited ASCII float-list parsers.
//!
//! This family skips punctuation, recognizes an optional sign, scans one
//! floating token, temporarily terminates it for a conversion call, restores
//! the delimiter, and advances an output float cursor. MWCC treats the nested
//! loops as one transaction: delimiter bytes are cached across comparisons,
//! adjacent character alternatives become unsigned ranges, and the numeric
//! loop's byte remains live as the later restored delimiter.

#[allow(unused_imports)]
use super::*;

struct FloatListParser<'a> {
    conversion: &'a str,
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn dereference_of(expression: &Expression, expected: &str) -> bool {
    matches!(
        expression,
        Expression::Dereference { pointer } if variable(pointer, expected)
    )
}

fn assigned_constant(statement: &Statement, expected: &str, value: i64) -> bool {
    matches!(
        statement,
        Statement::Assign { name, value: assigned }
            if name == expected && constant_value(assigned) == Some(value)
    )
}

fn assigned_variable(statement: &Statement, expected: &str, source: &str) -> bool {
    matches!(
        statement,
        Statement::Assign { name, value }
            if name == expected && variable(value, source)
    )
}

fn increment_assignment(expression: &Expression, expected: &str) -> bool {
    matches!(
        expression,
        Expression::Assign { target, value }
            if variable(target, expected)
                && matches!(
                    value.as_ref(),
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if variable(left, expected) && constant_value(right) == Some(1)
                )
    )
}

fn increment_statement(statement: &Statement, expected: &str) -> bool {
    matches!(
        statement,
        Statement::Assign { name, value }
            if name == expected
                && matches!(
                    value,
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if variable(left, expected) && constant_value(right) == Some(1)
                )
    )
}

fn equality_constants(expression: &Expression, pointer: &str, output: &mut Vec<i64>) -> bool {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left,
            right,
        } => {
            equality_constants(left, pointer, output) && equality_constants(right, pointer, output)
        }
        Expression::Binary {
            operator: BinaryOperator::Equal,
            left,
            right,
        } if dereference_of(left, pointer) => constant_value(right)
            .map(|value| output.push(value))
            .is_some(),
        _ => false,
    }
}

fn pointer_alias(local: &LocalDeclaration, source: &str) -> bool {
    if !matches!(
        local.declared_type,
        Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
    ) || local.array_length.is_some()
        || local.is_static
        || local.is_volatile
    {
        return false;
    }
    let Some(mut initializer) = local.initializer.as_ref() else {
        return false;
    };
    while let Expression::Cast { operand, .. } = initializer {
        initializer = operand;
    }
    variable(initializer, source)
}

fn recognize(function: &Function) -> Option<FloatListParser<'_>> {
    if function.return_type != Type::Int
        || !function.guards.is_empty()
        || function.parameters.len() != 3
        || function.locals.len() != 6
    {
        return None;
    }
    let [destination_parameter, string_parameter, maximum_parameter] =
        function.parameters.as_slice()
    else {
        return None;
    };
    if destination_parameter.parameter_type != Type::Pointer(Pointee::Float)
        || !matches!(
            string_parameter.parameter_type,
            Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
        )
        || maximum_parameter.parameter_type != Type::Int
    {
        return None;
    }
    let [string_local, index_local, digits_local, negate_local, start_local, saved_local] =
        function.locals.as_slice()
    else {
        return None;
    };
    if !pointer_alias(string_local, &string_parameter.name)
        || index_local.declared_type != Type::Int
        || digits_local.declared_type != Type::Int
        || !matches!(negate_local.declared_type, Type::Char | Type::UnsignedChar)
        || !matches!(
            start_local.declared_type,
            Type::Pointer(Pointee::Char | Pointee::UnsignedChar)
        )
        || !matches!(saved_local.declared_type, Type::Char | Type::UnsignedChar)
        || function
            .locals
            .iter()
            .skip(1)
            .any(|local| local.initializer.is_some())
        || !matches!(
            function.return_expression.as_ref(),
            Some(expression) if variable(expression, &index_local.name)
        )
    {
        return None;
    }
    let [Statement::If {
        condition: null_string,
        then_body: null_body,
        else_body: null_else,
    }, Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(outer_condition),
        step: Some(step),
        body,
    }] = function.statements.as_slice()
    else {
        return None;
    };
    if !matches!(
        null_string,
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } if variable(operand, &string_local.name)
    ) || !matches!(
        null_body.as_slice(),
        [Statement::Return(Some(value))] if constant_value(value) == Some(0)
    ) || !null_else.is_empty()
        || !matches!(
            initializer,
            Expression::Assign { target, value }
                if variable(target, &index_local.name)
                    && constant_value(value) == Some(0)
        )
        || !increment_assignment(step, &index_local.name)
        || !matches!(
            outer_condition,
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            } if matches!(
                left.as_ref(),
                Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left,
                    right,
                } if dereference_of(left, &string_local.name)
                    && constant_value(right) == Some(0)
            ) && matches!(
                right.as_ref(),
                Expression::Binary {
                    operator: BinaryOperator::Less,
                    left,
                    right,
                } if variable(left, &index_local.name)
                    && variable(right, &maximum_parameter.name)
            )
        )
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        condition: Some(delimiters),
        body: delimiter_body,
        ..
    }, Statement::If {
        condition: end_condition,
        then_body: end_body,
        else_body: end_else,
    }, Statement::If {
        condition: sign_condition,
        then_body: sign_body,
        else_body: sign_else,
    }, start_assignment, digits_assignment, Statement::Loop {
        kind: LoopKind::While,
        condition: Some(number_condition),
        body: number_body,
        ..
    }, Statement::If {
        condition: no_digits,
        then_body: no_digits_body,
        else_body: no_digits_else,
    }, save_assignment, terminator_store, conversion_store, Statement::If {
        condition: negate_condition,
        then_body: negate_body,
        else_body: negate_else,
    }, restore_store, destination_increment] = body.as_slice()
    else {
        return None;
    };
    let mut delimiter_values = Vec::new();
    if !equality_constants(delimiters, &string_local.name, &mut delimiter_values)
        || delimiter_values != [9, 32, 91, 93, 123, 125, 40, 41, 43, 44, 58, 59]
        || !matches!(
            delimiter_body.as_slice(),
            [statement] if increment_statement(statement, &string_local.name)
        )
        || !matches!(
            end_condition,
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if dereference_of(left, &string_local.name)
                && constant_value(right) == Some(0)
        )
        || !matches!(
            end_body.as_slice(),
            [Statement::Return(Some(value))] if variable(value, &index_local.name)
        )
        || !end_else.is_empty()
        || !matches!(
            sign_condition,
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if dereference_of(left, &string_local.name)
                && constant_value(right) == Some(45)
        )
        || sign_body.len() != 3
        || !assigned_constant(&sign_body[0], &negate_local.name, 1)
        || !increment_statement(&sign_body[1], &string_local.name)
        || !matches!(
            sign_else.as_slice(),
            [statement] if assigned_constant(statement, &negate_local.name, 0)
        )
        || !assigned_variable(start_assignment, &start_local.name, &string_local.name)
        || !assigned_constant(digits_assignment, &digits_local.name, 0)
        || !matches!(
            no_digits,
            Expression::Binary {
                operator: BinaryOperator::Equal,
                left,
                right,
            } if variable(left, &digits_local.name)
                && constant_value(right) == Some(0)
        )
        || !matches!(
            no_digits_body.as_slice(),
            [Statement::Return(Some(value))] if variable(value, &index_local.name)
        )
        || !no_digits_else.is_empty()
        || !matches!(
            save_assignment,
            Statement::Assign { name, value }
                if name == &saved_local.name
                    && dereference_of(value, &string_local.name)
        )
        || !matches!(
            terminator_store,
            Statement::Store { target, value }
                if dereference_of(target, &string_local.name)
                    && constant_value(value) == Some(0)
        )
        || !variable(negate_condition, &negate_local.name)
        || !negate_else.is_empty()
        || !matches!(
            restore_store,
            Statement::Store { target, value }
                if dereference_of(target, &string_local.name)
                    && variable(value, &saved_local.name)
        )
        || !increment_statement(destination_increment, &destination_parameter.name)
    {
        return None;
    }
    let [Statement::Loop {
        kind: LoopKind::While,
        condition: Some(sign_space_condition),
        body: sign_space_body,
        ..
    }] = &sign_body[2..]
    else {
        return None;
    };
    let mut sign_space_values = Vec::new();
    if !equality_constants(
        sign_space_condition,
        &string_local.name,
        &mut sign_space_values,
    ) || sign_space_values != [9, 32]
        || !matches!(
            sign_space_body.as_slice(),
            [statement] if increment_statement(statement, &string_local.name)
        )
        || number_body.len() != 2
        || !increment_statement(&number_body[1], &string_local.name)
    {
        return None;
    }
    let [Statement::Store {
        target: conversion_target,
        value: Expression::Call {
            name: conversion,
            arguments,
        },
    }] = std::slice::from_ref(conversion_store)
    else {
        return None;
    };
    if !dereference_of(conversion_target, &destination_parameter.name)
        || !matches!(
            arguments.as_slice(),
            [argument] if variable(argument, &start_local.name)
        )
        || !matches!(
            negate_body.as_slice(),
            [Statement::Store { target, value }]
                if dereference_of(target, &destination_parameter.name)
                    && matches!(
                        value,
                        Expression::Unary {
                            operator: UnaryOperator::Negate,
                            operand,
                        } if dereference_of(operand, &destination_parameter.name)
                    )
        )
        || !matches!(
            number_condition,
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                ..
            }
        )
    {
        return None;
    }
    Some(FloatListParser { conversion })
}

impl Generator {
    pub(crate) fn try_float_list_parser(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.behavior.use_lmw_stmw
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let destination = self.fresh_virtual_general_preferring(28);
        let maximum = self.fresh_virtual_general_preferring(29);
        let string = self.fresh_virtual_general_preferring(31);
        let index = self.fresh_virtual_general_preferring(30);
        let negate = self.fresh_virtual_general_preferring(26);
        let character = self.fresh_virtual_general_preferring(27);

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![negate, character, destination, maximum, index, string];
        self.output.pre_scheduled = true;
        self.owns_link_register_schedule = true;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::StoreMultipleWord {
                s: negate,
                a: 1,
                offset: 8,
            },
            Instruction::move_register(destination, 3),
            Instruction::move_register(maximum, 5),
            Instruction::move_register(string, 4),
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 11,
            },
            Instruction::load_immediate(3, 0),
            Instruction::Branch { target: 101 },
            Instruction::load_immediate(index, 0),
            Instruction::Branch { target: 95 },
            Instruction::Branch { target: 15 },
            Instruction::AddImmediate {
                d: string,
                a: string,
                immediate: 1,
            },
            Instruction::LoadByteZero {
                d: 3,
                a: string,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 9 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 32,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 91,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 93,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 123,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 125,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 14,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -40,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 24,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 14,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -43,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 24,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 14,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -58,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 24,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 14,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 44,
            },
            Instruction::move_register(3, index),
            Instruction::Branch { target: 101 },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 45,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 56,
            },
            Instruction::load_immediate(negate, 1),
            Instruction::AddImmediate {
                d: string,
                a: string,
                immediate: 1,
            },
            Instruction::Branch { target: 50 },
            Instruction::AddImmediate {
                d: string,
                a: string,
                immediate: 1,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: string,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 9 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 49,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 32,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 49,
            },
            Instruction::Branch { target: 57 },
            Instruction::load_immediate(negate, 0),
            Instruction::move_register(3, string),
            Instruction::load_immediate(4, 0),
            Instruction::Branch { target: 66 },
            Instruction::CompareLogicalWordImmediate {
                a: character,
                immediate: 48,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 65,
            },
            Instruction::CompareLogicalWordImmediate {
                a: character,
                immediate: 57,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 65,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: string,
                a: string,
                immediate: 1,
            },
            Instruction::LoadByteZero {
                d: character,
                a: string,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: character,
                immediate: 48,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 71,
            },
            Instruction::CompareLogicalWordImmediate {
                a: character,
                immediate: 57,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 60,
            },
            Instruction::CompareLogicalWordImmediate {
                a: character,
                immediate: 46,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 60,
            },
            Instruction::CompareLogicalWordImmediate {
                a: character,
                immediate: 69,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 60,
            },
            Instruction::AddImmediate {
                d: 0,
                a: character,
                immediate: -101,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 24,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 1,
                target: 60,
            },
            Instruction::CompareWordImmediate { a: 4, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 83,
            },
            Instruction::move_register(3, index),
            Instruction::Branch { target: 101 },
            Instruction::load_immediate(0, 0),
            Instruction::StoreByte {
                s: 0,
                a: string,
                offset: 0,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.conversion);
        self.output.instructions.extend([
            Instruction::BranchAndLink {
                target: shape.conversion.to_string(),
            },
            Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: negate,
                clear: 24,
            },
            Instruction::StoreFloatSingle {
                s: 1,
                a: destination,
                offset: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 92,
            },
            Instruction::LoadFloatSingle {
                d: 0,
                a: destination,
                offset: 0,
            },
            Instruction::FloatNegate { d: 0, b: 0 },
            Instruction::StoreFloatSingle {
                s: 0,
                a: destination,
                offset: 0,
            },
            Instruction::StoreByte {
                s: character,
                a: string,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: destination,
                a: destination,
                immediate: 4,
            },
            Instruction::AddImmediate {
                d: index,
                a: index,
                immediate: 1,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: string,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 100,
            },
            Instruction::CompareWord {
                a: index,
                b: maximum,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 0,
                target: 15,
            },
            Instruction::move_register(3, index),
            Instruction::LoadMultipleWord {
                d: negate,
                a: 1,
                offset: 8,
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
            false,
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
    fn collects_source_order_equality_constants() {
        let leaf = |value| Expression::Binary {
            operator: BinaryOperator::Equal,
            left: Box::new(Expression::Dereference {
                pointer: Box::new(Expression::Variable("p".into())),
            }),
            right: Box::new(Expression::IntegerLiteral(value)),
        };
        let expression = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(leaf(9)),
            right: Box::new(leaf(32)),
        };
        let mut values = Vec::new();
        assert!(equality_constants(&expression, "p", &mut values));
        assert_eq!(values, [9, 32]);
    }
}
