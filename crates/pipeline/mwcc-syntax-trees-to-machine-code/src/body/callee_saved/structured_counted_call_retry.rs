//! Counted retry loops whose final call result remains in the ABI result lane.
//!
//! A leading source assignment initializes the retry counter before a
//! guaranteed `do/while`. Treating that assignment as a deferred local gives
//! the incoming call argument the highest saved home and retains a phantom
//! entry-value lane. This recognizer promotes only the complete transaction to
//! a declaration initializer, leaving its call result transient.

use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LoopKind, Statement, Type};
use mwcc_machine_code::Instruction;

pub(super) fn normalize(function: &Function) -> Option<Function> {
    let [Statement::Assign { name: counter, value: initial }, loop_statement] =
        function.statements.as_slice()
    else {
        return None;
    };
    let result = classify_loop(function, loop_statement, counter)?;
    if !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == result)
    {
        return None;
    }
    let local = function.locals.iter().find(|local| local.name == *counter)?;
    if local.initializer.is_some() || !matches!(initial, Expression::IntegerLiteral(value) if *value > 0)
    {
        return None;
    }

    let mut normalized = function.clone();
    normalized
        .locals
        .iter_mut()
        .find(|local| local.name == *counter)
        .expect("the retry counter was classified")
        .initializer = Some(initial.clone());
    normalized.statements.remove(0);
    Some(normalized)
}

pub(super) fn is_normalized(function: &Function) -> bool {
    let [loop_statement] = function.statements.as_slice() else {
        return false;
    };
    function.locals.iter().any(|counter| {
        matches!(counter.initializer, Some(Expression::IntegerLiteral(value)) if value > 0)
            && classify_loop(function, loop_statement, &counter.name).is_some_and(|result| {
                matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == result)
            })
    })
}

pub(super) fn schedule(instructions: &mut [Instruction]) {
    let Some(start) = instructions.windows(6).position(|window| {
        matches!(window,
            [
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d, a, immediate: -1 },
                Instruction::CompareWordImmediate { a: 3, immediate: 0 },
                Instruction::BranchConditionalForward { condition_bit: 2, .. },
                Instruction::CompareWordImmediate { a: counter, immediate: 0 },
                Instruction::BranchConditionalForward { condition_bit: 1, .. },
            ] if d == a && d == counter)
    }) else {
        return;
    };
    instructions.swap(start + 1, start + 2);
}

fn classify_loop<'a>(
    function: &'a Function,
    statement: &'a Statement,
    counter: &str,
) -> Option<&'a str> {
    if !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        || !function.guards.is_empty()
        || function.parameters.len() != 1
        || function.locals.len() != 2
        || function.locals.iter().any(|local| {
            !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
                || local.array_length.is_some()
                || local.is_static
                || local.is_volatile
        })
    {
        return None;
    }
    let Statement::Loop {
        kind: LoopKind::DoWhile,
        initializer: None,
        condition: Some(condition),
        step: None,
        body,
    } = statement
    else {
        return None;
    };
    let [
        Statement::Assign {
            name: result,
            value:
                Expression::Call {
                    arguments,
                    ..
                },
        },
        Statement::Assign {
            name: decremented,
            value: decrement,
        },
    ] = body.as_slice()
    else {
        return None;
    };
    let [Expression::Variable(argument)] = arguments.as_slice() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::LogicalAnd,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let valid_result_test = matches!(left.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left,
            right,
        } if variable(left, result) && integer(right, 0));
    let valid_counter_test = matches!(right.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Greater,
            left,
            right,
        } if variable(left, counter) && integer(right, 0));
    let valid_decrement = matches!(decrement,
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } if variable(left, counter) && integer(right, 1));
    (decremented == counter
        && argument == &function.parameters[0].name
        && result != counter
        && valid_result_test
        && valid_counter_test
        && valid_decrement)
        .then_some(result.as_str())
}

fn variable(expression: &Expression, expected: &str) -> bool {
    matches!(expression, Expression::Variable(name) if name == expected)
}

fn integer(expression: &Expression, expected: i64) -> bool {
    matches!(expression, Expression::IntegerLiteral(value) if *value == expected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{LocalDeclaration, Parameter};

    fn retry() -> Function {
        Function {
            return_type: Type::Int,
            name: "retry".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Pointer(mwcc_syntax_trees::Pointee::Int),
                name: "buffer".into(),
            }],
            locals: ["result", "tries"]
                .into_iter()
                .map(|name| LocalDeclaration {
                    declared_type: Type::Int,
                    name: name.into(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    attribute_alignment: None,
                    row_bytes: None,
                })
                .collect(),
            statements: vec![
                Statement::Assign {
                    name: "tries".into(),
                    value: Expression::IntegerLiteral(3),
                },
                Statement::Loop {
                    kind: LoopKind::DoWhile,
                    initializer: None,
                    condition: Some(Expression::Binary {
                        operator: BinaryOperator::LogicalAnd,
                        left: Box::new(Expression::Binary {
                            operator: BinaryOperator::NotEqual,
                            left: Box::new(Expression::Variable("result".into())),
                            right: Box::new(Expression::IntegerLiteral(0)),
                        }),
                        right: Box::new(Expression::Binary {
                            operator: BinaryOperator::Greater,
                            left: Box::new(Expression::Variable("tries".into())),
                            right: Box::new(Expression::IntegerLiteral(0)),
                        }),
                    }),
                    step: None,
                    body: vec![
                        Statement::Assign {
                            name: "result".into(),
                            value: Expression::Call {
                                name: "send".into(),
                                arguments: vec![Expression::Variable("buffer".into())],
                            },
                        },
                        Statement::Assign {
                            name: "tries".into(),
                            value: Expression::Binary {
                                operator: BinaryOperator::Subtract,
                                left: Box::new(Expression::Variable("tries".into())),
                                right: Box::new(Expression::IntegerLiteral(1)),
                            },
                        },
                    ],
                },
            ],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
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
    fn promotes_the_retry_count_to_an_entry_initializer() {
        let normalized = normalize(&retry()).expect("the complete retry loop should match");

        assert!(is_normalized(&normalized));
        assert_eq!(normalized.statements.len(), 1);
        assert!(matches!(
            normalized.locals[1].initializer,
            Some(Expression::IntegerLiteral(3))
        ));
    }

    #[test]
    fn schedules_the_result_test_before_the_independent_decrement() {
        let mut instructions = vec![
            Instruction::BranchAndLink { target: "send".into() },
            Instruction::AddImmediate { d: 31, a: 31, immediate: -1 },
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 6,
            },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 1,
                target: 0,
            },
        ];

        schedule(&mut instructions);

        assert!(matches!(instructions[1], Instruction::CompareWordImmediate { a: 3, .. }));
        assert!(matches!(instructions[2], Instruction::AddImmediate { d: 31, .. }));
    }
}
