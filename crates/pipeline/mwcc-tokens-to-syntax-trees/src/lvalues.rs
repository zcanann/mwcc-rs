//! Canonicalization of source lvalues used as assignment targets.
//!
//! Metrowerks accepts its historical "cast lvalue" extension: a cast may wrap
//! an assignable expression and assignments through that cast update the
//! underlying object.  Reads must retain the cast because it still controls
//! pointer arithmetic and conversion.  Assignment targets, however, name the
//! storage being updated and therefore discard only the outer cast wrappers.

use mwcc_syntax_trees::Expression;

/// Return the storage-bearing lvalue named by an assignment target.
///
/// Do not recursively alter dereference/member/index operands: casts there are
/// ordinary address/type expressions and determine the width of a memory
/// access.  Only casts which directly wrap the complete lvalue participate in
/// MWCC's cast-lvalue extension.
pub(crate) fn canonical_assignment_target(mut target: Expression) -> Expression {
    loop {
        target = match target {
            Expression::Cast { operand, .. } if is_lvalue(&operand) => *operand,
            other => return other,
        };
    }
}

fn is_lvalue(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Variable(_)
            | Expression::Dereference { .. }
            | Expression::Index { .. }
            | Expression::Member { .. }
            | Expression::Cast { .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Pointee, Type};

    #[test]
    fn removes_only_casts_wrapping_the_complete_lvalue() {
        let pointer_cast = Expression::Cast {
            target_type: Type::Pointer(Pointee::UnsignedChar),
            operand: Box::new(Expression::Variable("pointer".into())),
        };
        let dereference = Expression::Dereference {
            pointer: Box::new(pointer_cast),
        };

        assert!(matches!(
            canonical_assignment_target(dereference),
            Expression::Dereference { pointer }
                if matches!(pointer.as_ref(), Expression::Cast { .. })
        ));
    }
}
