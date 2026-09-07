//! The 2.3.3 O4 scheduler issues a split-counter loop's second high-half
//! sample before its low-half sample, even when both reads are volatile.
//! Keep this measured bug separate from semantic inline composition.

use mwcc_syntax_trees::{
    ArmBody, BinaryOperator, Expression, Function, LoopKind, Pointee, Statement, Type,
};
use std::collections::HashMap;

pub(crate) fn materialize(
    function: &Function,
    globals: &HashMap<String, Type>,
) -> Option<Function> {
    let globals = globals
        .iter()
        .filter(|(name, _)| {
            !function
                .parameters
                .iter()
                .any(|parameter| parameter.name == **name)
                && !function.locals.iter().any(|local| local.name == **name)
        })
        .map(|(name, ty)| (name.clone(), *ty))
        .collect();
    let mut scheduled = function.clone();
    rewrite(&mut scheduled.statements, &globals).then_some(scheduled)
}

fn rewrite(statements: &mut [Statement], globals: &HashMap<String, Type>) -> bool {
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Loop {
                kind,
                condition,
                body,
                ..
            } => {
                if *kind == LoopKind::DoWhile
                    && condition
                        .as_ref()
                        .is_some_and(|condition| high_sample_is_last(body, condition, globals))
                {
                    body.swap(1, 2);
                    changed = true;
                }
                changed |= rewrite(body, globals);
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                changed |= rewrite(then_body, globals);
                changed |= rewrite(else_body, globals);
            }
            Statement::Switch { arms, default, .. } => {
                for arm in arms.iter_mut().map(|arm| &mut arm.body).chain(default) {
                    if let ArmBody::Statements(body) = arm {
                        changed |= rewrite(body, globals);
                    }
                }
            }
            _ => {}
        }
    }
    changed
}

fn high_sample_is_last(
    body: &[Statement],
    condition: &Expression,
    globals: &HashMap<String, Type>,
) -> bool {
    let [Statement::Assign {
        name: previous,
        value: Expression::Variable(current),
    }, Statement::Assign {
        name: low,
        value: low_read,
    }, Statement::Assign {
        name: high,
        value: high_read,
    }] = body
    else {
        return false;
    };
    if high != current || previous == high || low == high || low == previous {
        return false;
    }
    if !matches!(condition, Expression::Binary { operator: BinaryOperator::NotEqual, left, right }
        if matches!((left.as_ref(), right.as_ref()), (Expression::Variable(a), Expression::Variable(b))
            if (a == high && b == previous) || (a == previous && b == high)))
    {
        return false;
    }
    let Some(low_base) = halfword_read_base(low_read, globals) else {
        return false;
    };
    if halfword_read_base(high_read, globals) != Some(low_base) {
        return false;
    }
    // The two samples may only move when their address calculations do not
    // depend on any binding updated by this iteration.
    [previous, low, high].iter().all(|name| {
        !crate::analysis::expression_reads_name(low_read, name)
            && !crate::analysis::expression_reads_name(high_read, name)
    })
}

fn halfword_read_base<'a>(
    expression: &'a Expression,
    globals: &HashMap<String, Type>,
) -> Option<&'a str> {
    let pointer = match expression {
        Expression::Dereference { pointer } => pointer.as_ref(),
        Expression::Index { base, .. } => base.as_ref(),
        _ => return None,
    };
    let halfword = match pointer {
        Expression::Cast {
            target_type: Type::Pointer(Pointee::UnsignedShort),
            ..
        } => true,
        Expression::Variable(name) => matches!(
            globals.get(name),
            Some(Type::Pointer(Pointee::UnsignedShort))
        ),
        _ => false,
    };
    if !halfword || crate::analysis::expression_has_side_effect(expression) {
        return None;
    }
    fn root(expression: &Expression) -> Option<&str> {
        match expression {
            Expression::Variable(name) => Some(name),
            Expression::Cast { operand, .. } => root(operand),
            Expression::Binary {
                operator: BinaryOperator::Add | BinaryOperator::Subtract,
                left,
                ..
            } => root(left),
            _ => None,
        }
    }
    root(pointer).filter(|name| globals.contains_key(*name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(index: Expression) -> Expression {
        Expression::Index {
            base: Box::new(Expression::Variable("registers".into())),
            index: Box::new(index),
        }
    }

    fn readback() -> (Vec<Statement>, Expression) {
        let body = vec![
            Statement::Assign {
                name: "previous".into(),
                value: Expression::Variable("high".into()),
            },
            Statement::Assign {
                name: "low".into(),
                value: sample(Expression::IntegerLiteral(40)),
            },
            Statement::Assign {
                name: "high".into(),
                value: sample(Expression::IntegerLiteral(39)),
            },
        ];
        let condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(Expression::Variable("high".into())),
            right: Box::new(Expression::Variable("previous".into())),
        };
        (body, condition)
    }

    #[test]
    fn reorders_the_readback_pair_once() {
        let globals = HashMap::from([("registers".into(), Type::Pointer(Pointee::UnsignedShort))]);
        let (body, condition) = readback();
        let mut statements = vec![Statement::Loop {
            kind: LoopKind::DoWhile,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }];
        assert!(rewrite(&mut statements, &globals));
        assert!(!rewrite(&mut statements, &globals));
        let Statement::Loop { body, .. } = &statements[0] else {
            unreachable!();
        };
        assert!(matches!(&body[1], Statement::Assign { name, .. } if name == "high"));
        assert!(matches!(&body[2], Statement::Assign { name, .. } if name == "low"));
    }

    #[test]
    fn retains_dependent_addresses_and_unmeasured_widths() {
        let globals = HashMap::from([("registers".into(), Type::Pointer(Pointee::UnsignedShort))]);
        let (mut body, condition) = readback();
        let Statement::Assign { value, .. } = &mut body[2] else {
            unreachable!();
        };
        *value = sample(Expression::Variable("low".into()));
        assert!(!high_sample_is_last(&body, &condition, &globals));
        let (body, condition) = readback();
        let globals = HashMap::from([("registers".into(), Type::Pointer(Pointee::UnsignedInt))]);
        assert!(!high_sample_is_last(&body, &condition, &globals));
    }
}
