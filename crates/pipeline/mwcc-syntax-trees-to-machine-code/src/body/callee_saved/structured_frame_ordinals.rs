//! Anonymous-symbol accounting owned by structured automatic frames.
//!
//! Frame-resident values are created before a function's literal pool. Their
//! optimizer labels therefore shift the pool itself rather than the trailing
//! object ledger used by general ordinal accounting.

use mwcc_syntax_trees::{Expression, LocalDeclaration, Statement};

/// Count the frame construction labels which precede the literal pool.
///
/// Address-taken scalar locals share one table root regardless of slot count.
/// An automatic array owns a three-node construction root and supersedes the
/// scalar table root when both storage classes occupy one frame. The first
/// direct call adds a publication node when it writes an address-taken scalar.
/// Each composed inline statement body retains a three-node binding block in a
/// structured frame, in addition to its general inline-expansion residue.
pub(super) fn pre_constant_label_count(
    frame_array_count: usize,
    frame_scalar_locals: &[&LocalDeclaration],
    statements: &[Statement],
    inline_statement_body_substitutions: usize,
) -> u32 {
    let frame_root = if frame_array_count != 0 {
        3
    } else if frame_scalar_locals.is_empty() {
        0
    } else {
        1 + u32::from(first_call_publishes_scalar(
            statements,
            frame_scalar_locals,
        ))
    };
    frame_root + 3 * inline_statement_body_substitutions as u32
}

fn first_call_publishes_scalar(
    statements: &[Statement],
    frame_scalar_locals: &[&LocalDeclaration],
) -> bool {
    let arguments = statements.iter().find_map(|statement| match statement {
        Statement::Expression(Expression::Call { arguments, .. })
        | Statement::Assign {
            value: Expression::Call { arguments, .. },
            ..
        } => Some(arguments),
        _ => None,
    });
    arguments.is_some_and(|arguments| {
        arguments.iter().any(|argument| {
            matches!(
                argument,
                Expression::AddressOf { operand }
                    if matches!(
                        operand.as_ref(),
                        Expression::Variable(name)
                            if frame_scalar_locals.iter().any(|local| local.name == *name)
                    )
            )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    fn scalar(name: &str) -> LocalDeclaration {
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
    fn counts_array_only_frame_root() {
        assert_eq!(pre_constant_label_count(1, &[], &[], 0), 3);
    }

    #[test]
    fn array_root_supersedes_scalar_slot_table() {
        let first = scalar("first");
        let second = scalar("second");
        assert_eq!(
            pre_constant_label_count(1, &[&first, &second], &[], 0),
            3
        );
        assert_eq!(
            pre_constant_label_count(0, &[&first, &second], &[], 0),
            1
        );
    }

    #[test]
    fn first_call_scalar_publication_adds_one_label() {
        let slot = scalar("slot");
        let publish = Statement::Expression(Expression::Call {
            name: "write".into(),
            arguments: vec![Expression::AddressOf {
                operand: Box::new(Expression::Variable("slot".into())),
            }],
        });
        assert_eq!(
            pre_constant_label_count(0, &[&slot], &[publish], 0),
            2
        );
    }

    #[test]
    fn earlier_call_prevents_first_call_publication_label() {
        let slot = scalar("slot");
        let observe = Statement::Expression(Expression::Call {
            name: "observe".into(),
            arguments: Vec::new(),
        });
        let publish = Statement::Expression(Expression::Call {
            name: "write".into(),
            arguments: vec![Expression::AddressOf {
                operand: Box::new(Expression::Variable("slot".into())),
            }],
        });
        assert_eq!(
            pre_constant_label_count(0, &[&slot], &[observe, publish], 0),
            1
        );
    }

    #[test]
    fn counts_structured_inline_binding_blocks() {
        assert_eq!(pre_constant_label_count(1, &[], &[], 3), 12);
    }

    #[test]
    fn frame_without_addressable_storage_has_no_prefix() {
        assert_eq!(pre_constant_label_count(0, &[], &[], 0), 0);
    }
}
