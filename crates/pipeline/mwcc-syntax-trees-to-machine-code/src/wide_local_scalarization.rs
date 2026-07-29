//! Prove and scalarize wide locals whose high word is unobservable.
//!
//! A common input-bitset idiom assigns a `u32` call result through a `u64`
//! global and into a `u64` automatic, then tests only masks in the low word.
//! Keep the global write semantically 64-bit, but represent the automatic as
//! its proven `u32` lane so the ordinary structured allocator can own the
//! surrounding function. Arbitrary wide values remain on the pair path.

use crate::analysis::expression_reads_name;
use mwcc_syntax_trees::{ArmBody, BinaryOperator, Expression, Function, Statement, Type};
use std::collections::{HashMap, HashSet};

pub(crate) fn scalarize_zero_extended_mask_local(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
    call_return_types: &HashMap<String, Type>,
) -> Option<Function> {
    let wide_locals: Vec<_> = function
        .locals
        .iter()
        .filter(|local| {
            local.declared_type == Type::UnsignedLongLong
                && local.initializer.is_none()
                && !local.is_static
                && !local.is_volatile
                && local.array_length.is_none()
        })
        .collect();
    let [wide_local] = wide_locals.as_slice() else {
        return None;
    };
    if !function.guards.is_empty()
        || function
            .return_expression
            .as_ref()
            .is_some_and(|value| expression_reads_name(value, &wide_local.name))
        || function.locals.iter().any(|local| {
            local
                .initializer
                .as_ref()
                .is_some_and(|value| expression_reads_name(value, &wide_local.name))
        })
        || !uses_only_low_word_masks(&function.statements, &wide_local.name)
    {
        return None;
    }

    let mut assignment_count = 0usize;
    let statements = rewrite_statements(
        &function.statements,
        &wide_local.name,
        globals,
        volatile_globals,
        call_return_types,
        &mut assignment_count,
    )?;
    if assignment_count != 1 {
        return None;
    }

    let mut rewritten = function.clone();
    rewritten
        .locals
        .iter_mut()
        .find(|local| local.name == wide_local.name)
        .expect("the selected local remains present")
        .declared_type = Type::UnsignedInt;
    rewritten.statements = statements;
    Some(rewritten)
}

fn uses_only_low_word_masks(statements: &[Statement], name: &str) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign {
            name: assigned, ..
        } if assigned == name => true,
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            (!expression_reads_name(condition, name) || low_word_mask(condition, name))
                && uses_only_low_word_masks(then_body, name)
                && uses_only_low_word_masks(else_body, name)
        }
        Statement::Store { target, value } => {
            !expression_reads_name(target, name) && !expression_reads_name(value, name)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            !expression_reads_name(value, name)
        }
        Statement::Return(value) => value
            .as_ref()
            .is_none_or(|value| !expression_reads_name(value, name)),
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            [initializer, condition, step]
                .into_iter()
                .flatten()
                .all(|value| !expression_reads_name(value, name))
                && uses_only_low_word_masks(body, name)
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            !expression_reads_name(scrutinee, name)
                && arms
                    .iter()
                    .all(|arm| arm_uses_only_low_word_masks(&arm.body, name))
                && default
                    .as_ref()
                    .is_none_or(|body| arm_uses_only_low_word_masks(body, name))
        }
        Statement::InlineAsm(_)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => true,
    })
}

fn arm_uses_only_low_word_masks(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(value) => !expression_reads_name(value, name),
        ArmBody::Statements(statements) => uses_only_low_word_masks(statements, name),
    }
}

fn low_word_mask(expression: &Expression, name: &str) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } if matches!(left.as_ref(), Expression::Variable(variable) if variable == name)
            && matches!(
                right.as_ref(),
                Expression::IntegerLiteral(mask) if (1..=u32::MAX as i64).contains(mask)
            )
    )
}

