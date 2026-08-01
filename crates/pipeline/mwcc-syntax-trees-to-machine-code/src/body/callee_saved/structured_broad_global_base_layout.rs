//! Saved-home layout for a broad aggregate accessed through one shared base.
//!
//! When four or more repeated member addresses collapse into a global base and
//! one mutation-heavy member address, MWCC retains the three ordinary loop
//! values in source-local order below that two-register prefix. The synthetic
//! loop-invariant address is deliberately last.

use mwcc_syntax_trees::LocalDeclaration;

pub(super) struct StructuredBroadGlobalBaseLayout {
    preferences: std::collections::HashMap<String, u8>,
}

impl StructuredBroadGlobalBaseLayout {
    pub(super) fn plan(
        broad_member_fanout: bool,
        eager_count: usize,
        saved_parameter_count: usize,
        deferred: &[&LocalDeclaration],
    ) -> Option<Self> {
        if !broad_member_fanout
            || eager_count != 0
            || saved_parameter_count != 0
            || deferred.len() != 3
        {
            return None;
        }
        let [first, second, invariant] = deferred else {
            return None;
        };
        if first.name.starts_with("__mwcc_")
            || second.name.starts_with("__mwcc_")
            || !invariant.name.starts_with("__mwcc_loop_address_")
        {
            return None;
        }
        let preferences = [
            (first.name.clone(), 29),
            (second.name.clone(), 28),
            (invariant.name.clone(), 27),
        ]
        .into_iter()
        .collect();
        Some(Self { preferences })
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        self.preferences.get(name).copied()
    }

    pub(super) fn retains_linkage_lane(&self) -> bool {
        true
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
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    #[test]
    fn ranks_source_values_ahead_of_the_invariant_address() {
        let locals = [local("read_size"), local("frame"), local("__mwcc_loop_address_0")];
        let deferred = locals.iter().collect::<Vec<_>>();
        let layout = StructuredBroadGlobalBaseLayout::plan(true, 0, 0, &deferred)
            .expect("a broad three-value loop has a layout");

        assert_eq!(layout.preference("read_size"), Some(29));
        assert_eq!(layout.preference("frame"), Some(28));
        assert_eq!(layout.preference("__mwcc_loop_address_0"), Some(27));
    }
}
