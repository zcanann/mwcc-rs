//! Publish callee-local return values and leave only the current inline instance.

use mwcc_syntax_trees::Statement;

pub(super) fn rewrite_inline_returns(
    statements: &mut Vec<Statement>,
    boundary: &str,
    destination: Option<&str>,
) -> bool {
    let mut changed = false;
    let mut index = 0;
    while index < statements.len() {
        if let Statement::Return(value) = &mut statements[index] {
            let value = value.take();
            statements[index] = Statement::Goto(boundary.to_owned());
            if let Some(value) = value {
                statements.insert(
                    index,
                    match destination {
                        Some(name) => Statement::Assign {
                            name: name.to_owned(),
                            value,
                        },
                        None => Statement::Expression(value),
                    },
                );
                index += 1;
            }
            changed = true;
        }
        match &mut statements[index] {
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                changed |= rewrite_inline_returns(then_body, boundary, destination);
                changed |= rewrite_inline_returns(else_body, boundary, destination);
            }
            Statement::Loop { body, .. } => {
                changed |= rewrite_inline_returns(body, boundary, destination);
            }
            Statement::Switch { arms, default, .. } => {
                for arm in arms {
                    if let mwcc_syntax_trees::ArmBody::Statements(body) = &mut arm.body {
                        changed |= rewrite_inline_returns(body, boundary, destination);
                    }
                }
                if let Some(mwcc_syntax_trees::ArmBody::Statements(body)) = default {
                    changed |= rewrite_inline_returns(body, boundary, destination);
                }
            }
            _ => {}
        }
        index += 1;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Expression;

    #[test]
    fn early_values_reach_the_inline_destination_before_leaving_the_instance() {
        let mut body = vec![Statement::If {
            condition: Expression::Variable("ready".into()),
            then_body: vec![Statement::Return(Some(Expression::IntegerLiteral(0)))],
            else_body: vec![],
        }];
        assert!(rewrite_inline_returns(&mut body, "done", Some("result")));
        let Statement::If { then_body, .. } = &body[0] else {
            panic!()
        };
        assert!(matches!(&then_body[..], [
            Statement::Assign { name, value: Expression::IntegerLiteral(0) },
            Statement::Goto(label)
        ] if name == "result" && label == "done"));
    }

    #[test]
    fn discarded_returns_still_evaluate_their_expression() {
        let value = Expression::Call {
            name: "effect".into(),
            arguments: vec![],
        };
        let mut body = vec![Statement::Return(Some(value.clone()))];
        assert!(rewrite_inline_returns(&mut body, "done", None));
        assert!(
            matches!(&body[..], [Statement::Expression(Expression::Call { name, arguments }), Statement::Goto(label)]
            if name == "effect" && arguments.is_empty() && label == "done")
        );
    }
}
