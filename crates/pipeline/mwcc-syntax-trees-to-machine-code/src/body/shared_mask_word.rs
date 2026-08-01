//! Materialize a flags word shared by adjacent leading boolean assignments.
//!
//! Frontends commonly spell independent booleans as masks of one struct member:
//! `a = (s->flags & A) != 0; b = (s->flags & B) ? 1 : 0;`. MWCC loads the
//! non-volatile word once and extracts both bits from that value. Keeping this
//! source-level makes the shared value and its lifetime visible to the ordinary
//! virtual-register allocator.

use super::*;

#[derive(Clone, Copy, PartialEq)]
struct MemberKey<'a> {
    base: &'a str,
    offset: u32,
    member_type: Type,
    index_stride: Option<u32>,
}

fn masked_member(expression: &Expression) -> Option<MemberKey<'_>> {
    let mask = match expression {
        Expression::Conditional { condition, .. } => condition.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } if constant_value(right) == Some(0) => left.as_ref(),
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } if constant_value(left) == Some(0) => right.as_ref(),
        _ => return None,
    };
    let Expression::Binary {
        operator: BinaryOperator::BitAnd,
        left,
        right,
    } = mask
    else {
        return None;
    };
    let member = if constant_value(right).is_some() {
        left.as_ref()
    } else if constant_value(left).is_some() {
        right.as_ref()
    } else {
        return None;
    };
    let Expression::Member {
        base,
        offset,
        member_type,
        index_stride,
    } = member
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    matches!(member_type, Type::Int | Type::UnsignedInt).then_some(MemberKey {
        base,
        offset: *offset,
        member_type: *member_type,
        index_stride: *index_stride,
    })
}

fn replace_mask_member(expression: &Expression, temporary: &str) -> Expression {
    fn replace_mask(mask: &Expression, temporary: &str) -> Expression {
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } = mask
        else {
            return mask.clone();
        };
        let (left, right) = if constant_value(right).is_some() {
            (
                Expression::Variable(temporary.into()),
                right.as_ref().clone(),
            )
        } else {
            (
                left.as_ref().clone(),
                Expression::Variable(temporary.into()),
            )
        };
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    match expression {
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(replace_mask(condition, temporary)),
            when_true: when_true.clone(),
            when_false: when_false.clone(),
            origin: *origin,
        },
        Expression::Binary {
            operator,
            left,
            right,
        } if constant_value(right) == Some(0) => Expression::Binary {
            operator: *operator,
            left: Box::new(replace_mask(left, temporary)),
            right: right.clone(),
        },
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: left.clone(),
            right: Box::new(replace_mask(right, temporary)),
        },
        _ => expression.clone(),
    }
}

pub(crate) fn materialize_leading_shared_mask_word(function: &Function) -> Option<Function> {
    let [Statement::Assign {
        name: first_name,
        value: first_value,
    }, Statement::Assign {
        name: second_name,
        value: second_value,
    }, ..] = function.statements.as_slice()
    else {
        return None;
    };
    let first = masked_member(first_value)?;
    let second = masked_member(second_value)?;
    if first != second
        || first_name == second_name
        || first_name == first.base
        || second_name == first.base
        || function
            .locals
            .iter()
            .find(|local| local.name == first.base)
            .is_some_and(|local| local.is_volatile)
    {
        return None;
    }

    let mut ordinal = 0;
    let temporary = loop {
        let candidate = format!("__mwcc_shared_mask_word_{ordinal}");
        if function.locals.iter().all(|local| local.name != candidate)
            && function
                .parameters
                .iter()
                .all(|parameter| parameter.name != candidate)
        {
            break candidate;
        }
        ordinal += 1;
    };

    let mut rewritten = function.clone();
    rewritten.locals.push(LocalDeclaration {
        declared_type: first.member_type,
        name: temporary.clone(),
        initializer: Some(Expression::Member {
            base: Box::new(Expression::Variable(first.base.into())),
            offset: first.offset,
            member_type: first.member_type,
            index_stride: first.index_stride,
        }),
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    });
    let [Statement::Assign { value: first, .. }, Statement::Assign { value: second, .. }, ..] =
        rewritten.statements.as_mut_slice()
    else {
        unreachable!("the cloned leading statements retain their shape")
    };
    *first = replace_mask_member(first, &temporary);
    *second = replace_mask_member(second, &temporary);
    Some(rewritten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{ConditionalOrigin, Parameter};

    fn member() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset: 12,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    fn mask(value: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: Box::new(member()),
            right: Box::new(Expression::IntegerLiteral(value)),
        }
    }

    fn local(name: &str) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn shares_adjacent_boolean_masks_of_one_member() {
        let function = Function {
            return_type: Type::Void,
            name: "read_flags".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 16 },
                name: "state".into(),
            }],
            locals: vec![local("a"), local("b")],
            statements: vec![
                Statement::Assign {
                    name: "a".into(),
                    value: Expression::Binary {
                        operator: BinaryOperator::NotEqual,
                        left: Box::new(mask(8)),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    },
                },
                Statement::Assign {
                    name: "b".into(),
                    value: Expression::Conditional {
                        condition: Box::new(mask(4)),
                        when_true: Box::new(Expression::IntegerLiteral(1)),
                        when_false: Box::new(Expression::IntegerLiteral(0)),
                        origin: ConditionalOrigin::Ternary,
                    },
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

        let rewritten = materialize_leading_shared_mask_word(&function).expect("shared mask word");
        let shared = rewritten.locals.last().expect("materialized local");
        assert_eq!(shared.declared_type, Type::UnsignedInt);
        assert!(matches!(
            shared.initializer,
            Some(Expression::Member { offset: 12, .. })
        ));
        for statement in &rewritten.statements[..2] {
            let Statement::Assign { value, .. } = statement else {
                unreachable!()
            };
            assert!(!crate::analysis::expression_reads_name(value, "state"));
            assert!(crate::analysis::expression_reads_name(
                value,
                "__mwcc_shared_mask_word_0"
            ));
        }
    }
}
