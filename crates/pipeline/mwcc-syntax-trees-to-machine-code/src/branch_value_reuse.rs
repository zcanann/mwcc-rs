//! Materialize values that MWCC carries from a branch test into its body.
//!
//! Inline expansion can expose a canonical guarded update:
//! `if (global.member != 0) { global.member--; ... }`. MWCC loads the member
//! once, tests that register, and derives the stored value from the same
//! register. Represent that live range explicitly in the AST so structured
//! register planning does not reload the global on the true edge.

use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, LocalDeclaration, Statement, Type,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn materialize_guarded_global_member_update(
    function: &Function,
    globals: &HashMap<String, Type>,
) -> Option<Function> {
    let mut rewritten = function.clone();
    let occupied: HashSet<&str> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .collect();

    for statement in &mut rewritten.statements {
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = statement
        else {
            continue;
        };
        if !else_body.is_empty() {
            continue;
        }
        let Some(condition_member) = zero_compared_global_member(condition, globals) else {
            continue;
        };
        if !is_matching_unit_update(then_body.first(), &condition_member) {
            continue;
        }

        let local_name = unique_cache_name(&occupied);
        replace_compared_member(condition, &condition_member, &local_name);
        rewrite_unit_update(
            then_body.first_mut().expect("matched first statement"),
            &local_name,
        );
        rewritten.locals.push(LocalDeclaration {
            declared_type: register_value_type(condition_member.member_type),
            name: local_name,
            initializer: Some(condition_member.expression()),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        return Some(rewritten);
    }
    None
}

fn register_value_type(storage_type: Type) -> Type {
    match storage_type {
        Type::Char | Type::Short => Type::Int,
        Type::UnsignedChar | Type::UnsignedShort => Type::UnsignedInt,
        _ => storage_type,
    }
}

#[derive(Clone)]
struct DirectMember {
    global: String,
    offset: u32,
    member_type: Type,
    index_stride: Option<u32>,
}

impl DirectMember {
    fn expression(&self) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(self.global.clone())),
            offset: self.offset,
            member_type: self.member_type,
            index_stride: self.index_stride,
        }
    }
}

fn zero_compared_global_member(
    condition: &Expression,
    globals: &HashMap<String, Type>,
) -> Option<DirectMember> {
    let Expression::Binary {
        operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(right.as_ref(), Expression::IntegerLiteral(0)) {
        return None;
    }
    let operand = match left.as_ref() {
        Expression::Cast { operand, .. } => operand.as_ref(),
        operand => operand,
    };
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride,
    } = operand
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    globals.contains_key(global).then(|| DirectMember {
        global: global.clone(),
        offset: *offset,
        member_type: *member_type,
        index_stride: *index_stride,
    })
}

fn same_direct_member(expression: &Expression, expected: &DirectMember) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } if *offset == expected.offset
            && *member_type == expected.member_type
            && *index_stride == expected.index_stride
            && matches!(base.as_ref(), Expression::Variable(name) if name == &expected.global)
    )
}

fn is_matching_unit_update(
    statement: Option<&Statement>,
    expected: &DirectMember,
) -> bool {
    match statement {
        Some(Statement::Expression(Expression::PostStep {
            target,
            operator,
            pointer_link: None,
        })) => {
            matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                && same_direct_member(target, expected)
        }
        Some(Statement::Store {
            target,
            value:
                Expression::Binary {
                    operator,
                    left,
                    right,
                },
        }) => {
            matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                && same_direct_member(target, expected)
                && same_direct_member(left, expected)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1))
        }
        _ => false,
    }
}

fn rewrite_unit_update(statement: &mut Statement, local_name: &str) {
    match statement {
        Statement::Expression(Expression::PostStep {
            target,
            operator,
            pointer_link: None,
        }) => {
            let stored_target = target.as_ref().clone();
            *statement = Statement::Store {
                target: stored_target,
                value: Expression::Binary {
                    operator: *operator,
                    left: Box::new(Expression::Variable(local_name.to_owned())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                },
            };
        }
        Statement::Store {
            value: Expression::Binary { left, .. },
            ..
        } => {
            *left = Box::new(Expression::Variable(local_name.to_owned()));
        }
        _ => unreachable!("the statement was recognized as a unit update"),
    }
}

fn replace_compared_member(
    condition: &mut Expression,
    expected: &DirectMember,
    local_name: &str,
) {
    let Expression::Binary { left, .. } = condition else {
        unreachable!("the condition was recognized as a comparison");
    };
    match left.as_mut() {
        Expression::Cast {
            target_type,
            operand,
        } => {
            debug_assert!(same_direct_member(operand, expected));
            if *target_type == expected.member_type {
                *left = Box::new(Expression::Variable(local_name.to_owned()));
            } else {
                **operand = Expression::Variable(local_name.to_owned());
            }
        }
        operand => {
            debug_assert!(same_direct_member(operand, expected));
            *operand = Expression::Variable(local_name.to_owned());
        }
    }
}

fn unique_cache_name(occupied: &HashSet<&str>) -> String {
    for ordinal in 0usize.. {
        let candidate = format!("__mwcc_branch_value_{ordinal}");
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the finite function cannot occupy every generated name")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Parameter;

    #[test]
    fn materializes_a_guarded_global_member_post_decrement() {
        let member = || Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset: 0,
            member_type: Type::UnsignedShort,
            index_stride: None,
        };
        let function = Function {
            return_type: Type::Void,
            name: "tick".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "__mwcc_branch_value_0".into(),
            }],
            locals: Vec::new(),
            statements: vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: Box::new(Expression::Cast {
                        target_type: Type::UnsignedShort,
                        operand: Box::new(member()),
                    }),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![Statement::Expression(Expression::PostStep {
                    target: Box::new(member()),
                    operator: BinaryOperator::Subtract,
                    pointer_link: None,
                })],
                else_body: Vec::new(),
            }],
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
            "state".into(),
            Type::Struct { size: 8, align: 4 },
        )]);

        let rewritten = materialize_guarded_global_member_update(&function, &globals)
            .expect("the guarded update should be materialized");
        assert_eq!(rewritten.locals.len(), 1);
        assert_eq!(rewritten.locals[0].name, "__mwcc_branch_value_1");
        assert_eq!(rewritten.locals[0].declared_type, Type::UnsignedInt);
        assert!(matches!(
            rewritten.statements.as_slice(),
            [Statement::If {
                condition: Expression::Binary { left, .. },
                then_body,
                ..
            }] if matches!(
                left.as_ref(),
                Expression::Variable(name) if name == "__mwcc_branch_value_1"
            ) && matches!(
                then_body.as_slice(),
                [Statement::Store {
                    value: Expression::Binary { left, right, .. },
                    ..
                }] if matches!(
                    left.as_ref(),
                    Expression::Variable(name) if name == "__mwcc_branch_value_1"
                ) && matches!(right.as_ref(), Expression::IntegerLiteral(1))
            )
        ));
    }
}
