//! Classification of address-bearing global initializers.
//!
//! The driver serializes supported tables, while this module owns the linkage
//! and target-provenance checks that decide when private tables are safe to
//! lower. Keeping those checks separate makes new initializer families
//! measurable without adding more policy to the driver loop.

use mwcc_machine_code::MachineFunction;
use mwcc_syntax_trees::{GlobalDeclaration, PointerElement, Type};
use std::collections::HashSet;

/// A private writable function-pointer table can name functions defined in
/// this unit or functions declared here and supplied by another object. Both
/// families have unambiguous function-symbol linkage; the object writer owns
/// their first-use ordering in the symbol table.
pub(crate) fn private_function_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
    functions: &[MachineFunction],
    declared_functions: &HashSet<String>,
) -> bool {
    global.is_static
        && !global.is_const
        && global.array_length.is_some()
        && elements.iter().all(|element| {
            matches!(element, PointerElement::Symbol(name)
                if declared_functions.contains(name)
                    || functions.iter().any(|function| &function.name == name))
                || matches!(element, PointerElement::Null)
        })
}

/// A private aggregate whose address fields all name storage defined by this
/// translation unit has no unresolved symbol-order question: every target gets
/// an object symbol from the same writer pass. Animal Crossing animation data
/// uses this for `{ left_table, right_table, enum_value, NULL }` records.
pub(crate) fn private_unit_data_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
    globals: &[GlobalDeclaration],
) -> bool {
    global.is_static
        && !global.is_const
        && matches!(global.declared_type, Type::Struct { .. })
        && elements.iter().all(|element| match element {
            PointerElement::Symbol(name) => globals
                .iter()
                .any(|candidate| candidate.name == *name && candidate.is_data_definition()),
            PointerElement::Null | PointerElement::Scalar(_) => true,
            PointerElement::Str(_) => false,
        })
}

/// A private writable table whose relocations all target string literals owned
/// by this translation unit is self-contained. The driver interns each string
/// and the object writer can therefore order every local target without any
/// unresolved external-symbol policy.
pub(crate) fn private_string_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
) -> bool {
    global.is_static
        && !global.is_const
        && global.array_length.is_some()
        && elements
            .iter()
            .all(|element| matches!(element, PointerElement::Str(_) | PointerElement::Null))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private_table(elements: Vec<PointerElement>) -> GlobalDeclaration {
        GlobalDeclaration {
            declared_type: Type::Int,
            source_fundamental: None,
            name: "callbacks".into(),
            is_extern: false,
            is_static: true,
            is_volatile: false,
            is_weak: false,
            non_static_functions_before: 0,
            functions_before: 0,
            array_length: Some(elements.len() as u16),
            array_length_inferred: false,
            initializer: None,
            is_const: false,
            address_initializer: Some(elements),
            data_bytes: None,
            data_relocations: Vec::new(),
            section: None,
            attribute_alignment: None,
        }
    }

    #[test]
    fn accepts_private_tables_of_declared_external_functions() {
        let elements = vec![
            PointerElement::Symbol("external_callback".into()),
            PointerElement::Null,
        ];
        let global = private_table(elements.clone());
        let declared = HashSet::from(["external_callback".to_string()]);

        assert!(private_function_table(&global, &elements, &[], &declared));
    }

    #[test]
    fn rejects_private_tables_with_unclassified_address_targets() {
        let elements = vec![PointerElement::Symbol("external_data".into())];
        let global = private_table(elements.clone());

        assert!(!private_function_table(
            &global,
            &elements,
            &[],
            &HashSet::new(),
        ));
    }
}
