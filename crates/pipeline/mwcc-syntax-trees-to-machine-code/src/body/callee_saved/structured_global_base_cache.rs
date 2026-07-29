//! One shared address live range for a leading global aggregate load cluster.
//!
//! MWCC materializes a large global array's address once when several
//! constant-displacement members are read before the body's first call.  The
//! base is call-clobbered: its lifetime ends with that leading cluster rather
//! than consuming a saved register for the remainder of the function.

use mwcc_syntax_trees::{Expression, Function};

use super::structured_expression_visit::visit_statement;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalBasePlan {
    pub(super) global: String,
    pub(super) total_size: u32,
}

pub(super) fn plan(
    function: &Function,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<StructuredGlobalBasePlan> {
    let mut occurrences = std::collections::HashMap::<String, usize>::new();
    for statement in function
        .statements
        .iter()
        .take_while(|statement| !crate::analysis::statement_has_call(statement))
    {
        visit_statement(statement, &mut |expression| {
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
            if let Some(global) = global.filter(|name| global_array_sizes.contains_key(*name)) {
                *occurrences.entry(global.clone()).or_default() += 1;
            }
        });
    }
    let (global, count) =
        occurrences
            .into_iter()
            .max_by(|(left_name, left_count), (right_name, right_count)| {
                left_count
                    .cmp(right_count)
                    .then_with(|| right_name.cmp(left_name))
            })?;
    (count >= 3).then(|| StructuredGlobalBasePlan {
        total_size: global_array_sizes[&global],
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
                &std::collections::HashMap::from([("pads".into(), 272)])
            ),
            Some(StructuredGlobalBasePlan {
                global: "pads".into(),
                total_size: 272,
            })
        );
    }

    #[test]
    fn rejects_a_pair_and_ignores_members_after_the_first_call() {
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
                &std::collections::HashMap::from([("pads".into(), 272)])
            ),
            None
        );
    }
}
