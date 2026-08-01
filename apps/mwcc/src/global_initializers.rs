//! Classification of address-bearing global initializers.
//!
//! The driver serializes supported tables, while this module owns the linkage
//! and target-provenance checks that decide when private tables are safe to
//! lower. Keeping those checks separate makes new initializer families
//! measurable without adding more policy to the driver loop.

use mwcc_machine_code::{MachineFunction, RelocationTarget};
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

/// An internal or read-only table whose relocations all target string literals
/// owned by this translation unit is self-contained. Writable internal tables
/// route to `.data`; const tables may retain external linkage and route to
/// `.rodata`. The driver interns each string and the object writer can therefore
/// order every target without any unresolved external-symbol policy.
pub(crate) fn owned_string_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
) -> bool {
    (global.is_static || global.is_const)
        && global.array_length.is_some()
        && elements
            .iter()
            .all(|element| matches!(element, PointerElement::Str(_) | PointerElement::Null))
}

/// An address-shaped aggregate containing only integer bit patterns and null
/// pointers needs no relocation policy at all. The parser preserves pointer
/// casts as scalar slots, so records such as `{ (u8*)0, (u8*)-1, 1, 1 }` can
/// be serialized directly into their ordinary writable/read-only section.
pub(crate) fn literal_address_table(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
) -> bool {
    (global.is_static || global.is_const)
        && elements
            .iter()
            .all(|element| matches!(element, PointerElement::Null | PointerElement::Scalar(_)))
}

/// Physical storage reserved for an address-bearing initializer. The parsed
/// element sequence contains only source-written slots, while C zero-fills the
/// remainder of an explicitly sized array or aggregate. Preserve the declared
/// extent so a partial table initializer does not shrink the ELF object.
pub(crate) fn storage_size(
    global: &GlobalDeclaration,
    elements: &[PointerElement],
) -> u32 {
    match global.declared_type {
        Type::Struct { size, .. } => {
            u32::from(size) * global.array_length.map_or(1, u32::from)
        }
        _ => 4 * global.array_length.map_or(elements.len() as u32, u32::from),
    }
}

/// Whole-file IPA removes an internal, read-only section registration when
/// nothing in the unit names the registration object. The section attribute
/// itself is not a liveness root in the 4.x optimizer: measured `.dtors$10`
/// probes retain the word without `-ipa file` and remove it with that flag.
///
/// Keep the classification semantic and conservative. A reference from code,
/// another data initializer, or `force_active` preserves the object.
pub(crate) fn unreferenced_section_registration(
    global: &GlobalDeclaration,
    globals: &[GlobalDeclaration],
    functions: &[MachineFunction],
) -> bool {
    if !global.is_static
        || !global.is_const
        || global.force_active
        || global.section.is_none()
        || !matches!(
            global.address_initializer.as_deref(),
            Some([PointerElement::Symbol(_)])
        )
    {
        return false;
    }
    let referenced_by_code = functions.iter().any(|function| {
        function.symbol_order.iter().any(|name| name == &global.name)
            || function.relocations.iter().any(|relocation| {
                matches!(
                    &relocation.target,
                    RelocationTarget::External(name)
                        | RelocationTarget::ExternalWithAddend(name, _)
                        if name == &global.name
                )
            })
    });
    let referenced_by_data = globals
        .iter()
        .filter(|candidate| candidate.name != global.name)
        .filter_map(|candidate| candidate.address_initializer.as_deref())
        .flatten()
        .any(|element| {
            matches!(element, PointerElement::Symbol(name) if name == &global.name)
        });
    !referenced_by_code && !referenced_by_data
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
            force_active: false,
            non_static_functions_before: 0,
            functions_before: 0,
            array_length: Some(elements.len() as u16),
            array_length_inferred: false,
            initializer: None,
            is_const: false,
            pointer_pointee_const: false,
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

    #[test]
    fn accepts_private_const_tables_of_owned_strings() {
        let elements = vec![
            PointerElement::Str(b"one".to_vec()),
            PointerElement::Str(b"two".to_vec()),
        ];
        let mut global = private_table(elements.clone());
        global.is_const = true;

        assert!(owned_string_table(&global, &elements));
    }

    #[test]
    fn accepts_externally_visible_const_tables_of_owned_strings() {
        let elements = vec![
            PointerElement::Str(b"one".to_vec()),
            PointerElement::Str(b"two".to_vec()),
        ];
        let mut global = private_table(elements.clone());
        global.is_static = false;
        global.is_const = true;

        assert!(owned_string_table(&global, &elements));
    }

    #[test]
    fn accepts_a_const_record_of_literal_pointer_bits_and_scalars() {
        let elements = vec![
            PointerElement::Null,
            PointerElement::Scalar(-1),
            PointerElement::Scalar(1),
            PointerElement::Scalar(1),
        ];
        let mut global = private_table(elements.clone());
        global.is_static = false;
        global.is_const = true;
        global.declared_type = Type::Struct { size: 16, align: 4 };

        assert!(literal_address_table(&global, &elements));
    }

    #[test]
    fn partial_struct_tables_keep_their_declared_zero_filled_tail() {
        let elements = vec![PointerElement::Symbol("callback".into()); 32];
        let mut global = private_table(elements.clone());
        global.declared_type = Type::Struct { size: 4, align: 4 };
        global.array_length = Some(33);

        assert_eq!(storage_size(&global, &elements), 132);
    }

    #[test]
    fn inferred_pointer_tables_use_the_parsed_element_count() {
        let elements = vec![PointerElement::Null, PointerElement::Null];
        let mut global = private_table(elements.clone());
        global.array_length = None;

        assert_eq!(storage_size(&global, &elements), 8);
    }

    #[test]
    fn classifies_only_unreferenced_internal_section_registrations() {
        let mut registration =
            private_table(vec![PointerElement::Symbol("destroy".into())]);
        registration.is_const = true;
        registration.array_length = None;
        registration.section = Some(".dtors$10".into());
        assert!(unreferenced_section_registration(
            &registration,
            std::slice::from_ref(&registration),
            &[],
        ));

        let mut referenced = MachineFunction::new("caller");
        referenced.symbol_order.push(registration.name.clone());
        assert!(!unreferenced_section_registration(
            &registration,
            std::slice::from_ref(&registration),
            &[referenced],
        ));

        registration.force_active = true;
        assert!(!unreferenced_section_registration(
            &registration,
            std::slice::from_ref(&registration),
            &[],
        ));
    }
}
