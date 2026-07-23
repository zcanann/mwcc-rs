//! CodeWarrior C++ vtable-group construction.
//!
//! Class parsing records slots and ownership; function lowering only decides
//! when an owner is defined. This module owns the shared object-data shape.

use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, GlobalDeclaration, Type};
use std::collections::{HashMap, HashSet, VecDeque};

use crate::cxx::{encode_qualified_scope, mangle_qualified_member_function, ClassLayout};

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
    // The writer emits data-relocation symbols in reverse-slot order. Record
    // ordinary virtuals before deleting destructors so the final ELF stream
    // follows CodeWarrior's destructor-then-method order.
    relocations.extend(
        class
            .virtual_definitions
            .iter()
            .map(|(offset, name)| (u32::from(*offset), name.clone(), 0)),
    );
    if let Some(destructor) = destructor {
        for component in &class.vtable_components {
            if let Some(slot) = component
                .virtual_destructor_slot
                .filter(|_| !component.virtual_destructor_is_pure)
            {
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
    GlobalDeclaration {
        declared_type: Type::Struct {
            size: table_size as u32,
            align: 4,
        },
        source_fundamental: None,
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

/// Materialize weak vtable groups needed by inline base destructors reached
/// from an already-owned derived vtable. A deleting destructor restores each
/// polymorphic base's address point while unwinding; the corresponding inline
/// destructor and table therefore form a dependency closure even when the base
/// has no out-of-line key function of its own.
pub(super) fn add_inline_base_groups(
    globals: &mut Vec<GlobalDeclaration>,
    classes: &HashMap<String, ClassLayout>,
    class_order: &[String],
    inline_functions: &[Function],
) -> Compilation<HashSet<String>> {
    let inline_names = inline_functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<HashSet<_>>();
    let mut queue = class_order
        .iter()
        .filter_map(|name| {
            let table = vtable_name(name).ok()?;
            globals.iter().any(|global| global.name == table).then_some(name.clone())
        })
        .collect::<VecDeque<_>>();
    let mut visited = HashSet::new();
    let mut dependency_destructors = HashSet::new();

    while let Some(owner) = queue.pop_front() {
        if !visited.insert(owner.clone()) {
            continue;
        }
        let Some(class) = classes.get(&owner) else {
            continue;
        };
        for base in &class.bases {
            let Some(base_class) = classes.get(&base.name) else {
                continue;
            };
            if !base_class.has_virtual_destructor {
                continue;
            }
            let scopes = base.name.split("::").collect::<Vec<_>>();
            let destructor = mangle_qualified_member_function(&scopes, "__dt", &[])?;
            if !inline_names.contains(destructor.as_str()) {
                continue;
            }
            dependency_destructors.insert(destructor.clone());
            let table = vtable_name(&base.name)?;
            if !globals.iter().any(|global| global.name == table) {
                let mut group = global(base_class, table, Some(&destructor));
                group.is_weak = true;
                globals.push(group);
            }
            queue.push_back(base.name.clone());
        }
    }
    Ok(dependency_destructors)
}

fn vtable_name(class: &str) -> Compilation<String> {
    let scopes = class.split("::").collect::<Vec<_>>();
    Ok(format!("__vt__{}", encode_qualified_scope(&scopes)?))
}
