//! Saved-home order retained from MWCC's pre-composition value graph.
//!
//! A guarded value helper can be composed into mutually exclusive statement
//! edges after allocation profitability has already selected caller homes.
//! When several caller locals survive that original value diamond, MWCC keeps
//! the late call result and the inlined transaction state at the top of the
//! saved bank, followed by the earlier caller values.

use mwcc_syntax_trees::LocalDeclaration;
use std::collections::{HashMap, HashSet};

pub(super) struct StructuredPrecompositionHomeLayout {
    preferences: HashMap<String, u8>,
    save_order: Vec<String>,
}

impl StructuredPrecompositionHomeLayout {
    pub(super) fn plan(
        deferred: &[&LocalDeclaration],
        source_survivors: &HashSet<String>,
    ) -> Option<Self> {
        let source = deferred
            .iter()
            .filter(|local| source_survivors.contains(&local.name))
            .copied()
            .collect::<Vec<_>>();
        let inlined = deferred
            .iter()
            .filter(|local| !source_survivors.contains(&local.name))
            .copied()
            .collect::<Vec<_>>();
        if source.len() != 3 || inlined.len() != 1 || deferred.len() != 4 {
            return None;
        }

        let mut preferences = HashMap::new();
        preferences.insert(source[2].name.clone(), 31);
        preferences.insert(inlined[0].name.clone(), 30);
        preferences.insert(source[0].name.clone(), 29);
        preferences.insert(source[1].name.clone(), 28);
        let save_order = [
            source[2].name.clone(),
            inlined[0].name.clone(),
            source[0].name.clone(),
            source[1].name.clone(),
        ]
        .into();
        Some(Self {
            preferences,
            save_order,
        })
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        self.preferences.get(name).copied()
    }

    pub(super) fn save_order(&self) -> impl Iterator<Item = &str> {
        self.save_order.iter().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    fn local(name: &str) -> LocalDeclaration {
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
            row_bytes: None,
        }
    }

    #[test]
    fn ranks_late_value_and_inlined_state_ahead_of_earlier_callers() {
        let locals = [
            local("error"),
            local("status"),
            local("category"),
            local("__mwcc_inline_finished"),
        ];
        let deferred = locals.iter().collect::<Vec<_>>();
        let source = ["error", "status", "category"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let plan = StructuredPrecompositionHomeLayout::plan(&deferred, &source)
            .expect("three caller values and one inlined state have a layout");

        assert_eq!(plan.preference("category"), Some(31));
        assert_eq!(plan.preference("__mwcc_inline_finished"), Some(30));
        assert_eq!(plan.preference("error"), Some(29));
        assert_eq!(plan.preference("status"), Some(28));
        assert_eq!(
            plan.save_order().collect::<Vec<_>>(),
            ["category", "__mwcc_inline_finished", "error", "status"]
        );
    }
}
