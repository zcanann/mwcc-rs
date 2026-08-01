//! A repeated scalar-global member address retained across a possible call.
//!
//! Build 163 can keep `&global.member` in a callee-saved register when the same
//! member is read on both sides of a call. Other members of the aggregate still
//! materialize their own base, so this is deliberately separate from the
//! call-free whole-aggregate base cache.

use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement, Type};

use super::structured_expression_visit::visit_expression;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalMemberAddressPlan {
    pub(super) global: String,
    pub(super) total_size: u32,
    pub(super) offset: i16,
    pub(super) defer_until_first_use: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Member {
        global: String,
        offset: u32,
        inside_loop: bool,
    },
    Call,
}

#[cfg(test)]
pub(super) fn plan(
    function: &Function,
    addressable_globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<StructuredGlobalMemberAddressPlan> {
    plans(function, addressable_globals, global_array_sizes)
        .into_iter()
        .next()
}

pub(super) fn plans(
    function: &Function,
    addressable_globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Vec<StructuredGlobalMemberAddressPlan> {
    let struct_sizes = addressable_globals
        .iter()
        .filter_map(|(name, declared_type)| match declared_type {
            Type::Struct { size, .. } if !global_array_sizes.contains_key(name) => {
                Some((name.clone(), u32::from(*size)))
            }
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut events = Vec::new();
    for initializer in function
        .locals
        .iter()
        .filter_map(|local| local.initializer.as_ref())
    {
        collect_expression_events(initializer, &struct_sizes, false, &mut events);
    }
    collect_statement_events(&function.statements, &struct_sizes, false, &mut events);
    let mut positions =
        std::collections::HashMap::<(String, u32), Vec<(usize, bool)>>::new();
    for (position, event) in events.iter().enumerate() {
        if let Event::Member {
            global,
            offset,
            inside_loop,
        } = event
        {
            positions
                .entry((global.clone(), *offset))
                .or_default()
                .push((position, *inside_loop));
        }
    }

    let mut candidates = positions
        .into_iter()
        .filter_map(|((global, offset), occurrences)| {
            let (first, _) = *occurrences.first()?;
            let (last, _) = *occurrences.last()?;
            if occurrences.len() < 2 || first >= last {
                return None;
            }
            let call_between = events[first + 1..last]
                .iter()
                .any(|event| matches!(event, Event::Call));
            call_between
                .then(|| {
                    Some((
                        occurrences.len(),
                        StructuredGlobalMemberAddressPlan {
                            total_size: struct_sizes[&global],
                            global,
                            offset: i16::try_from(offset).ok()?,
                            defer_until_first_use: events[..first]
                                .iter()
                                .any(|event| matches!(event, Event::Call)),
                        },
                    ))
                })
                .flatten()
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|(left_count, left), (right_count, right)| {
        right_count
            .cmp(left_count)
            .then_with(|| left.global.cmp(&right.global))
            .then_with(|| left.offset.cmp(&right.offset))
    });
    candidates.into_iter().map(|(_, plan)| plan).collect()
}

/// A retained whole-aggregate base subsumes the colder member-address live
/// ranges. MWCC still gives the hottest repeated member its own address home,
/// which shortens the loop dependency chain, but accesses every other member
/// directly from the shared base.
pub(super) fn retain_hottest_for_cached_global(
    plans: &mut Vec<StructuredGlobalMemberAddressPlan>,
    cached_global: Option<&str>,
) -> bool {
    let Some(cached_global) = cached_global else {
        return false;
    };
    // Small member sets retain independent addresses; only a broad aggregate
    // fanout makes the shared base the cheaper representation.
    if plans
        .iter()
        .filter(|plan| plan.global == cached_global)
        .count()
        < 4
    {
        return false;
    }
    let selected_offset = plans
        .iter()
        .find(|plan| plan.global == cached_global && !plan.defer_until_first_use)
        .or_else(|| plans.iter().find(|plan| plan.global == cached_global))
        .map(|plan| plan.offset)
        .expect("the broad cached-global member set is nonempty");
    plans.retain(|plan| {
        if plan.global != cached_global {
            return true;
        }
        plan.offset == selected_offset
    });
    true
}

fn collect_statement_events(
    statements: &[Statement],
    struct_sizes: &std::collections::HashMap<String, u32>,
    inside_loop: bool,
    events: &mut Vec<Event>,
) {
    for statement in statements {
        match statement {
            Statement::Store { target, value } => {
                collect_expression_events(target, struct_sizes, inside_loop, events);
                collect_expression_events(value, struct_sizes, inside_loop, events);
            }
            Statement::Assign { value, .. } | Statement::Expression(value) => {
                collect_expression_events(value, struct_sizes, inside_loop, events);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                collect_expression_events(condition, struct_sizes, inside_loop, events);
                collect_statement_events(then_body, struct_sizes, inside_loop, events);
                collect_statement_events(else_body, struct_sizes, inside_loop, events);
            }
            Statement::Return(value) => {
                if let Some(value) = value {
                    collect_expression_events(value, struct_sizes, inside_loop, events);
                }
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                collect_expression_events(scrutinee, struct_sizes, inside_loop, events);
                for arm in arms {
                    collect_arm_events(&arm.body, struct_sizes, inside_loop, events);
                }
                if let Some(default) = default {
                    collect_arm_events(default, struct_sizes, inside_loop, events);
                }
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                for expression in [initializer, condition, step].into_iter().flatten() {
                    collect_expression_events(expression, struct_sizes, true, events);
                }
                collect_statement_events(body, struct_sizes, true, events);
            }
            Statement::InlineAsm(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => {}
        }
    }
}

fn collect_expression_events(
    expression: &Expression,
    struct_sizes: &std::collections::HashMap<String, u32>,
    inside_loop: bool,
    events: &mut Vec<Event>,
) {
    visit_expression(expression, &mut |expression| {
        let Expression::Member {
            base,
            offset,
            index_stride: None,
            ..
        } = expression
        else {
            return;
        };
        let Expression::Variable(global) = base.as_ref() else {
            return;
        };
        if struct_sizes.contains_key(global) {
            events.push(Event::Member {
                global: global.clone(),
                offset: *offset,
                inside_loop,
            });
        }
    });
    if crate::analysis::expression_has_call(expression) {
        events.push(Event::Call);
    }
}

fn collect_arm_events(
    body: &ArmBody,
    struct_sizes: &std::collections::HashMap<String, u32>,
    inside_loop: bool,
    events: &mut Vec<Event>,
) {
    match body {
        ArmBody::Return(expression) => {
            collect_expression_events(expression, struct_sizes, inside_loop, events)
        }
        ArmBody::Statements(statements) => {
            collect_statement_events(statements, struct_sizes, inside_loop, events)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, LocalDeclaration, LoopKind, Statement};

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("record".into())),
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "f".into(),
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
    fn retains_only_the_repeated_member_address_across_the_call() {
        let function = function(vec![
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("limit".into())),
                    right: Box::new(member(8)),
                },
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "panic".into(),
                    arguments: Vec::new(),
                })],
                else_body: Vec::new(),
            },
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![member(8), member(4)],
            }),
        ]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalMemberAddressPlan {
                global: "record".into(),
                total_size: 12,
                offset: 8,
                defer_until_first_use: false,
            })
        );
    }

    #[test]
    fn rejects_repetition_without_an_intervening_call() {
        let function = function(vec![Statement::Assign {
            name: "sum".into(),
            value: Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(member(8)),
                right: Box::new(member(8)),
            },
        }]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            None
        );
    }

    #[test]
    fn rejects_a_repeated_member_inside_a_loop() {
        let function = function(vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::Variable("running".into())),
            step: None,
            body: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![member(8), member(8)],
            })],
        }]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            None
        );
    }

    #[test]
    fn retains_a_loop_member_when_a_call_separates_its_uses() {
        let function = function(vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(member(8)),
            step: None,
            body: vec![
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: Vec::new(),
                }),
                Statement::Assign {
                    name: "later".into(),
                    value: member(8),
                },
            ],
        }]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalMemberAddressPlan {
                global: "record".into(),
                total_size: 12,
                offset: 8,
                defer_until_first_use: false,
            })
        );
    }

    #[test]
    fn returns_every_member_address_worth_retaining() {
        let function = function(vec![
            Statement::Assign {
                name: "first".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(member(8)),
                    right: Box::new(member(4)),
                },
            },
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: Vec::new(),
            }),
            Statement::Assign {
                name: "later".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(member(8)),
                    right: Box::new(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(member(8)),
                        right: Box::new(member(4)),
                    }),
                },
            },
        ]);

        assert_eq!(
            plans(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            )
            .into_iter()
            .map(|plan| plan.offset)
            .collect::<Vec<_>>(),
            [8, 4]
        );
    }

    #[test]
    fn retains_a_preloop_member_address_reused_by_the_loop_condition() {
        let function = function(vec![
            Statement::Assign {
                name: "initial".into(),
                value: member(8),
            },
            Statement::Expression(Expression::Call {
                name: "prepare".into(),
                arguments: Vec::new(),
            }),
            Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::Less,
                    left: Box::new(Expression::Variable("index".into())),
                    right: Box::new(member(8)),
                }),
                step: None,
                body: Vec::new(),
            },
        ]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalMemberAddressPlan {
                global: "record".into(),
                total_size: 12,
                offset: 8,
                defer_until_first_use: false,
            })
        );
    }

    #[test]
    fn retains_an_initializer_member_address_across_a_loop_call() {
        let mut function = function(vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(member(8)),
            step: None,
            body: vec![
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: Vec::new(),
                }),
                Statement::Assign {
                    name: "later".into(),
                    value: member(8),
                },
            ],
        }]);
        function.locals.push(LocalDeclaration {
            declared_type: Type::UnsignedInt,
            name: "initial".into(),
            initializer: Some(member(8)),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalMemberAddressPlan {
                global: "record".into(),
                total_size: 12,
                offset: 8,
                defer_until_first_use: false,
            })
        );
    }

    #[test]
    fn retains_a_member_across_a_nested_optional_call() {
        let function = function(vec![
            Statement::Expression(Expression::Call {
                name: "prepare".into(),
                arguments: Vec::new(),
            }),
            Statement::If {
                condition: Expression::Variable("enabled".into()),
                then_body: vec![
                    Statement::If {
                        condition: Expression::Binary {
                            operator: BinaryOperator::Equal,
                            left: Box::new(Expression::Variable("limit".into())),
                            right: Box::new(member(8)),
                        },
                        then_body: vec![Statement::Expression(Expression::Call {
                            name: "panic".into(),
                            arguments: Vec::new(),
                        })],
                        else_body: Vec::new(),
                    },
                    Statement::Expression(Expression::Call {
                        name: "consume".into(),
                        arguments: vec![member(8), member(4)],
                    }),
                ],
                else_body: Vec::new(),
            },
        ]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "record".into(),
                    Type::Struct { size: 12, align: 4 },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalMemberAddressPlan {
                global: "record".into(),
                total_size: 12,
                offset: 8,
                defer_until_first_use: true,
            })
        );
    }
}
