//! Make shared global-aggregate store bases explicit in structured ASTs.
//!
//! MWCC materializes one address per consecutive cluster of stores into the
//! same global struct. Hygienic pointer locals expose those disjoint live
//! ranges to the ordinary structured allocator and fix each activation point
//! after any preceding call.

use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};
use std::collections::{HashMap, HashSet};

pub(crate) fn materialize_consecutive_global_struct_store_base(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<Function> {
    let mut rewritten = function.clone();
    let mut changed = false;
    loop {
        let occupied: HashSet<_> = rewritten
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .chain(rewritten.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let local_name = unique_base_name(&occupied);
        let Some((global, size)) = materialize_first_cluster(
            &mut rewritten.statements,
            &local_name,
            globals,
            volatile_globals,
        ) else {
            break;
        };
        debug_assert!(globals.contains_key(&global));
        rewritten.locals.push(LocalDeclaration {
            declared_type: Type::StructPointer { element_size: size },
            name: local_name,
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
        changed = true;
    }
    changed.then_some(rewritten)
}

fn materialize_first_cluster(
    statements: &mut Vec<Statement>,
    local_name: &str,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<(String, u32)> {
    let mut start = 0usize;
    while start < statements.len() {
        if let Some(global) =
            direct_global_struct_store(&statements[start], globals, volatile_globals)
        {
            let end = statements[start..]
                .iter()
                .take_while(|statement| {
                    direct_global_struct_store(statement, globals, volatile_globals)
                        .as_deref()
                        == Some(global.as_str())
                })
                .count()
                + start;
            let Type::Struct { size, .. } = globals[&global] else {
                unreachable!("the store recognizer selected a struct global");
            };
            let shared_start = if end - start >= 3
                && size <= 8
                && leading_computed_offset_zero_store(&statements[start])
            {
                start + 1
            } else {
                start
            };
            if end - shared_start >= 2 {
                statements.insert(
                    shared_start,
                    Statement::Assign {
                        name: local_name.to_owned(),
                        value: Expression::AddressOf {
                            operand: Box::new(Expression::Variable(global.clone())),
                        },
                    },
                );
                for statement in &mut statements[shared_start + 1..=end] {
                    let Statement::Store {
                        target: Expression::Member { base, .. },
                        ..
                    } = statement
                    else {
                        unreachable!("the selected cluster contains only member stores");
                    };
                    *base = Box::new(Expression::Variable(local_name.to_owned()));
                }
                return Some((global, size));
            }
        }

        let nested = match &mut statements[start] {
            Statement::If {
                then_body,
                else_body,
                ..
            } => materialize_first_cluster(
                then_body,
                local_name,
                globals,
                volatile_globals,
            )
            .or_else(|| {
                materialize_first_cluster(
                    else_body,
                    local_name,
                    globals,
                    volatile_globals,
                )
            }),
            Statement::Loop { body, .. } => {
                materialize_first_cluster(body, local_name, globals, volatile_globals)
            }
            Statement::Switch { arms, default, .. } => {
                let in_arm = arms.iter_mut().find_map(|arm| match &mut arm.body {
                    mwcc_syntax_trees::ArmBody::Statements(body) => {
                        materialize_first_cluster(
                            body,
                            local_name,
                            globals,
                            volatile_globals,
                        )
                    }
                    mwcc_syntax_trees::ArmBody::Return(_) => None,
                });
                in_arm.or_else(|| match default {
                    Some(mwcc_syntax_trees::ArmBody::Statements(body)) => {
                        materialize_first_cluster(
                            body,
                            local_name,
                            globals,
                            volatile_globals,
                        )
                    }
                    Some(mwcc_syntax_trees::ArmBody::Return(_)) | None => None,
                })
            }
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
        start += 1;
    }
    None
}

fn leading_computed_offset_zero_store(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Store {
            target: Expression::Member { offset: 0, .. },
            value,
        } if !matches!(value, Expression::IntegerLiteral(_))
    )
}

fn direct_global_struct_store(
    statement: &Statement,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<String> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                index_stride: None,
                ..
            },
        ..
    } = statement
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    (matches!(globals.get(global), Some(Type::Struct { .. }))
        && !volatile_globals.contains(global))
    .then(|| global.clone())
}

fn unique_base_name(occupied: &HashSet<&str>) -> String {
    for ordinal in 0usize.. {
        let candidate = format!("__mwcc_global_store_base_{ordinal}");
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the finite function cannot occupy every generated name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "publish".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
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
    fn materializes_one_base_at_the_consecutive_store_cluster() {
        let store = |offset| Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: Expression::IntegerLiteral(0),
        };
        let function = function(vec![
            Statement::Expression(Expression::Call {
                name: "read".into(),
                arguments: Vec::new(),
            }),
            store(12),
            store(8),
        ]);
        let globals =
            HashMap::from([("state".into(), Type::Struct { size: 16, align: 4 })]);

        let rewritten = materialize_consecutive_global_struct_store_base(
            &function,
            &globals,
            &HashSet::new(),
        )
        .expect("the two stores should share one base");
        assert_eq!(rewritten.locals.len(), 1);
        assert_eq!(rewritten.locals[0].name, "__mwcc_global_store_base_0");
        assert!(matches!(
            rewritten.statements.as_slice(),
            [
                Statement::Expression(Expression::Call { .. }),
                Statement::Assign {
                    value: Expression::AddressOf { .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Member { base: first, .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Member { base: second, .. },
                    ..
                },
            ] if matches!(
                first.as_ref(),
                Expression::Variable(name) if name == "__mwcc_global_store_base_0"
            ) && matches!(
                second.as_ref(),
                Expression::Variable(name) if name == "__mwcc_global_store_base_0"
            )
        ));
    }

    #[test]
    fn nests_the_base_after_a_foldable_leading_store() {
        let store = |offset, value| Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value,
        };
        let function = function(vec![Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![
                store(
                    0,
                    Expression::Binary {
                        operator: mwcc_syntax_trees::BinaryOperator::Subtract,
                        left: Box::new(Expression::Variable("old".into())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    },
                ),
                store(2, Expression::IntegerLiteral(0)),
                store(4, Expression::IntegerLiteral(0)),
            ],
            else_body: Vec::new(),
        }]);
        let globals =
            HashMap::from([("state".into(), Type::Struct { size: 8, align: 4 })]);

        let rewritten = materialize_consecutive_global_struct_store_base(
            &function,
            &globals,
            &HashSet::new(),
        )
        .expect("the trailing stores should share one nested base");
        assert!(matches!(
            rewritten.statements.as_slice(),
            [Statement::If { then_body, .. }] if matches!(
                then_body.as_slice(),
                [
                    Statement::Store {
                        target: Expression::Member { base: first, .. },
                        ..
                    },
                    Statement::Assign { .. },
                    Statement::Store {
                        target: Expression::Member { base: second, .. },
                        ..
                    },
                    Statement::Store {
                        target: Expression::Member { base: third, .. },
                        ..
                    },
                ] if matches!(
                    first.as_ref(),
                    Expression::Variable(name) if name == "state"
                ) && matches!(
                    second.as_ref(),
                    Expression::Variable(name) if name == "__mwcc_global_store_base_0"
                ) && matches!(
                    third.as_ref(),
                    Expression::Variable(name) if name == "__mwcc_global_store_base_0"
                )
            )
        ));
    }

    #[test]
    fn materializes_every_disjoint_cluster() {
        let store = |offset| Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: Expression::IntegerLiteral(0),
        };
        let function = function(vec![
            Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: vec![store(0), store(4)],
                else_body: Vec::new(),
            },
            Statement::Expression(Expression::Call {
                name: "separate".into(),
                arguments: Vec::new(),
            }),
            store(8),
            store(12),
        ]);
        let globals =
            HashMap::from([("state".into(), Type::Struct { size: 16, align: 4 })]);

        let rewritten = materialize_consecutive_global_struct_store_base(
            &function,
            &globals,
            &HashSet::new(),
        )
        .expect("both disjoint store clusters should receive bases");

        assert_eq!(
            rewritten
                .locals
                .iter()
                .map(|local| local.name.as_str())
                .collect::<Vec<_>>(),
            [
                "__mwcc_global_store_base_0",
                "__mwcc_global_store_base_1"
            ]
        );
        assert!(matches!(
            rewritten.statements.as_slice(),
            [
                Statement::If { then_body, .. },
                Statement::Expression(_),
                Statement::Assign { name, .. },
                Statement::Store { .. },
                Statement::Store { .. },
            ] if name == "__mwcc_global_store_base_1"
                && matches!(
                    then_body.as_slice(),
                    [
                        Statement::Assign { name, .. },
                        Statement::Store { .. },
                        Statement::Store { .. },
                    ] if name == "__mwcc_global_store_base_0"
                )
        ));
    }
}
