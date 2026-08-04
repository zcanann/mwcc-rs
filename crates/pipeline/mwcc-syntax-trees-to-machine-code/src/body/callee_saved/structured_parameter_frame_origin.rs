//! Saved-parameter value origins that retain a legacy optimizer frame lane.
//!
//! Directly forwarding an entry parameter through calls can live entirely in
//! its saved GPR. An indirect load/store through that parameter after a call
//! keeps the entry-table identity in build 163, even without source locals.

#[allow(unused_imports)]
use super::*;
use super::structured_expression_visit::visit_statement;

pub(super) fn has_straight_line_post_call_indirect_access(
    statements: &[Statement],
    parameter: &str,
) -> bool {
    let mut prior_call = false;
    for statement in statements {
        if prior_call && statement_indirectly_accesses(statement, parameter) {
            return true;
        }
        prior_call |= statement_has_call(statement);
    }
    false
}

fn statement_indirectly_accesses(statement: &Statement, parameter: &str) -> bool {
    let mut found = false;
    visit_statement(statement, &mut |expression| {
        let address = match expression {
            Expression::Dereference { pointer } => Some(pointer.as_ref()),
            Expression::Index { base, .. } | Expression::Member { base, .. } => {
                Some(base.as_ref())
            }
            _ => None,
        };
        found |= address.is_some_and(|address| expression_reads_name(address, parameter));
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, argument: Expression) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.into(),
            arguments: vec![argument],
        })
    }

    #[test]
    fn distinguishes_direct_forwarding_from_post_call_member_access() {
        let forwarded = vec![
            call("first", Expression::Variable("owner".into())),
            call("second", Expression::Variable("owner".into())),
        ];
        let member_loaded = vec![
            call("first", Expression::Variable("owner".into())),
            call(
                "second",
                Expression::Member {
                    base: Box::new(Expression::Variable("owner".into())),
                    offset: 32,
                    member_type: Type::Int,
                    index_stride: None,
                },
            ),
        ];

        assert!(!has_straight_line_post_call_indirect_access(
            &forwarded,
            "owner"
        ));
        assert!(has_straight_line_post_call_indirect_access(
            &member_loaded,
            "owner"
        ));
    }
}
