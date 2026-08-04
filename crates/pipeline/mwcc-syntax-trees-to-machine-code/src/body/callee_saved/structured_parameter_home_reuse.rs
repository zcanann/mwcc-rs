//! Lifetime-safe reuse of dead incoming-parameter homes by deferred locals.
//!
//! Structured frames initially reserve one saved home per cross-call parameter
//! and one per colored deferred-local group. MWCC colors both value classes in
//! one graph: after a parameter's final read, a later local definition may use
//! the same physical home. This plan composes those two independently proven
//! interval sets without coupling statement emission to source names.

use super::structured_locals::{
    structured_name_last_read, structured_name_occurs_in_loop,
    DeferredSavedHomePlan,
};
use super::structured_eager_home_reuse::StructuredEagerHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) struct StructuredParameterHomeReuse {
    home_index_by_group: Vec<usize>,
    pub(super) fresh_group_count: usize,
    pub(super) reuses_loop_exit_parameter_home: bool,
}

impl StructuredParameterHomeReuse {
    /// Preserve every recovered source home even when ordinary liveness proves
    /// that a deferred local could reuse an expired parameter lane.
    pub(super) fn retain_distinct(
        eager_count: usize,
        parameter_count: usize,
        deferred_group_count: usize,
    ) -> Self {
        Self {
            home_index_by_group: (0..deferred_group_count)
                .map(|group| eager_count + parameter_count + group)
                .collect(),
            fresh_group_count: deferred_group_count,
            reuses_loop_exit_parameter_home: false,
        }
    }

    pub(super) fn plan(
        function: &Function,
        eager_count: usize,
        saved_parameters: &[&Parameter],
        deferred: &DeferredSavedHomePlan,
        eager_reuse: &StructuredEagerHomeReuse,
    ) -> Self {
        let mut reused_parameter_by_group = vec![None; deferred.group_count];
        let mut reuses_loop_exit_parameter_home = false;
        let mut parameters: Vec<_> = saved_parameters
            .iter()
            .enumerate()
            .filter_map(|(index, parameter)| {
                let occurs_in_loop =
                    structured_name_occurs_in_loop(function, &parameter.name);
                (!occurs_in_loop
                    || (0..deferred.group_count).any(|group| {
                        deferred.members(group).any(|result| {
                            loop_exit_member_result_reuses_parameter(
                                function,
                                result,
                                &parameter.name,
                            )
                        })
                    }))
                .then_some((index, *parameter, occurs_in_loop))
            })
            .filter(|(_, parameter, _)| {
                function
                    .return_expression
                    .as_ref()
                    .is_none_or(|expression| {
                        !expression_reads_name(expression, &parameter.name)
                    })
            })
            .filter_map(|(index, parameter, occurs_in_loop)| {
                structured_name_last_read(function, &parameter.name)
                    .map(|last_read| {
                        (index, parameter.name.as_str(), last_read, occurs_in_loop)
                    })
            })
            .collect();
        // Ordinary expired parameters are unconstrained interval colors. Give
        // them first choice before the narrower loop-exit exception: otherwise
        // a loop-carried parameter with a later lexical read can steal the only
        // compatible result group from an already-expired parameter, producing
        // a valid but non-MWCC coloring.
        parameters.sort_by_key(|(_, _, last_read, occurs_in_loop)| {
            (*occurs_in_loop, std::cmp::Reverse(*last_read))
        });

        for (parameter, parameter_name, last_read, occurs_in_loop) in parameters {
            let reusable = (0..deferred.group_count)
                .filter(|group| reused_parameter_by_group[*group].is_none())
                .filter(|group| {
                    eager_reuse.home_index(*group).is_none()
                        || eager_reuse
                            .expired_last_read(*group)
                            .is_some_and(|eager_last_read| last_read > eager_last_read)
                })
                // A local assignment defines its result only after the entire
                // right-hand side has consumed the parameter. A final parameter
                // read and the local definition in the same statement therefore
                // have adjacent, non-overlapping live intervals.
                .filter(|group| {
                    deferred.first_assignment(*group) >= last_read
                        || (deferred.member_count(*group) == 1
                            && deferred.members(*group).any(|result| {
                                super::structured_if_else_member_reuse::function_member_select_reuses_parameter(
                                    function,
                                    result,
                                    parameter_name,
                                ) || loop_exit_member_result_reuses_parameter(
                                    function,
                                    result,
                                    parameter_name,
                                )
                            }))
                })
                .max_by_key(|group| deferred.first_assignment(*group));
            if let Some(group) = reusable {
                reuses_loop_exit_parameter_home |= occurs_in_loop
                    && deferred.members(group).any(|result| {
                        loop_exit_member_result_reuses_parameter(
                            function,
                            result,
                            parameter_name,
                        )
                    });
                reused_parameter_by_group[group] = Some(parameter);
            }
        }

        let mut fresh_group_count = 0;
        let home_index_by_group = reused_parameter_by_group
            .into_iter()
            .enumerate()
            .map(|(group, parameter)| {
                if let Some(parameter) = parameter {
                    eager_count + parameter
                } else if let Some(home) = eager_reuse.home_index(group) {
                    home
                } else {
                    let home = eager_count + saved_parameters.len() + fresh_group_count;
                    fresh_group_count += 1;
                    home
                }
            })
            .collect();
        Self {
            home_index_by_group,
            fresh_group_count,
            reuses_loop_exit_parameter_home,
        }
    }

