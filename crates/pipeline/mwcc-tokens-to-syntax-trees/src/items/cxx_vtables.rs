//! CodeWarrior C++ vtable-group construction.
//!
//! Class parsing records slots and ownership; function lowering only decides
//! when an owner is defined. This module owns the shared object-data shape.

use mwcc_syntax_trees::{Function, GlobalDeclaration, Type};

use crate::cxx::ClassLayout;

/// Construct the complete vtable group owned by a C++ key function.
/// Destructor lowering supplies its deleting entry; ordinary virtual key
/// functions use the same layout and relocation path without one.
pub(super) fn global(
    class: &ClassLayout,
    name: String,
    destructor: Option<&str>,
) -> GlobalDeclaration {
    let table_size: usize = class
        .vtable_components
        .iter()
        .map(|component| 8 + component.virtual_slots.max(1) * 4)
        .sum();
    let mut relocations = Vec::new();
    let mut component_offset = 0u32;
    if let Some(destructor) = destructor {
        for component in &class.vtable_components {
            if let Some(slot) = component.virtual_destructor_slot {
                let target = if component.object_offset == 0 {
                    destructor.to_string()
                } else {
                    format!("@{}@{}", component.object_offset, destructor)
                };
                relocations.push((component_offset + u32::from(slot), target, 0));
            }
            component_offset += 8 + component.virtual_slots.max(1) as u32 * 4;
        }
    }
    relocations.extend(
        class
            .virtual_definitions
            .iter()
            .map(|(offset, name)| (u32::from(*offset), name.clone(), 0)),
    );
    GlobalDeclaration {
        declared_type: Type::Struct {
            size: table_size as u32,
            align: 4,
        },
        name,
        is_extern: false,
        is_static: false,
        is_volatile: false,
        is_weak: false,
        non_static_functions_before: 0,
        functions_before: 0,
        array_length: None,
        array_length_inferred: false,
        initializer: None,
        is_const: false,
        address_initializer: None,
        data_bytes: Some(vec![0; table_size]),
        data_relocations: relocations,
        section: None,
        attribute_alignment: None,
    }
}

/// Compiler-generated vtable groups are emitted after the translation unit's
/// ordinary function stream, regardless of which key function owns them.
pub(super) fn position_after_functions(
    globals: &mut [GlobalDeclaration],
    functions: &[Function],
) {
    let non_static_functions = functions
        .iter()
        .filter(|function| !function.is_static)
        .count();
    for global in globals
        .iter_mut()
        .filter(|global| global.name.starts_with("__vt__"))
    {
        global.non_static_functions_before = non_static_functions;
        global.functions_before = functions.len();
    }
}
