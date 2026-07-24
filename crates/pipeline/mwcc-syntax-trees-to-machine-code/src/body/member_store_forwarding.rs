//! Straight-line forwarding of values just stored into one aggregate.
//!
//! Whole-file inlining commonly exposes a setter followed immediately by an
//! inlined reader of the same object. MWCC keeps the required stores but feeds
//! the reader from the still-live source registers instead of reloading the
//! members. This pass handles the alias-safe form: a store-only body targeting
//! one explicit base variable, with parameter/literal values available for
//! forwarding.

use mwcc_syntax_trees::{Expression, Function, Statement};
use std::collections::{HashMap, HashSet};

type Slot = (String, u32);

pub(super) fn forward(function: &Function) -> Option<Function> {
    let parameters: HashSet<&str> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect();
    let mut values = HashMap::<Slot, Expression>::new();
    let mut owner = None::<String>;
    let mut changed = false;
    let mut statements = Vec::with_capacity(function.statements.len());

    for statement in &function.statements {
        let Statement::Store { target, value } = statement else {
            return None;
        };
        let (base, offset) = member_slot(target)?;
        match &owner {
            Some(owner) if owner != base => return None,
            None => owner = Some(base.to_owned()),
            _ => {}
        }
        let value = rewrite(value, &values, &mut changed)?;
        if forwardable(&value, &parameters) {
            values.insert((base.to_owned(), offset), value.clone());
        } else {
            values.remove(&(base.to_owned(), offset));
        }
        statements.push(Statement::Store {
            target: target.clone(),
            value,
        });
    }

    changed.then(|| Function {
        statements,
        ..function.clone()
    })
}

fn member_slot(expression: &Expression) -> Option<(&str, u32)> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = expression
    else {
        return None;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return None;
    };
    Some((base, *offset))
}

fn forwardable(expression: &Expression, parameters: &HashSet<&str>) -> bool {
    match expression {
        Expression::Variable(name) => parameters.contains(name.as_str()),
        Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
        _ => false,
    }
}

fn rewrite(
    expression: &Expression,
    values: &HashMap<Slot, Expression>,
    changed: &mut bool,
) -> Option<Expression> {
    if let Some((base, offset)) = member_slot(expression) {
        if let Some(value) = values.get(&(base.to_owned(), offset)) {
            *changed = true;
            return Some(value.clone());
        }
    }
    Some(match expression {
        Expression::Variable(_)
        | Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => expression.clone(),
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(rewrite(left, values, changed)?),
            right: Box::new(rewrite(right, values, changed)?),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(rewrite(operand, values, changed)?),
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(rewrite(condition, values, changed)?),
            when_true: Box::new(rewrite(when_true, values, changed)?),
            when_false: Box::new(rewrite(when_false, values, changed)?),
            origin: *origin,
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(rewrite(operand, values, changed)?),
        },
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(rewrite(base, values, changed)?),
            offset: *offset,
            member_type: *member_type,
            index_stride: *index_stride,
        },
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{ConditionalOrigin, Parameter, Type};

    fn member(base: &str, offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset,
            member_type,
            index_stride: None,
        }
    }

    fn store(target: Expression, value: Expression) -> Statement {
        Statement::Store { target, value }
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "initialize".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 16 },
                    name: "object".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "kind".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "maximum".into(),
                },
                Parameter {
                    parameter_type: Type::Float,
                    name: "minimum".into(),
                },
            ],
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
    fn forwards_parameter_stores_into_a_later_member_select() {
        let source = function(vec![
            store(
                member("object", 0, Type::Int),
                Expression::Variable("kind".into()),
            ),
            store(
                member("object", 4, Type::Float),
                Expression::Variable("maximum".into()),
            ),
            store(
                member("object", 8, Type::Float),
                Expression::Variable("minimum".into()),
            ),
            store(
                member("object", 12, Type::Float),
                Expression::Conditional {
                    condition: Box::new(member("object", 0, Type::Int)),
                    when_true: Box::new(member("object", 4, Type::Float)),
                    when_false: Box::new(member("object", 8, Type::Float)),
                    origin: ConditionalOrigin::Ternary,
                },
            ),
        ]);

        let forwarded = forward(&source).expect("all three prior stores are available");
        let Statement::Store {
            value:
                Expression::Conditional {
                    condition,
                    when_true,
                    when_false,
                    ..
                },
            ..
        } = &forwarded.statements[3]
        else {
            panic!("last store should retain its conditional");
        };
        assert!(matches!(condition.as_ref(), Expression::Variable(name) if name == "kind"));
        assert!(matches!(when_true.as_ref(), Expression::Variable(name) if name == "maximum"));
        assert!(matches!(when_false.as_ref(), Expression::Variable(name) if name == "minimum"));
    }

    #[test]
    fn rejects_bodies_with_an_observable_alias_boundary() {
        let source = function(vec![
            store(
                member("object", 0, Type::Int),
                Expression::Variable("kind".into()),
            ),
            Statement::Expression(Expression::Variable("unknown_effect".into())),
            store(
                member("object", 4, Type::Int),
                member("object", 0, Type::Int),
            ),
        ]);
        assert!(forward(&source).is_none());
    }
}
