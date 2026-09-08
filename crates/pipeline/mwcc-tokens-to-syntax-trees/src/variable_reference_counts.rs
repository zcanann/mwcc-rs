//! Bound source occurrences survive executable desugaring separately from the AST.
//!
//! `x += y` has one source occurrence of x; `x = x + y` has two, even
//! though both lower to the same executable assignment. Positions deduplicate
//! parser lookahead/replays. Member names, type names, and uninitialized
//! declarations never enter through the bound-variable expression path.

use crate::parser::Parser;
use mwcc_syntax_trees::{LocalDeclaration, Parameter};

impl Parser {
    pub(crate) fn record_variable_reference(&mut self, name: &str, position: usize) {
        if self.current_debug_function_name.is_some() {
            self.current_variable_reference_sites
                .entry(name.to_owned())
                .or_default()
                .insert(position);
        }
    }

    pub(crate) fn finish_variable_reference_counts(
        &mut self,
        name: &str,
        parameters: &[Parameter],
        locals: &[LocalDeclaration],
    ) {
        // C++ implicit operands and parse-time inline substitution need their
        // own source-identity propagation before these counts are authoritative.
        if self.cplusplus
            || self.default_cplusplus
            || self.inline_substitution_count != 0
            || locals
                .iter()
                .any(|local| !self.variable_types.contains_key(&local.name))
        {
            return;
        }
        let counts = parameters
            .iter()
            .map(|p| &p.name)
            .chain(locals.iter().map(|l| &l.name))
            .map(|name| {
                (
                    name.clone(),
                    self.current_variable_reference_sites
                        .get(name)
                        .map_or(0, |sites| sites.len()),
                )
            })
            .collect();
        self.function_variable_reference_counts
            .insert(name.to_owned(), counts);
    }
}

#[cfg(test)]
mod tests {
    fn counts(source: &str) -> std::collections::HashMap<String, usize> {
        let unit = crate::parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        unit.function_variable_reference_counts["f"].clone()
    }

    #[test]
    fn source_counts_distinguish_desugared_assignments_and_steps() {
        let uses = counts(
            "void f(unsigned p, unsigned unused) {
            unsigned x = 0; unsigned y;
            x += p; x = x + p; y = 1; x++;
            for (y = 0; y < 2; y++) x += p;
        }",
        );
        assert_eq!(uses["x"], 6);
        assert_eq!(uses["p"], 3);
        assert_eq!(uses["y"], 4);
        assert_eq!(uses["unused"], 0);
    }

    #[test]
    fn bound_names_exclude_members_and_separate_block_shadows() {
        let uses = counts(
            "struct S { unsigned p; };
            void f(unsigned* p, struct S* s) {
                unsigned x = 1;
                if (*p) { unsigned x = 2; *p = x + s->p; }
                *p = x;
            }",
        );
        assert_eq!(uses["p"], 3);
        assert_eq!(uses["s"], 1);
        let locals: Vec<_> = uses.iter().filter(|(n, _)| n.starts_with('x')).collect();
        assert_eq!(locals.len(), 2);
        assert!(locals.iter().all(|(_, count)| **count == 2), "{uses:?}");
    }
}
