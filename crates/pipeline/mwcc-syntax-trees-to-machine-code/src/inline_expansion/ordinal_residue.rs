//! Anonymous ordinal residue left by inline substitutions.
//!
//! The optimizer's eliminated value nodes remain visible through later `@N`
//! pool symbols even though neither the call nor those nodes survive codegen.

use mwcc_syntax_trees::InlineExpansionFacts;

pub(super) fn ordinal_residue(
    facts: InlineExpansionFacts,
    statement_body_substitutions: usize,
    value_body_substitutions: usize,
    statement_body_weight: u8,
) -> u32 {
    facts.leading_initializer_substitutions as u32
        + u32::from(statement_body_weight) * statement_body_substitutions as u32
        + 3 * value_body_substitutions as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_each_inline_substitution_form() {
        assert_eq!(
            ordinal_residue(
                InlineExpansionFacts {
                    leading_initializer_substitutions: 1,
                },
                2,
                1,
                2,
            ),
            8
        );
    }
}
