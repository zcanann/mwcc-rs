//! Strength reduction for repeated member-array arguments in counted loops.
//!
//! Optimized MWCC turns `object->words[i]` and `object->words[i + k]` passed
//! by one loop-local call into displacement loads from a pointer carried by the
//! loop. Making that cursor explicit before liveness planning gives its
//! cross-call lifetime to the ordinary allocator and leaves subscript lowering
//! with constant indices.

use super::*;

pub(super) const CURSOR_PREFIX: &str = "__mwcc_member_array_cursor_";

#[derive(Clone)]
struct CursorPlan {
    index: String,
    array: Expression,
    cursor_base: Expression,
    element: Pointee,
    element_offset: i64,
    step_elements: i64,
}

pub(super) fn strength_reduce_member_array_call_cursors(
    function: &Function,
) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut next_name = 0usize;
    let mut declarations = Vec::new();
    let mut cursor_by_element = Vec::<(Pointee, String)>::new();
    let mut statements = Vec::with_capacity(function.statements.len());
    let mut changed = false;

    statements.extend(function.statements.iter().map(|statement| {
        reduce_statement(
            statement,
            &mut used,
            &mut next_name,
            &mut declarations,
            &mut cursor_by_element,
            &mut changed,
        )
    }));

    changed.then(|| {
        let mut reduced = function.clone();
        reduced.locals.extend(declarations);
        reduced.statements = statements;
        reduced
    })
}

fn reduce_statement(
    statement: &Statement,
    used: &mut std::collections::HashSet<String>,
    next_name: &mut usize,
    declarations: &mut Vec<LocalDeclaration>,
    cursor_by_element: &mut Vec<(Pointee, String)>,
    changed: &mut bool,
) -> Statement {
    if let Some(plan) = plan(statement) {
        let cursor = cursor_by_element
            .iter()
            .find_map(|(element, cursor)| (*element == plan.element).then(|| cursor.clone()))
            .unwrap_or_else(|| {
                let cursor = fresh_name(used, next_name);
                declarations.push(LocalDeclaration {
                    declared_type: Type::Pointer(plan.element),
                    name: cursor.clone(),
                    initializer: None,
                    is_volatile: false,
                    array_length: None,
                    is_static: false,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    is_const: false,
                    attribute_alignment: None,
                    row_bytes: None,
                });
                cursor_by_element.push((plan.element, cursor.clone()));
                cursor
            });
        *changed = true;
        return rewrite_loop(statement, &plan, &cursor);
    }
    match statement {
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: condition.clone(),
            then_body: then_body
                .iter()
                .map(|statement| {
                    reduce_statement(
                        statement,
                        used,
                        next_name,
                        declarations,
                        cursor_by_element,
                        changed,
                    )
                })
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| {
                    reduce_statement(
                        statement,
                        used,
                        next_name,
                        declarations,
                        cursor_by_element,
                        changed,
                    )
                })
                .collect(),
        },
        Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } => Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: step.clone(),
            body: body
                .iter()
                .map(|statement| {
                    reduce_statement(
                        statement,
                        used,
                        next_name,
                        declarations,
                        cursor_by_element,
                        changed,
                    )
                })
                .collect(),
        },
        _ => statement.clone(),
    }
}

fn plan(statement: &Statement) -> Option<CursorPlan> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    let index = zero_initializer(initializer)?;
    let (step_index, step_elements) = counted_step(step)?;
    if step_index != index || !crate::analysis::expression_reads_name(condition, index) {
        return None;
    }
    let [Statement::Expression(Expression::Call { arguments, .. })] = body.as_slice() else {
        return None;
    };

    for argument in arguments {
        let Some((base, used)) = indexed_argument(argument) else {
            continue;
        };
        let Expression::MemberAddress {
            base: owner,
            offset,
            element,
            index_stride: None,
        } = base
        else {
            continue;
        };
        let element_size = i64::from(element.size());
        if !matches!(
            element,
            Pointee::Int | Pointee::UnsignedInt | Pointee::Float | Pointee::Double
        )
            || i64::from(*offset) % element_size != 0
            || relative_index(used, index).is_none()
        {
            continue;
        }
        let element_offset = i64::from(*offset) / element_size;
        let matching = arguments
            .iter()
            .filter_map(|argument| {
                let (other_base, other_index) = indexed_argument(argument)?;
                crate::analysis::structurally_equal(base, other_base)
                    .then(|| relative_index(other_index, index))
                    .flatten()
            })
            .collect::<Vec<_>>();
        if matching.len() >= 2
            && matching.contains(&0)
            && matching.iter().all(|relative| {
                relative
                    .checked_add(element_offset)
                    .and_then(|index| index.checked_mul(element_size))
                    .is_some_and(|offset| i16::try_from(offset).is_ok())
            })
        {
            return Some(CursorPlan {
                index: index.to_owned(),
                array: base.clone(),
                cursor_base: Expression::MemberAddress {
                    base: owner.clone(),
                    offset: 0,
                    element: *element,
                    index_stride: None,
                },
                element: *element,
                element_offset,
                step_elements,
            });
        }
    }
    None
}

