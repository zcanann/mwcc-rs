//! One shared address live range for repeated global aggregate member loads.
//!
//! MWCC materializes a large global array's address once for either a leading
//! cluster or a cluster that begins before a call and continues afterward. The
//! allocator naturally colors the former into a volatile register and the
//! latter into a callee-saved register.

use crate::generator::Generator;
use mwcc_syntax_trees::{Expression, Function, Statement, Type};

use super::structured_expression_visit::{visit_expression, visit_statement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalBasePlan {
    pub(super) global: String,
    pub(super) total_size: u32,
    /// The shared address is referenced again after the first call and therefore
    /// needs a declared callee-saved home, not merely an allocator preference.
    pub(super) crosses_call: bool,
    /// Source accesses on the function's linear statement spine. Nested arm
    /// bodies begin distinct address live ranges and rematerialize their base.
    pub(super) use_count: usize,
}

pub(super) fn plan(
    function: &Function,
    addressable_globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<StructuredGlobalBasePlan> {
    let global_size = |name: &str| {
        global_array_sizes.get(name).copied().or_else(|| {
            match addressable_globals.get(name) {
                Some(Type::Struct { size, .. }) => u32::try_from(*size).ok(),
                _ => None,
            }
        })
    };
    fn collect(
        expression: &Expression,
        addressable_globals: &std::collections::HashMap<String, Type>,
        global_array_sizes: &std::collections::HashMap<String, u32>,
        occurrences: &mut std::collections::HashMap<String, usize>,
    ) {
        let Expression::Member { base, .. } = expression else {
            return;
        };
        let global = match base.as_ref() {
            Expression::Variable(global) => Some(global),
            Expression::Index { base, index }
                if matches!(index.as_ref(), Expression::IntegerLiteral(_)) =>
            {
                match base.as_ref() {
                    Expression::Variable(global) => Some(global),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(global) = global.filter(|name| {
            global_array_sizes.contains_key(*name)
                || matches!(addressable_globals.get(*name), Some(Type::Struct { .. }))
        }) {
            *occurrences.entry(global.clone()).or_default() += 1;
        }
    }

    fn visit_linear_statement(statement: &Statement, visit: &mut impl FnMut(&Expression)) {
        match statement {
            Statement::Store { target, value } => {
                visit_expression(target, visit);
                visit_expression(value, visit);
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => visit_expression(value, visit),
            Statement::If { condition, .. } => visit_expression(condition, visit),
            Statement::Switch { scrutinee, .. } => visit_expression(scrutinee, visit),
            Statement::Loop {
                initializer,
                condition,
                step,
                ..
            } => {
                for expression in [initializer, condition, step].into_iter().flatten() {
                    visit_expression(expression, visit);
                }
            }
            Statement::Return(None)
            | Statement::InlineAsm(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => {}
        }
    }

    let mut leading = std::collections::HashMap::<String, usize>::new();
    let initializers_have_call = function.locals.iter().any(|local| {
        local
            .initializer
            .as_ref()
            .is_some_and(crate::analysis::expression_has_call)
    });
    for initializer in function
        .locals
        .iter()
        .filter_map(|local| local.initializer.as_ref())
        .take_while(|initializer| !crate::analysis::expression_has_call(initializer))
    {
        visit_expression(initializer, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut leading,
            )
        });
    }
    if !initializers_have_call {
        for statement in function
            .statements
            .iter()
            .take_while(|statement| !crate::analysis::statement_has_call(statement))
        {
            visit_statement(statement, &mut |expression| {
                collect(
                    expression,
                    addressable_globals,
                    global_array_sizes,
                    &mut leading,
                )
            });
        }
    }

    let mut total = std::collections::HashMap::<String, usize>::new();
    for initializer in function
        .locals
        .iter()
        .filter_map(|local| local.initializer.as_ref())
    {
        visit_expression(initializer, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut total,
            )
        });
    }
    for statement in &function.statements {
        visit_statement(statement, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut total,
            )
        });
    }
    for guard in &function.guards {
        visit_expression(&guard.condition, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut total,
            )
        });
        visit_expression(&guard.value, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut total,
            )
        });
    }
    if let Some(expression) = &function.return_expression {
        visit_expression(expression, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut total,
            )
        });
    }

    let mut linear_total = std::collections::HashMap::<String, usize>::new();
    for initializer in function
        .locals
        .iter()
        .filter_map(|local| local.initializer.as_ref())
    {
        visit_expression(initializer, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut linear_total,
            )
        });
    }
    for statement in &function.statements {
        visit_linear_statement(statement, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut linear_total,
            )
        });
    }
    for guard in &function.guards {
        for expression in [&guard.condition, &guard.value] {
            visit_expression(expression, &mut |expression| {
                collect(
                    expression,
                    addressable_globals,
                    global_array_sizes,
                    &mut linear_total,
                )
            });
        }
    }
    if let Some(expression) = &function.return_expression {
        visit_expression(expression, &mut |expression| {
            collect(
                expression,
                addressable_globals,
                global_array_sizes,
                &mut linear_total,
            )
        });
    }

    let (global, count) = total
        .into_iter()
        .filter(|(global, count)| {
            let leading_count = leading.get(global).copied().unwrap_or(0);
            leading_count >= 3
                || (leading_count >= 1 && *count >= 3 && *count > leading_count)
        })
        .max_by(|(left_name, left_count), (right_name, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_name.cmp(left_name))
        })?;
    let leading_count = leading.get(&global).copied().unwrap_or(0);
    let use_count = if loop_uses_global_member(&function.statements, &global) {
        count
    } else {
        linear_total.get(&global).copied().unwrap_or(leading_count)
    };
    Some(StructuredGlobalBasePlan {
        total_size: global_size(&global)?,
        use_count,
        global,
        crosses_call: count > leading_count,
    })
}