fn rewrite_statements(
    statements: &[Statement],
    name: &str,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
    call_return_types: &HashMap<String, Type>,
    assignment_count: &mut usize,
) -> Option<Vec<Statement>> {
    let mut output = Vec::new();
    for statement in statements {
        match statement {
            Statement::Assign {
                name: assigned,
                value:
                    Expression::Assign {
                        target,
                        value,
                    },
            } if assigned == name => {
                let Expression::Call { name: call, .. } = value.as_ref() else {
                    return None;
                };
                if call_return_types.get(call) != Some(&Type::UnsignedInt) {
                    return None;
                }
                let Expression::Member {
                    base,
                    offset,
                    member_type: Type::UnsignedLongLong,
                    index_stride: None,
                } = target.as_ref()
                else {
                    return None;
                };
                let Expression::Variable(global) = base.as_ref() else {
                    return None;
                };
                if !globals.contains_key(global) || volatile_globals.contains(global) {
                    return None;
                }
                let low_offset = offset.checked_add(4)?;
                *assignment_count += 1;
                output.push(Statement::Assign {
                    name: name.to_owned(),
                    value: value.as_ref().clone(),
                });
                output.push(Statement::Store {
                    target: Expression::Member {
                        base: base.clone(),
                        offset: low_offset,
                        member_type: Type::UnsignedInt,
                        index_stride: None,
                    },
                    value: Expression::Variable(name.to_owned()),
                });
                output.push(Statement::Store {
                    target: Expression::Member {
                        base: base.clone(),
                        offset: *offset,
                        member_type: Type::UnsignedInt,
                        index_stride: None,
                    },
                    value: Expression::IntegerLiteral(0),
                });
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => output.push(Statement::If {
                condition: condition.clone(),
                then_body: rewrite_statements(
                    then_body,
                    name,
                    globals,
                    volatile_globals,
                    call_return_types,
                    assignment_count,
                )?,
                else_body: rewrite_statements(
                    else_body,
                    name,
                    globals,
                    volatile_globals,
                    call_return_types,
                    assignment_count,
                )?,
            }),
            Statement::Loop {
                kind,
                initializer,
                condition,
                step,
                body,
            } => output.push(Statement::Loop {
                kind: *kind,
                initializer: initializer.clone(),
                condition: condition.clone(),
                step: step.clone(),
                body: rewrite_statements(
                    body,
                    name,
                    globals,
                    volatile_globals,
                    call_return_types,
                    assignment_count,
                )?,
            }),
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                let mut rewritten_arms = Vec::with_capacity(arms.len());
                for arm in arms {
                    let body = match &arm.body {
                        ArmBody::Return(value) => ArmBody::Return(value.clone()),
                        ArmBody::Statements(statements) => ArmBody::Statements(
                            rewrite_statements(
                                statements,
                                name,
                                globals,
                                volatile_globals,
                                call_return_types,
                                assignment_count,
                            )?,
                        ),
                    };
                    rewritten_arms.push(mwcc_syntax_trees::SwitchArm {
                        value: arm.value,
                        body,
                        falls_through: arm.falls_through,
                    });
                }
                let rewritten_default = match default {
                    Some(ArmBody::Return(value)) => Some(ArmBody::Return(value.clone())),
                    Some(ArmBody::Statements(statements)) => Some(ArmBody::Statements(
                        rewrite_statements(
                            statements,
                            name,
                            globals,
                            volatile_globals,
                            call_return_types,
                            assignment_count,
                        )?,
                    )),
                    None => None,
                };
                output.push(Statement::Switch {
                    scrutinee: scrutinee.clone(),
                    arms: rewritten_arms,
                    default: rewritten_default,
                });
            }
            _ => output.push(statement.clone()),
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::LocalDeclaration;

    #[test]
    fn scalarizes_a_zero_extended_call_used_only_for_low_masks() {
        let function = Function {
            return_type: Type::Void,
            name: "menu".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::UnsignedLongLong,
                name: "events".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![
                Statement::Assign {
                    name: "events".into(),
                    value: Expression::Assign {
                        target: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("menu_state".into())),
                            offset: 8,
                            member_type: Type::UnsignedLongLong,
                            index_stride: None,
                        }),
                        value: Box::new(Expression::Call {
                            name: "read_inputs".into(),
                            arguments: Vec::new(),
                        }),
                    },
                },
                Statement::If {
                    condition: Expression::Binary {
                        operator: BinaryOperator::BitAnd,
                        left: Box::new(Expression::Variable("events".into())),
                        right: Box::new(Expression::IntegerLiteral(0x20)),
                    },
                    then_body: Vec::new(),
                    else_body: Vec::new(),
                },
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let globals = HashMap::from([(
            "menu_state".into(),
            Type::Struct { size: 16, align: 4 },
        )]);
        let returns = HashMap::from([("read_inputs".into(), Type::UnsignedInt)]);

        let rewritten = scalarize_zero_extended_mask_local(
            &function,
            &globals,
            &HashSet::new(),
            &returns,
        )
        .expect("the low-word-only value should scalarize");
        assert_eq!(rewritten.locals[0].declared_type, Type::UnsignedInt);
        assert!(matches!(
            rewritten.statements.as_slice(),
            [
                Statement::Assign {
                    value: Expression::Call { .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Member {
                        offset: 12,
                        member_type: Type::UnsignedInt,
                        ..
                    },
                    value: Expression::Variable(low),
                },
                Statement::Store {
                    target: Expression::Member {
                        offset: 8,
                        member_type: Type::UnsignedInt,
                        ..
                    },
                    value: Expression::IntegerLiteral(0),
                },
                Statement::If { .. },
            ] if low == "events"
        ));
    }
}