fn zero_initializer(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    (crate::analysis::constant_value(value) == Some(0)).then_some(index)
}

fn counted_step(expression: &Expression) -> Option<(&str, i64)> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = value.as_ref()
    else {
        return None;
    };
    let step = crate::analysis::constant_value(right)?;
    (matches!(left.as_ref(), Expression::Variable(name) if name == index) && step > 0)
        .then_some((index.as_str(), step))
}

fn indexed_argument(expression: &Expression) -> Option<(&Expression, &Expression)> {
    match expression {
        Expression::Index { base, index } => Some((base, index)),
        Expression::Cast { operand, .. } => indexed_argument(operand),
        _ => None,
    }
}

fn relative_index(expression: &Expression, index: &str) -> Option<i64> {
    match expression {
        Expression::Variable(name) if name == index => Some(0),
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } => match (left.as_ref(), right.as_ref()) {
            (Expression::Variable(name), constant) if name == index => {
                crate::analysis::constant_value(constant)
            }
            (constant, Expression::Variable(name)) if name == index => {
                crate::analysis::constant_value(constant)
            }
            _ => None,
        },
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name) if name == index) => {
            crate::analysis::constant_value(right)?.checked_neg()
        }
        _ => None,
    }
}

fn rewrite_loop(statement: &Statement, plan: &CursorPlan, cursor: &str) -> Statement {
    let Statement::Loop {
        kind,
        initializer,
        condition,
        step,
        body,
    } = statement
    else {
        unreachable!("member-array cursor was recognized from a loop")
    };
    let initializer = initializer
        .as_ref()
        .expect("recognized member-array cursor initializer");
    let step = step.as_ref().expect("recognized member-array cursor step");
    Statement::Loop {
        kind: *kind,
        initializer: Some(Expression::Comma {
            left: Box::new(initializer.clone()),
            right: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable(cursor.to_owned())),
                value: Box::new(Expression::AddressOf {
                    operand: Box::new(Expression::Index {
                        base: Box::new(plan.cursor_base.clone()),
                        index: Box::new(Expression::Variable(plan.index.clone())),
                    }),
                }),
            }),
        }),
        condition: condition.clone(),
        step: Some(Expression::Comma {
            left: Box::new(step.clone()),
            right: Box::new(Expression::Assign {
                target: Box::new(Expression::Variable(cursor.to_owned())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable(cursor.to_owned())),
                    right: Box::new(Expression::IntegerLiteral(plan.step_elements)),
                }),
            }),
        }),
        body: body
            .iter()
            .map(|statement| rewrite_call(statement, plan, cursor))
            .collect(),
    }
}

fn rewrite_call(statement: &Statement, plan: &CursorPlan, cursor: &str) -> Statement {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return statement.clone();
    };
    Statement::Expression(Expression::Call {
        name: name.clone(),
        arguments: arguments
            .iter()
            .map(|argument| rewrite_argument(argument, plan, cursor))
            .collect(),
    })
}

fn rewrite_argument(argument: &Expression, plan: &CursorPlan, cursor: &str) -> Expression {
    if let Expression::Cast {
        target_type,
        operand,
    } = argument
    {
        return Expression::Cast {
            target_type: *target_type,
            operand: Box::new(rewrite_argument(operand, plan, cursor)),
        };
    }
    let Expression::Index { base, index } = argument else {
        return argument.clone();
    };
    if !crate::analysis::structurally_equal(base, &plan.array) {
        return argument.clone();
    }
    let Some(relative) = relative_index(index, &plan.index) else {
        return argument.clone();
    };
    Expression::Index {
        base: Box::new(Expression::Variable(cursor.to_owned())),
        index: Box::new(Expression::IntegerLiteral(
            relative + plan.element_offset,
        )),
    }
}

