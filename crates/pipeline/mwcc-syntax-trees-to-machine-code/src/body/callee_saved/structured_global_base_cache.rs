//! One shared address live range for repeated global aggregate member loads.
//!
//! MWCC materializes a large global array's address once for either a leading
//! cluster or a cluster that begins before a call and continues afterward. The
//! allocator naturally colors the former into a volatile register and the
//! latter into a callee-saved register.

use mwcc_syntax_trees::{Expression, Function, Type};

use super::structured_expression_visit::{visit_expression, visit_statement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalBasePlan {
    pub(super) global: String,
    pub(super) total_size: u32,
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

    let mut leading = std::collections::HashMap::<String, usize>::new();
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

    let mut total = std::collections::HashMap::<String, usize>::new();
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

    let (global, _) = total
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
    Some(StructuredGlobalBasePlan {
        total_size: global_size(&global)?,
        global,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, Statement, Type};

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
}
