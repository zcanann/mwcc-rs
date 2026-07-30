//! Counted pointer searches with an early pointer return.
//!
//! Wide-character library searches expose the compact CTR form particularly
//! clearly: the source counter exists only to bound the walk, so mwcc places
//! the bound in CTR and advances the pointer directly.  Recognition and
//! emission live here so additional measured element widths can extend this
//! family without adding function-name or project-specific captures.

#[allow(unused_imports)]
use super::*;

struct CountedPointerSearch<'a> {
    cursor: &'a str,
    needle: &'a str,
    count: &'a str,
    stride: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn assignment<'a>(expression: &'a Expression, target: &str) -> Option<&'a Expression> {
    match expression {
        Expression::Assign {
            target: assigned,
            value,
        } if variable(assigned) == Some(target) => Some(value),
        _ => None,
    }
}

fn is_increment_of(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if variable(left) == Some(name) && constant_value(right) == Some(1)
    )
}

fn is_name_pair(left: &Expression, right: &Expression, first: &str, second: &str) -> bool {
    (variable(left) == Some(first) && variable(right) == Some(second))
        || (variable(left) == Some(second) && variable(right) == Some(first))
}

fn recognize(function: &Function) -> Option<CountedPointerSearch<'_>> {
    if function.return_type != Type::Pointer(Pointee::UnsignedShort)
        || !function.guards.is_empty()
        || function_makes_call(function)
        || constant_value(function.return_expression.as_ref()?) != Some(0)
    {
        return None;
    }
    let [cursor, needle, count] = function.parameters.as_slice() else {
        return None;
    };
    if cursor.parameter_type != Type::Pointer(Pointee::UnsignedShort)
        || needle.parameter_type != Type::UnsignedShort
        || count.parameter_type != Type::Int
    {
        return None;
    }
    let [counter] = function.locals.as_slice() else {
        return None;
    };
    if counter.declared_type != Type::Int
        || counter.initializer.is_some()
        || counter.is_volatile
        || counter.array_length.is_some()
        || counter.is_static
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
    if assignment(initializer, &counter.name).and_then(constant_value) != Some(0)
        || !matches!(
            condition,
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if is_name_pair(left, right, &counter.name, &count.name)
        )
        || !assignment(step, &counter.name)
            .is_some_and(|value| is_increment_of(value, &counter.name))
    {
        return None;
    }
    let [Statement::If {
        condition: found,
        then_body,
        else_body,
    }, Statement::Assign {
        name: advanced,
        value: advance,
    }] = body.as_slice()
    else {
        return None;
    };
    if !else_body.is_empty()
        || !matches!(
            then_body.as_slice(),
            [Statement::Return(Some(value))] if variable(value) == Some(&cursor.name)
        )
        || advanced != &cursor.name
        || !is_increment_of(advance, &cursor.name)
    {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = found
    else {
        return None;
    };
    let dereferences_cursor = |expression: &Expression| {
        matches!(
            expression,
            Expression::Dereference { pointer }
                if variable(pointer) == Some(&cursor.name)
        )
    };
    let compares_needle = (dereferences_cursor(left) && variable(right) == Some(&needle.name))
        || (dereferences_cursor(right) && variable(left) == Some(&needle.name));
    if !compares_needle {
        return None;
    }

    Some(CountedPointerSearch {
        cursor: &cursor.name,
        needle: &needle.name,
        count: &count.name,
        stride: i16::from(Pointee::UnsignedShort.size()),
    })
}

impl Generator {
    pub(crate) fn try_counted_pointer_search(&mut self, function: &Function) -> Compilation<bool> {
        let Some(search) = recognize(function) else {
            return Ok(false);
        };
        if !self.frame_slots.is_empty() || !self.output.instructions.is_empty() {
            return Ok(false);
        }
        let cursor = self.general_register_of(search.cursor)?;
        let needle = self.general_register_of(search.needle)?;
        let count = self.general_register_of(search.count)?;
        if (cursor, needle, count) != (3, 4, 5) {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        // The source `for` contributes five anonymous control-flow ordinals,
        // and its early-return `if` contributes two.
        self.output.anonymous_label_bump += 7;
        self.output.instructions.extend([
            Instruction::MoveToCountRegister { s: count },
            Instruction::CompareWordImmediate {
                a: count,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
            Instruction::LoadHalfwordZero {
                d: GENERAL_SCRATCH,
                a: cursor,
                offset: 0,
            },
            Instruction::CompareLogicalWord {
                a: GENERAL_SCRATCH,
                b: needle,
            },
            Instruction::BranchConditionalToLinkRegister {
                options: 12,
                condition_bit: 2,
            },
            Instruction::AddImmediate {
                d: cursor,
                a: cursor,
                immediate: search.stride,
            },
            Instruction::BranchConditionalForward {
                options: 16,
                condition_bit: 0,
                target: 3,
            },
            Instruction::load_immediate(Eabi::general_result().number, 0),
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn name(value: &str) -> Expression {
        Expression::Variable(value.to_string())
    }

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn assign(target: &str, value: Expression) -> Expression {
        Expression::Assign {
            target: Box::new(name(target)),
            value: Box::new(value),
        }
    }

    fn function() -> Function {
        Function {
            return_type: Type::Pointer(Pointee::UnsignedShort),
            name: "renamed_wide_search".to_string(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Pointer(Pointee::UnsignedShort),
                    name: "cursor".to_string(),
                },
                Parameter {
                    parameter_type: Type::UnsignedShort,
                    name: "wanted".to_string(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "limit".to_string(),
                },
            ],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "index".to_string(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(assign("index", Expression::IntegerLiteral(0))),
                condition: Some(binary(
                    BinaryOperator::NotEqual,
                    name("index"),
                    name("limit"),
                )),
                step: Some(assign(
                    "index",
                    binary(
                        BinaryOperator::Add,
                        name("index"),
                        Expression::IntegerLiteral(1),
                    ),
                )),
                body: vec![
                    Statement::If {
                        condition: binary(
                            BinaryOperator::Equal,
                            Expression::Dereference {
                                pointer: Box::new(name("cursor")),
                            },
                            name("wanted"),
                        ),
                        then_body: vec![Statement::Return(Some(name("cursor")))],
                        else_body: Vec::new(),
                    },
                    Statement::Assign {
                        name: "cursor".to_string(),
                        value: binary(
                            BinaryOperator::Add,
                            name("cursor"),
                            Expression::IntegerLiteral(1),
                        ),
                    },
                ],
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::IntegerLiteral(0)),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn recognizes_the_counted_wide_search_independently_of_names() {
        let function = function();
        let search = recognize(&function).expect("counted pointer search");
        assert_eq!(search.cursor, "cursor");
        assert_eq!(search.needle, "wanted");
        assert_eq!(search.count, "limit");
        assert_eq!(search.stride, 2);
    }

    #[test]
    fn rejects_a_cursor_advance_by_the_wrong_stride() {
        let mut function = function();
        let Statement::Loop { body, .. } = &mut function.statements[0] else {
            unreachable!()
        };
        let Statement::Assign { value, .. } = &mut body[1] else {
            unreachable!()
        };
        *value = binary(
            BinaryOperator::Add,
            name("cursor"),
            Expression::IntegerLiteral(2),
        );
        assert!(recognize(&function).is_none());
    }
}
