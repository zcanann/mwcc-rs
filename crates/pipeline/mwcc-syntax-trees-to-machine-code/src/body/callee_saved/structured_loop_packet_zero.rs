//! Reuse of zero-valued words inside dense display-list packet loops.
//!
//! MWCC keeps one zero local for packet runs with several zero payload words.
//! Keeping this transform separate avoids mixing constant-store policy into
//! invariant and dynamic arithmetic discovery.

#[allow(unused_imports)]
use super::*;

pub(super) fn has_repeated_zero_words(body: &[Statement]) -> bool {
    body.iter()
        .filter(|statement| is_zero_word(statement))
        .count()
        >= 3
}

pub(super) fn rewrite(body: &[Statement], name: &str) -> Vec<Statement> {
    let mut output = Vec::with_capacity(body.len() + 1);
    let mut initialized = false;
    for statement in body {
        if !is_zero_word(statement) {
            output.push(statement.clone());
            continue;
        }
        if !initialized {
            output.push(Statement::Assign {
                name: name.to_owned(),
                value: Expression::IntegerLiteral(0),
            });
            initialized = true;
        }
        let Statement::Store { target, .. } = statement else {
            unreachable!("zero packet words are stores")
        };
        output.push(Statement::Store {
            target: target.clone(),
            value: Expression::Variable(name.to_owned()),
        });
    }
    output
}

fn is_zero_word(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Store {
            target: Expression::Member {
                member_type: Type::UnsignedInt,
                index_stride: None,
                ..
            },
            value,
        } if crate::analysis::constant_value(value) == Some(0)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_word(offset: u32) -> Statement {
        Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("cursor".into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: Expression::IntegerLiteral(0),
        }
    }

    #[test]
    fn names_three_zero_packet_words_once() {
        let body = vec![zero_word(0), zero_word(4), zero_word(8)];
        assert!(has_repeated_zero_words(&body));
        let rewritten = rewrite(&body, "zero");

        assert!(matches!(
            &rewritten[..],
            [
                Statement::Assign {
                    name,
                    value: Expression::IntegerLiteral(0),
                },
                Statement::Store {
                    value: Expression::Variable(first),
                    ..
                },
                Statement::Store {
                    value: Expression::Variable(second),
                    ..
                },
                Statement::Store {
                    value: Expression::Variable(third),
                    ..
                },
            ] if name == "zero" && first == name && second == name && third == name
        ));
    }

    #[test]
    fn leaves_two_zero_packet_words_below_the_reuse_threshold() {
        assert!(!has_repeated_zero_words(&[zero_word(0), zero_word(4)]));
    }
}
