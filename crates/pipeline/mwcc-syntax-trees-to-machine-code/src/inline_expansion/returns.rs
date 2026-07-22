//! Rebind callee-local void returns to one inline instance's exit boundary.

use mwcc_syntax_trees::Statement;

pub(super) fn rewrite_inline_returns(statements: &mut [Statement], boundary: &str) -> bool {
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Return(None) => {
                *statement = Statement::Goto(boundary.to_owned());
                changed = true;
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                changed |= rewrite_inline_returns(then_body, boundary);
                changed |= rewrite_inline_returns(else_body, boundary);
            }
            Statement::Loop { body, .. } => {
                changed |= rewrite_inline_returns(body, boundary);
            }
            Statement::Switch { arms, default, .. } => {
                for arm in arms {
                    if let mwcc_syntax_trees::ArmBody::Statements(body) = &mut arm.body {
                        changed |= rewrite_inline_returns(body, boundary);
                    }
                }
                if let Some(mwcc_syntax_trees::ArmBody::Statements(body)) = default {
                    changed |= rewrite_inline_returns(body, boundary);
                }
            }
            _ => {}
        }
    }
    changed
}
