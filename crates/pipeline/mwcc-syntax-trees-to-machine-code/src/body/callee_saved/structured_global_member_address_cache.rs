//! A repeated scalar-global member address retained across a possible call.
//!
//! Build 163 can keep `&global.member` in a callee-saved register when the same
//! member is read on both sides of a call. Other members of the aggregate still
//! materialize their own base, so this is deliberately separate from the
//! call-free whole-aggregate base cache.

use mwcc_syntax_trees::{Expression, Function, Type};

use super::structured_expression_visit::{visit_expression, visit_statement};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalMemberAddressPlan {
    pub(super) global: String,
    pub(super) total_size: u32,
    pub(super) offset: i16,
}

pub(super) fn plan(
    function: &Function,
    addressable_globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<StructuredGlobalMemberAddressPlan> {
    let struct_sizes = addressable_globals
        .iter()
        .filter_map(|(name, declared_type)| match declared_type {
            Type::Struct { size, .. } if !global_array_sizes.contains_key(name) => {
                Some((name.clone(), u32::from(*size)))
            }
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    let mut positions = std::collections::HashMap::<(String, u32), Vec<usize>>::new();
    for (statement_index, statement) in function.statements.iter().enumerate() {
        visit_statement(statement, &mut |expression| {
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
                positions
                    .entry((global.clone(), *offset))
                    .or_default()
                    .push(statement_index);
            }
        });
    }

    positions
        .into_iter()
        .filter_map(|((global, offset), positions)| {
            let first = *positions.first()?;
            let last = *positions.last()?;
            if positions.len() < 2 || first >= last {
                return None;
            }
            let call_between = function.statements[first + 1..last]
                .iter()
                .any(crate::analysis::statement_has_call)
                || member_precedes_nested_call(&function.statements[first], &global, offset);
            call_between
                .then(|| {
                    Some((
                        positions.len(),
                        StructuredGlobalMemberAddressPlan {
                            total_size: struct_sizes[&global],
                            global,
                            offset: i16::try_from(offset).ok()?,
                        },
                    ))
                })
                .flatten()
        })
        .max_by(|(left_count, left), (right_count, right)| {
            left_count
                .cmp(right_count)
                .then_with(|| right.global.cmp(&left.global))
                .then_with(|| right.offset.cmp(&left.offset))
        })
        .map(|(_, plan)| plan)
}

fn member_precedes_nested_call(
    statement: &mwcc_syntax_trees::Statement,
    global: &str,
    offset: u32,
) -> bool {
    let mwcc_syntax_trees::Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    if crate::analysis::expression_has_call(condition) {
        return false;
    }
    let mut contains_member = false;
    visit_expression(condition, &mut |expression| {
        contains_member |= matches!(expression, Expression::Member {
            base,
            offset: found,
            index_stride: None,
            ..
        } if *found == offset
            && matches!(base.as_ref(), Expression::Variable(found) if found == global));
    });
    contains_member
        && then_body
            .iter()
            .chain(else_body)
            .any(crate::analysis::statement_has_call)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, Statement};

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
}
