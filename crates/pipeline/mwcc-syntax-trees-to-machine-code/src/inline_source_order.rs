//! Source visibility for same-translation-unit automatic inlining.
//!
//! MWCC's ordinary file-IPA pass consumes definitions in source order. A
//! definition is available to later callers, but a prototype does not make a
//! later definition available to an earlier caller. Keep that policy separate
//! from the semantic eligibility checks performed by each inline-body owner.

use mwcc_syntax_trees::Function;
use std::collections::HashMap;

pub(crate) struct DefinitionOrder {
    positions: HashMap<String, usize>,
}

impl DefinitionOrder {
    pub(crate) fn new(functions: &[Function]) -> Self {
        Self {
            positions: functions
                .iter()
                .enumerate()
                .map(|(index, function)| (function.name.clone(), index))
                .collect(),
        }
    }

    pub(crate) fn is_visible_to(&self, callee: &str, caller_index: usize) -> bool {
        self.positions
            .get(callee)
            .is_some_and(|callee_index| *callee_index < caller_index)
    }
}