fn fresh_name(
    used: &mut std::collections::HashSet<String>,
    next: &mut usize,
) -> String {
    loop {
        let name = format!("{CURSOR_PREFIX}{}", *next);
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(index: Expression) -> Expression {
        Expression::Index {
            base: Box::new(Expression::MemberAddress {
                base: Box::new(Expression::Variable("context".into())),
                offset: 0,
                element: Pointee::UnsignedInt,
                index_stride: None,
            }),
            index: Box::new(index),
        }
    }

    fn counted_call(arguments: Vec<Expression>) -> Statement {
        counted_call_with_step(arguments, 1)
    }

    fn counted_call_with_step(arguments: Vec<Expression>, step: i64) -> Statement {
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(16)),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::IntegerLiteral(step)),
                }),
            }),
            body: vec![Statement::Expression(Expression::Call {
                name: "report".into(),
                arguments,
            })],
        }
    }

    fn function(statement: Statement) -> Function {
        Function {
            return_type: Type::Void,
            name: "dump".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![mwcc_syntax_trees::Parameter {
                parameter_type: Type::StructPointer { element_size: 8 },
                name: "context".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: Type::UnsignedInt,
                name: "i".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![statement],
            guards: Vec::new(),
            return_expression: None,
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
    fn carries_a_member_array_pointer_across_a_counted_call_loop() {
        let affine = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Variable("i".into())),
            right: Box::new(Expression::IntegerLiteral(16)),
        };
        let source = function(counted_call(vec![
            Expression::Variable("i".into()),
            word(Expression::Variable("i".into())),
            word(affine.clone()),
            word(affine),
        ]));
        let reduced = strength_reduce_member_array_call_cursors(&source)
            .expect("the repeated member array should acquire a cursor");

        assert!(matches!(
            reduced.locals.last(),
            Some(LocalDeclaration {
                name,
                declared_type: Type::Pointer(Pointee::UnsignedInt),
                ..
            }) if name == "__mwcc_member_array_cursor_0"
        ));
        let Statement::Loop { body, .. } = &reduced.statements[0] else {
            panic!("expected the rewritten loop")
        };
        let [Statement::Expression(Expression::Call { arguments, .. })] = body.as_slice() else {
            panic!("expected the rewritten call")
        };
        assert!(matches!(
            &arguments[1],
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::Variable(name) if name == "__mwcc_member_array_cursor_0")
                    && crate::analysis::constant_value(index) == Some(0)
        ));
        assert!(matches!(
            &arguments[2],
            Expression::Index { index, .. }
                if crate::analysis::constant_value(index) == Some(16)
        ));
    }

    #[test]
    fn reuses_one_cursor_lane_for_sequential_loops_over_the_same_element_type() {
        let arguments = || {
            vec![
                word(Expression::Variable("i".into())),
                word(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::IntegerLiteral(16)),
                }),
            ]
        };
        let mut source = function(counted_call(arguments()));
        source.statements.push(counted_call(arguments()));

        let reduced = strength_reduce_member_array_call_cursors(&source)
            .expect("both loops should acquire the shared cursor lane");

        assert_eq!(
            reduced
                .locals
                .iter()
                .filter(|local| local.name.starts_with("__mwcc_member_array_cursor_"))
                .count(),
            1,
        );
        for statement in &reduced.statements {
            let Statement::Loop {
                initializer: Some(Expression::Comma { right, .. }),
                ..
            } = statement
            else {
                panic!("expected a rewritten counted loop")
            };
            assert!(matches!(
                right.as_ref(),
                Expression::Assign { target, .. }
                    if matches!(target.as_ref(), Expression::Variable(name)
                        if name == "__mwcc_member_array_cursor_0")
            ));
        }
    }

    #[test]
    fn leaves_a_single_member_array_argument_alone() {
        let source = function(counted_call(vec![word(Expression::Variable("i".into()))]));
        assert!(strength_reduce_member_array_call_cursors(&source).is_none());
    }

    #[test]
    fn carries_casted_double_elements_with_the_loop_stride() {
        let double = |index| Expression::Cast {
            target_type: Type::UnsignedInt,
            operand: Box::new(Expression::Index {
                base: Box::new(Expression::MemberAddress {
                    base: Box::new(Expression::Variable("context".into())),
                    offset: 144,
                    element: Pointee::Double,
                    index_stride: None,
                }),
                index: Box::new(index),
            }),
        };
        let next = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Variable("i".into())),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        let source = function(Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![counted_call_with_step(
                vec![
                    double(Expression::Variable("i".into())),
                    double(next),
                ],
                2,
            )],
            else_body: Vec::new(),
        });
        let reduced = strength_reduce_member_array_call_cursors(&source)
            .expect("the casted double pair should acquire a cursor");

        assert!(matches!(
            reduced.locals.last(),
            Some(LocalDeclaration {
                declared_type: Type::Pointer(Pointee::Double),
                ..
            })
        ));
        let Statement::If { then_body, .. } = &reduced.statements[0] else {
            panic!("expected the containing conditional")
        };
        let [Statement::Loop { step, body, .. }] = then_body.as_slice() else {
            panic!("expected the rewritten loop")
        };
        assert!(matches!(
            step,
            Some(Expression::Comma { right, .. })
                if matches!(right.as_ref(), Expression::Assign { value, .. }
                    if matches!(value.as_ref(), Expression::Binary { right, .. }
                        if crate::analysis::constant_value(right) == Some(2)))
        ));
        let [Statement::Expression(Expression::Call { arguments, .. })] = body.as_slice() else {
            panic!("expected the rewritten call")
        };
        assert!(matches!(
            &arguments[0],
            Expression::Cast { operand, .. }
                if matches!(operand.as_ref(), Expression::Index { index, .. }
                    if crate::analysis::constant_value(index) == Some(18))
        ));
        assert!(matches!(
            &arguments[1],
            Expression::Cast { operand, .. }
                if matches!(operand.as_ref(), Expression::Index { index, .. }
                    if crate::analysis::constant_value(index) == Some(19))
        ));
    }
}