    pub(super) fn home_index(&self, group: usize) -> usize {
        self.home_index_by_group[group]
    }

    pub(super) fn reuses_parameter_home(&self, eager_count: usize, parameter_count: usize) -> bool {
        let fresh_home_base = eager_count + parameter_count;
        self.home_index_by_group
            .iter()
            .any(|home| *home < fresh_home_base)
    }
}

fn loop_exit_member_result_reuses_parameter(
    function: &Function,
    result: &str,
    parameter: &str,
) -> bool {
    function
        .statements
        .iter()
        .enumerate()
        .any(|(loop_index, statement)| {
            let Statement::Loop { body, .. } = statement else {
                return false;
            };
            let owns_exit = loop_exit_assignment_count(body, result, parameter)
                .is_some_and(|count| count != 0);
            owns_exit
                && !function.statements[loop_index + 1..]
                    .iter()
                    .any(|statement| {
                        super::structured_liveness::statement_reads_name(statement, parameter)
                    })
                && !function.return_expression.as_ref().is_some_and(|expression| {
                    expression_reads_name(expression, parameter)
                })
        })
}

/// Count assignments that create `result` only on an immediate exit from this
/// loop. The result may overwrite the expired parameter home after either a
/// final member load or a constant selection. Any other assignment shape makes
/// the coalescing proof fail closed.
fn loop_exit_assignment_count(
    statements: &[Statement],
    result: &str,
    parameter: &str,
) -> Option<usize> {
    let mut count = 0;
    for (index, statement) in statements.iter().enumerate() {
        match statement {
            Statement::Assign { name, value } if name == result => {
                let exit_follows = matches!(statements.get(index + 1), Some(Statement::Break));
                let safe_value = matches!(value, Expression::IntegerLiteral(_))
                    || matches!(value, Expression::Member { base, .. }
                        if matches!(base.as_ref(), Expression::Variable(name) if name == parameter));
                if !exit_follows || !safe_value {
                    return None;
                }
                count += 1;
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                count += loop_exit_assignment_count(then_body, result, parameter)?;
                count += loop_exit_assignment_count(else_body, result, parameter)?;
            }
            // A `Break` in a nested loop does not exit the loop whose
            // parameter home we are considering.
            Statement::Loop { .. } => return None,
            _ => {}
        }
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::super::structured_eager_home_reuse::StructuredEagerHomeReuse;
    use super::super::structured_locals::plan_deferred_saved_homes;
    use super::*;

    fn function(return_reads_parameter: bool) -> Function {
        Function {
            return_type: Type::Int,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "incoming".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "late".into(),
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
            statements: vec![
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("incoming".into())],
                }),
                Statement::Assign {
                    name: "late".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("late".into())],
                }),
            ],
            guards: Vec::new(),
            return_expression: return_reads_parameter
                .then(|| Expression::Variable("incoming".into())),
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
    fn retains_recovered_deferred_homes_after_parameter_lanes() {
        let plan = StructuredParameterHomeReuse::retain_distinct(1, 2, 3);

        assert_eq!(plan.fresh_group_count, 3);
        assert_eq!(
            (0..3).map(|group| plan.home_index(group)).collect::<Vec<_>>(),
            vec![3, 4, 5]
        );
        assert!(!plan.reuses_parameter_home(1, 2));
        assert!(!plan.reuses_loop_exit_parameter_home);
    }

    #[test]
    fn reuses_a_parameter_home_after_its_final_read() {
        let function = function(false);
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 0);
    }

    #[test]
    fn keeps_a_parameter_home_live_when_the_return_reads_it() {
        let function = function(true);
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 1);
        assert_eq!(reuse.home_index(deferred.group("late")), 1);
    }

    #[test]
    fn reuses_a_parameter_consumed_by_the_defining_call() {
        let mut function = function(false);
        function.statements = vec![
            Statement::Assign {
                name: "late".into(),
                value: Expression::Call {
                    name: "produce".into(),
                    arguments: vec![Expression::Variable("incoming".into())],
                },
            },
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("late".into())],
            }),
        ];
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 0);
    }

    #[test]
    fn reuses_a_parameter_home_for_a_false_edge_member_select() {
        let mut function = function(false);
        let member = Expression::Member {
            base: Box::new(Expression::Variable("incoming".into())),
            offset: 12,
            member_type: Type::Int,
            index_stride: None,
        };
        function.statements = vec![
            Statement::Expression(Expression::Call {
                name: "before".into(),
                arguments: Vec::new(),
            }),
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(member.clone()),
                    right: Box::new(Expression::IntegerLiteral(3)),
                },
                then_body: vec![Statement::Assign {
                    name: "late".into(),
                    value: Expression::IntegerLiteral(1),
                }],
                else_body: vec![Statement::Assign {
                    name: "late".into(),
                    value: member,
                }],
            },
            Statement::Expression(Expression::Call {
                name: "after".into(),
                arguments: vec![Expression::Variable("late".into())],
            }),
        ];
        function.return_expression = Some(Expression::Variable("late".into()));
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 0);
    }

    #[test]
    fn prefers_the_most_recently_expired_parameter_over_an_eager_home() {
        let mut function = function(false);
        function.locals.insert(
            0,
            LocalDeclaration {
                declared_type: Type::Int,
                name: "eager".into(),
                initializer: Some(Expression::IntegerLiteral(7)),
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            },
        );
        function.statements.insert(
            0,
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("eager".into())],
            }),
        );
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[1]]).unwrap();
        let eager_reuse =
            StructuredEagerHomeReuse::plan(&function, &[&function.locals[0]], &deferred);
        let late_group = deferred.group("late");
        assert_eq!(eager_reuse.home_index(late_group), Some(0));

        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            1,
            &[&function.parameters[0]],
            &deferred,
            &eager_reuse,
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(late_group), 1);
    }

    #[test]
    fn keeps_a_parameter_read_on_each_loop_iteration_live() {
        let mut function = function(false);
        function.statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: vec![
                Statement::Expression(Expression::Variable("incoming".into())),
                Statement::Assign {
                    name: "late".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("late".into())],
                }),
            ],
        }];
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let eager_reuse = StructuredEagerHomeReuse::plan(&function, &[], &deferred);

        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &eager_reuse,
        );

        assert_eq!(reuse.fresh_group_count, 1);
        assert_eq!(reuse.home_index(deferred.group("late")), 1);
    }

    #[test]
    fn reuses_a_parameter_consumed_by_a_loop_exit_member_load() {
        let mut function = function(false);
        function.statements = vec![
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: vec![Statement::If {
                    condition: Expression::Variable("finished".into()),
                    then_body: vec![
                        Statement::Assign {
                            name: "late".into(),
                            value: Expression::Member {
                                base: Box::new(Expression::Variable("incoming".into())),
                                offset: 32,
                                member_type: Type::Int,
                                index_stride: None,
                            },
                        },
                        Statement::Break,
                    ],
                    else_body: Vec::new(),
                }],
            },
            Statement::Expression(Expression::Call {
                name: "restore".into(),
                arguments: Vec::new(),
            }),
        ];
        function.return_expression = Some(Expression::Variable("late".into()));
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 0);
        assert!(reuse.reuses_loop_exit_parameter_home);
    }

    #[test]
    fn reuses_a_parameter_across_nested_constant_loop_exits() {
        let mut function = function(false);
        function.statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: vec![Statement::If {
                condition: Expression::Variable("finished".into()),
                then_body: vec![
                    Statement::Assign {
                        name: "late".into(),
                        value: Expression::IntegerLiteral(0),
                    },
                    Statement::Break,
                ],
                else_body: vec![Statement::If {
                    condition: Expression::Variable("failed".into()),
                    then_body: vec![
                        Statement::Assign {
                            name: "late".into(),
                            value: Expression::IntegerLiteral(-1),
                        },
                        Statement::Break,
                    ],
                    else_body: vec![Statement::Expression(Expression::Member {
                        base: Box::new(Expression::Variable("incoming".into())),
                        offset: 12,
                        member_type: Type::Int,
                        index_stride: None,
                    })],
                }],
            }],
        }];
        function.return_expression = Some(Expression::Variable("late".into()));
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 0);
        assert!(reuse.reuses_loop_exit_parameter_home);
    }

    #[test]
    fn prefers_an_ordinary_expired_parameter_to_a_loop_exit_exception() {
        let mut function = function(false);
        function.parameters.push(Parameter {
            parameter_type: Type::Int,
            name: "early".into(),
        });
        function.statements = vec![
            Statement::Expression(Expression::Call {
                name: "consume_early".into(),
                arguments: vec![Expression::Variable("early".into())],
            }),
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: vec![
                    Statement::Expression(Expression::Member {
                        base: Box::new(Expression::Variable("incoming".into())),
                        offset: 12,
                        member_type: Type::Int,
                        index_stride: None,
                    }),
                    Statement::If {
                        condition: Expression::Variable("finished".into()),
                        then_body: vec![
                            Statement::Assign {
                                name: "late".into(),
                                value: Expression::IntegerLiteral(0),
                            },
                            Statement::Break,
                        ],
                        else_body: Vec::new(),
                    },
                ],
            },
        ];
        function.return_expression = Some(Expression::Variable("late".into()));
        let deferred = plan_deferred_saved_homes(&function, &[&function.locals[0]]).unwrap();
        let reuse = StructuredParameterHomeReuse::plan(
            &function,
            0,
            &[&function.parameters[0], &function.parameters[1]],
            &deferred,
            &StructuredEagerHomeReuse::plan(&function, &[], &deferred),
        );

        assert_eq!(reuse.fresh_group_count, 0);
        assert_eq!(reuse.home_index(deferred.group("late")), 1);
        assert!(!reuse.reuses_loop_exit_parameter_home);
    }
}
