//! Hygienic expression substitution for the inline subset.

use mwcc_syntax_trees::{Expression, Statement};
use std::collections::HashMap;

pub(super) fn substitute_statement(
    statement: &Statement,
    replacements: &HashMap<String, Expression>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: substitute_expression(target, replacements),
            value: substitute_expression(value, replacements),
        },
        Statement::Expression(expression) => {
            Statement::Expression(substitute_expression(expression, replacements))
        }
        Statement::Assign { name, value } => Statement::Assign {
            name: replacements
                .get(name)
                .and_then(|replacement| match replacement {
                    Expression::Variable(name) => Some(name.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| name.clone()),
            value: substitute_expression(value, replacements),
        },
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: substitute_expression(condition, replacements),
            then_body: then_body
                .iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
        },
        _ => statement.clone(),
    }
}

pub(super) fn substitute_expression(
    expression: &Expression,
    replacements: &HashMap<String, Expression>,
) -> Expression {
    match expression {
        Expression::Variable(name) => replacements
            .get(name)
            .map_or_else(|| expression.clone(), Clone::clone),
        Expression::AggregateLiteral(elements) => Expression::AggregateLiteral(
            elements
                .iter()
                .map(|element| substitute_expression(element, replacements))
                .collect(),
        ),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute_expression(left, replacements)),
            right: Box::new(substitute_expression(right, replacements)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(substitute_expression(operand, replacements)),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => {
            let condition = substitute_expression(condition, replacements);
            // An automatic object's address is non-null. This is especially
            // visible after inlining assertion guards around by-reference
            // parameters: `&local ? 0 : __assert(...)` disappears entirely in
            // MWCC instead of materializing and testing the frame address.
            if matches!(&condition, Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Variable(_)))
            {
                substitute_expression(when_true, replacements)
            } else {
                Expression::Conditional {
                    condition: Box::new(condition),
                    when_true: Box::new(substitute_expression(when_true, replacements)),
                    when_false: Box::new(substitute_expression(when_false, replacements)),
                    origin: *origin,
                }
            }
        }
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(substitute_expression(operand, replacements)),
        },
        Expression::BitFieldRead {
            extracted,
            promoted_type,
            storage,
            shift,
            width,
        } => Expression::BitFieldRead {
            extracted: Box::new(substitute_expression(extracted, replacements)),
            promoted_type: *promoted_type,
            storage: Box::new(substitute_expression(storage, replacements)),
            shift: *shift,
            width: *width,
        },
        Expression::IndexedUpdateValue { value } => Expression::IndexedUpdateValue {
            value: Box::new(substitute_expression(value, replacements)),
        },
        Expression::Dereference { pointer } => {
            let pointer = substitute_expression(pointer, replacements);
            // Inline by-reference parameters commonly substitute `&lvalue` for
            // a callee-side `*pointer`. Preserve the C lvalue identity `*&x ==
            // x` here, before an enclosing scalar member access is rebuilt.
            // The Member arm below can then fold embedded aggregate offsets
            // into the original base instead of materializing a fake pointer.
            if let Expression::AddressOf { operand } = pointer {
                *operand
            } else {
                Expression::Dereference {
                    pointer: Box::new(pointer),
                }
            }
        }
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(substitute_expression(operand, replacements)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(substitute_expression(base, replacements)),
            index: Box::new(substitute_expression(index, replacements)),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => {
            let substituted_base = substitute_expression(base, replacements);
            // An adjusted implicit object argument is represented as a
            // MemberAddress. Once substituted beneath a scalar member access,
            // fold that address adjustment—and any embedded aggregate offset—
            // into the final displacement. This preserves the lvalue semantics
            // without materializing a temporary pointer or loading an embedded
            // struct as though it were a pointer member.
            if index_stride.is_none() {
                if let Expression::Member {
                    base: embedded_base,
                    offset: embedded_offset,
                    member_type: mwcc_syntax_trees::Type::Struct { .. },
                    index_stride: None,
                } = &substituted_base
                {
                    if let Some(total) = embedded_offset.checked_add(*offset) {
                        return Expression::Member {
                            base: embedded_base.clone(),
                            offset: total,
                            member_type: *member_type,
                            index_stride: None,
                        };
                    }
                }
                if let Expression::MemberAddress {
                    base: address_base,
                    offset: address_offset,
                    index_stride: None,
                    ..
                } = &substituted_base
                {
                    if let Some(total) = address_offset.checked_add(*offset) {
                        if let Expression::Member {
                            base: embedded_base,
                            offset: embedded_offset,
                            member_type: mwcc_syntax_trees::Type::Struct { .. },
                            index_stride: None,
                        } = address_base.as_ref()
                        {
                            if let Some(total) = embedded_offset.checked_add(total) {
                                return Expression::Member {
                                    base: embedded_base.clone(),
                                    offset: total,
                                    member_type: *member_type,
                                    index_stride: None,
                                };
                            }
                        } else {
                            return Expression::Member {
                                base: address_base.clone(),
                                offset: total,
                                member_type: *member_type,
                                index_stride: None,
                            };
                        }
                    }
                }
            }
            Expression::Member {
                base: Box::new(substituted_base),
                offset: *offset,
                member_type: *member_type,
                index_stride: *index_stride,
            }
        }
        Expression::MemberAddress {
            base,
            offset,
            element,
            index_stride,
        } => Expression::MemberAddress {
            base: Box::new(substitute_expression(base, replacements)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::CallThrough { target, arguments } => Expression::CallThrough {
            target: Box::new(substitute_expression(target, replacements)),
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
                .collect(),
        },
        Expression::VirtualCall {
            object,
            vptr_offset,
            slot_offset,
            return_type,
            variadic,
            arguments,
        } => Expression::VirtualCall {
            object: Box::new(substitute_expression(object, replacements)),
            vptr_offset: *vptr_offset,
            slot_offset: *slot_offset,
            return_type: *return_type,
            variadic: *variadic,
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
                .collect(),
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
                .collect(),
        },
        Expression::ConstructedNew {
            allocation,
            allocation_size,
            constructor,
            arguments,
        } => Expression::ConstructedNew {
            allocation: Box::new(substitute_expression(allocation, replacements)),
            allocation_size: *allocation_size,
            constructor: constructor.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_expression(argument, replacements))
                .collect(),
        },
        Expression::PostStep { target, operator } => Expression::PostStep {
            target: Box::new(substitute_expression(target, replacements)),
            operator: *operator,
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(substitute_expression(target, replacements)),
            value: Box::new(substitute_expression(value, replacements)),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(substitute_expression(left, replacements)),
            right: Box::new(substitute_expression(right, replacements)),
        },
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::CompoundLiteral { .. } => expression.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    #[test]
    fn folds_a_substituted_reference_back_into_its_member_lvalue() {
        let expression = Expression::Member {
            base: Box::new(Expression::Dereference {
                pointer: Box::new(Expression::Variable("translate".into())),
            }),
            offset: 4,
            member_type: Type::Float,
            index_stride: None,
        };
        let replacements = HashMap::from([(
            "translate".into(),
            Expression::AddressOf {
                operand: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("fighter".into())),
                    offset: 6780,
                    member_type: Type::Struct { size: 12, align: 4 },
                    index_stride: None,
                }),
            },
        )]);

        let substituted = substitute_expression(&expression, &replacements);
        assert!(matches!(
            substituted,
            Expression::Member {
                base,
                offset: 6784,
                member_type: Type::Float,
                index_stride: None,
            } if matches!(base.as_ref(), Expression::Variable(name) if name == "fighter")
        ));
    }

    #[test]
    fn folds_an_inlined_address_assertion_to_its_true_arm() {
        let expression = Expression::Conditional {
            condition: Box::new(Expression::Variable("pointer".into())),
            when_true: Box::new(Expression::IntegerLiteral(0)),
            when_false: Box::new(Expression::Call {
                name: "__assert".into(),
                arguments: Vec::new(),
            }),
            origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
        };
        let replacements = HashMap::from([(
            "pointer".into(),
            Expression::AddressOf {
                operand: Box::new(Expression::Variable("frame_local".into())),
            },
        )]);

        assert!(matches!(
            substitute_expression(&expression, &replacements),
            Expression::IntegerLiteral(0)
        ));
    }
}
