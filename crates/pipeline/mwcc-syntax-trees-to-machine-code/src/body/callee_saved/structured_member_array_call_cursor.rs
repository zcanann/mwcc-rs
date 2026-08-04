//! Strength reduction for repeated member-array arguments in counted loops.
//!
//! Optimized MWCC turns `object->words[i]` and `object->words[i + k]` passed
//! by one loop-local call into displacement loads from a pointer carried by the
//! loop. Making that cursor explicit before liveness planning gives its
//! cross-call lifetime to the ordinary allocator and leaves subscript lowering
//! with constant indices.

use super::*;

#[derive(Clone)]
struct CursorPlan {
    index: String,
    array: Expression,
    cursor_base: Expression,
    element: Pointee,
    element_offset: i64,
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
    let mut statements = Vec::with_capacity(function.statements.len());
    let mut changed = false;

    for statement in &function.statements {
        let Some(plan) = plan(statement) else {
            statements.push(statement.clone());
            continue;
        };
        let cursor = fresh_name(&mut used, &mut next_name);
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
        statements.push(rewrite_loop(statement, &plan, &cursor));
        changed = true;
    }

    changed.then(|| {
        let mut reduced = function.clone();
        reduced.locals.extend(declarations);
        reduced.statements = statements;
        reduced
    })
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
    if unit_step_index(step)? != index
        || !crate::analysis::expression_reads_name(condition, index)
    {
        return None;
    }
    let [Statement::Expression(Expression::Call { arguments, .. })] = body.as_slice() else {
        return None;
    };

    for argument in arguments {
        let Expression::Index { base, index: used } = argument else {
            continue;
        };
        let Expression::MemberAddress {
            base: owner,
            offset,
            element,
            index_stride: None,
        } = base.as_ref()
        else {
            continue;
        };
        let element_size = i64::from(element.size());
        if !matches!(element, Pointee::Int | Pointee::UnsignedInt)
            || i64::from(*offset) % element_size != 0
            || relative_index(used, index).is_none()
        {
            continue;
        }
        let element_offset = i64::from(*offset) / element_size;
        let matching = arguments
            .iter()
            .filter_map(|argument| match argument {
                Expression::Index {
                    base: other_base,
                    index: other_index,
                } if crate::analysis::structurally_equal(base, other_base) => {
                    relative_index(other_index, index)
                }
                _ => None,
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
                array: base.as_ref().clone(),
                cursor_base: Expression::MemberAddress {
                    base: owner.clone(),
                    offset: 0,
                    element: *element,
                    index_stride: None,
                },
                element: *element,
                element_offset,
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

fn unit_step_index(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    matches!(
        value.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(name) if name == index)
            && crate::analysis::constant_value(right) == Some(1)
    )
    .then_some(index)
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
                    right: Box::new(Expression::IntegerLiteral(1)),
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
            .map(|argument| {
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
            })
            .collect(),
    })
}

fn fresh_name(
    used: &mut std::collections::HashSet<String>,
    next: &mut usize,
) -> String {
    loop {
        let name = format!("__mwcc_member_array_cursor_{}", *next);
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
                    right: Box::new(Expression::IntegerLiteral(1)),
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
    fn leaves_a_single_member_array_argument_alone() {
        let source = function(counted_call(vec![word(Expression::Variable("i".into()))]));
        assert!(strength_reduce_member_array_call_cursors(&source).is_none());
    }
}