/// A loop body is one address live range even though branch arms outside loops
/// rematerialize independently. Count every syntactic use that lowering emits
/// so the shared base is not exhausted halfway through the loop transaction.
fn loop_uses_global_member(statements: &[Statement], global: &str) -> bool {
    for statement in statements {
        match statement {
            Statement::Loop { body, .. } => {
                let mut occurrences = std::collections::HashMap::new();
                visit_statement(statement, &mut |expression| {
                    collect_global_member(expression, global, &mut occurrences)
                });
                if occurrences.get(global).copied().unwrap_or(0) != 0
                    || loop_uses_global_member(body, global)
                {
                    return true;
                }
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                if loop_uses_global_member(then_body, global)
                    || loop_uses_global_member(else_body, global)
                {
                    return true;
                }
            }
            Statement::Switch { arms, default, .. } => {
                if arms
                    .iter()
                    .map(|arm| &arm.body)
                    .chain(default.iter())
                    .any(|body| match body {
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            loop_uses_global_member(statements, global)
                        }
                        mwcc_syntax_trees::ArmBody::Return(_) => false,
                    })
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn collect_global_member(
    expression: &Expression,
    global: &str,
    occurrences: &mut std::collections::HashMap<String, usize>,
) {
    let Expression::Member { base, .. } = expression else {
        return;
    };
    let name = match base.as_ref() {
        Expression::Variable(name) => Some(name),
        Expression::Index { base, .. } => match base.as_ref() {
            Expression::Variable(name) => Some(name),
            _ => None,
        },
        _ => None,
    };
    if name.is_some_and(|name| name == global) {
        *occurrences.entry(global.to_string()).or_default() += 1;
    }
}

impl Generator {
    pub(crate) fn structured_global_base_register(&self, name: &str) -> Option<u8> {
        self.structured_global_base_cache
            .as_ref()
            .filter(|cache| cache.global == name && cache.remaining_uses != 0)
            .map(|cache| cache.register)
    }

    pub(crate) fn consume_structured_global_base_use(&mut self, name: &str) {
        if let Some(cache) = self
            .structured_global_base_cache
            .as_mut()
            .filter(|cache| cache.global == name && cache.remaining_uses != 0)
        {
            cache.remaining_uses -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, LocalDeclaration, LoopKind, Statement, Type};

    fn member(index: Option<i64>, offset: u32) -> Expression {
        let base = index.map_or_else(
            || Expression::Variable("pads".into()),
            |index| Expression::Index {
                base: Box::new(Expression::Variable("pads".into())),
                index: Box::new(Expression::IntegerLiteral(index)),
            },
        );
        Expression::Member {
            base: Box::new(base),
            offset,
            member_type: Type::Float,
            index_stride: index.map(|_| 68),
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
    fn plans_repeated_constant_members_before_the_first_call() {
        let product = Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(member(None, 48)),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: Box::new(member(Some(1), 48)),
                right: Box::new(member(Some(2), 48)),
            }),
        };
        let function = function(vec![
            Statement::Assign {
                name: "value".into(),
                value: product,
            },
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: Vec::new(),
            }),
            Statement::Assign {
                name: "later".into(),
                value: member(Some(3), 48),
            },
        ]);
        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::new(),
                &std::collections::HashMap::from([("pads".into(), 272)])
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: true,
                use_count: 4,
            })
        );
        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "pads".into(),
                    Type::Struct {
                        size: 272,
                        align: 4,
                    },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: true,
                use_count: 4,
            })
        );
    }

    #[test]
    fn extends_a_leading_pair_across_a_call_for_a_later_member() {
        let function = function(vec![
            Statement::Assign {
                name: "value".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(member(None, 48)),
                    right: Box::new(member(Some(1), 48)),
                },
            },
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: Vec::new(),
            }),
            Statement::Assign {
                name: "later".into(),
                value: member(Some(2), 48),
            },
        ]);
        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::new(),
                &std::collections::HashMap::from([("pads".into(), 272)])
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: true,
                use_count: 3,
            })
        );
    }

    #[test]
    fn ends_the_shared_base_before_a_nested_guard_body() {
        let function = function(vec![
            Statement::Assign {
                name: "first".into(),
                value: member(None, 48),
            },
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: Vec::new(),
            }),
            Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::LogicalOr,
                    left: Box::new(member(None, 80)),
                    right: Box::new(member(None, 80)),
                },
                then_body: vec![Statement::Assign {
                    name: "nested".into(),
                    value: Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(member(None, 167)),
                        right: Box::new(member(None, 216)),
                    },
                }],
                else_body: Vec::new(),
            },
        ]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "pads".into(),
                    Type::Struct {
                        size: 272,
                        align: 4,
                    },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: true,
                use_count: 3,
            })
        );
    }

    #[test]
    fn retains_the_shared_base_for_every_emitted_loop_use() {
        let function = function(vec![
            Statement::Assign {
                name: "first".into(),
                value: member(None, 48),
            },
            Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(member(None, 80)),
                step: None,
                body: vec![
                    Statement::Expression(Expression::Call {
                        name: "sink".into(),
                        arguments: Vec::new(),
                    }),
                    Statement::Assign {
                        name: "later".into(),
                        value: member(None, 167),
                    },
                ],
            },
        ]);

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "pads".into(),
                    Type::Struct {
                        size: 272,
                        align: 4,
                    },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: true,
                use_count: 3,
            })
        );
    }

    #[test]
    fn counts_a_local_initializer_as_the_pre_call_base_use() {
        let mut function = function(vec![
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: Vec::new(),
            }),
            Statement::Assign {
                name: "later".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(member(None, 80)),
                    right: Box::new(member(None, 167)),
                },
            },
        ]);
        function.locals.push(LocalDeclaration {
            declared_type: Type::Int,
            name: "initial".into(),
            initializer: Some(member(None, 216)),
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });

        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::from([(
                    "pads".into(),
                    Type::Struct {
                        size: 272,
                        align: 4,
                    },
                )]),
                &std::collections::HashMap::new(),
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: true,
                use_count: 3,
            })
        );
    }

    #[test]
    fn rejects_a_pair_without_a_post_call_reuse() {
        let function = function(vec![
            Statement::Assign {
                name: "value".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(member(None, 48)),
                    right: Box::new(member(Some(1), 48)),
                },
            },
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: Vec::new(),
            }),
        ]);
        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::new(),
                &std::collections::HashMap::from([("pads".into(), 272)])
            ),
            None
        );
    }

    #[test]
    fn leading_only_cluster_does_not_claim_a_saved_home() {
        let function = function(vec![Statement::Assign {
            name: "value".into(),
            value: Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(member(None, 48)),
                right: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(member(Some(1), 48)),
                    right: Box::new(member(Some(2), 48)),
                }),
            },
        }]);
        assert_eq!(
            plan(
                &function,
                &std::collections::HashMap::new(),
                &std::collections::HashMap::from([("pads".into(), 272)])
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
                crosses_call: false,
                use_count: 3,
            })
        );
    }
}
