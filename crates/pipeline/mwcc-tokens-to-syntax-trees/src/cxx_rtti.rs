//! Unit-level CodeWarrior C++ RTTI data materialization.
//!
//! Parsing owns class relationships; this pass owns their object-data ABI.
//! Keeping the two phases separate lets `-RTTI` remain a driver policy and
//! avoids mixing compiler-generated globals into ordinary declaration parsing.

use std::collections::{HashMap, HashSet};

use mwcc_syntax_trees::{CxxAbiClass, GlobalDeclaration, TranslationUnit, Type};

const ANONYMOUS_PREFIX: &str = "@@cxx_rtti:";

/// Add RTTI handles, type-name objects, inheritance tables, and vtable header
/// fields for the class closure referenced by this translation unit's owned
/// vtables. Generated classes follow reverse declaration order, as MWCC does.
pub fn materialize(unit: &mut TranslationUnit) {
    // RTTI ownership is fixed during the ordinary definition walk, before weak
    // inline bodies are materialized at the end of the translation unit. Keep
    // those late bodies out of the RTTI symbol's source-position count.
    let weak_materialized: HashSet<&str> = unit
        .weak_materialized
        .iter()
        .map(String::as_str)
        .collect();
    let immediate_materialized: HashSet<&str> = unit
        .immediate_weak_materializations
        .iter()
        .map(|(_, body)| body.as_str())
        .collect();
    let late_function_count = unit
        .functions
        .iter()
        .filter(|function| {
            weak_materialized.contains(function.name.as_str())
                && !immediate_materialized.contains(function.name.as_str())
        })
        .count();
    let late_non_static_count = unit
        .functions
        .iter()
        .filter(|function| {
            !function.is_static
                && weak_materialized.contains(function.name.as_str())
                && !immediate_materialized.contains(function.name.as_str())
        })
        .count();
    let classes: HashMap<&str, &CxxAbiClass> = unit
        .cxx_abi_classes
        .iter()
        .map(|class| (class.source_name.as_str(), class))
        .collect();
    let owned_vtables: HashSet<String> = unit
        .cxx_abi_classes
        .iter()
        .map(vtable_symbol)
        .filter(|vtable| unit.globals.iter().any(|global| global.name == *vtable))
        .collect();
    if owned_vtables.is_empty() {
        return;
    }

    let mut required = HashSet::new();
    for class in &unit.cxx_abi_classes {
        if owned_vtables.contains(&vtable_symbol(class)) {
            collect_class_closure(&class.source_name, &classes, &mut required);
        }
    }

    let original = std::mem::take(&mut unit.globals);
    let insertion = original
        .iter()
        .position(|global| owned_vtables.contains(&global.name))
        .unwrap_or(original.len());
    let mut vtables: HashMap<String, GlobalDeclaration> = original
        .iter()
        .filter(|global| owned_vtables.contains(&global.name))
        .cloned()
        .map(|global| (global.name.clone(), global))
        .collect();
    let mut retained: Vec<_> = original
        .into_iter()
        .filter(|global| !owned_vtables.contains(&global.name))
        .collect();

    let mut generated = Vec::new();
    for class in unit.cxx_abi_classes.iter().rev() {
        if !required.contains(class.source_name.as_str()) {
            continue;
        }
        let rtti = rtti_symbol(class);
        let mut owner_position = None;
        let mut late_weak_vtable = None;
        if let Some(mut vtable) = vtables.remove(&vtable_symbol(class)) {
            owner_position = Some((
                vtable.non_static_functions_before,
                vtable.functions_before,
            ));
            materialize_vtable_headers(&mut vtable, class, &rtti);
            if vtable.is_weak {
                late_weak_vtable = Some(vtable);
            } else {
                generated.push(vtable);
            }
        }

        let name = anonymous_name(class, "name");
        let mut name_bytes = class.source_name.as_bytes().to_vec();
        name_bytes.push(0);
        let mut name_global = data_global(
            name.clone(),
            name_bytes,
            Vec::new(),
            true,
            false,
            1,
        );
        if late_weak_vtable.is_some() {
            if let Some((non_static_functions_before, functions_before)) = owner_position {
                name_global.non_static_functions_before =
                    non_static_functions_before.saturating_sub(late_non_static_count);
                name_global.functions_before = functions_before.saturating_sub(late_function_count);
            }
        }
        generated.push(name_global);
        // An all-inline class has no early key-function owner. Its RTTI name is
        // therefore allocated at the constructor's source-function frontier,
        // immediately before the late weak vtable group.
        if let Some(vtable) = late_weak_vtable {
            generated.push(vtable);
        }

        let hierarchy = inheritance_entries(class, &classes);
        let hierarchy_name = (!hierarchy.is_empty()).then(|| anonymous_name(class, "bases"));
        if let Some(hierarchy_name) = &hierarchy_name {
            let mut bytes = vec![0; hierarchy.len() * 8 + 4];
            let relocations = hierarchy
                .iter()
                .enumerate()
                .map(|(index, (base, offset))| {
                    bytes[index * 8 + 4..index * 8 + 8].copy_from_slice(&offset.to_be_bytes());
                    (index as u32 * 8, rtti_symbol(base), 0)
                })
                .collect();
            generated.push(data_global(
                hierarchy_name.clone(),
                bytes,
                relocations,
                true,
                false,
                4,
            ));
        }

        let mut relocations = Vec::new();
        if let Some(hierarchy_name) = hierarchy_name {
            relocations.push((4, hierarchy_name, 0));
        }
        // The object writer emits ordinary data relocations in reverse source
        // order. Store field 1 before field 0 so RTTI handles appear in their
        // measured address order (`name`, then optional base table).
        relocations.push((0, name, 0));
        let mut handle = data_global(
            rtti,
            vec![0; 8],
            relocations,
            false,
            true,
            4,
        );
        if let Some((non_static_functions_before, functions_before)) = owner_position {
            handle.non_static_functions_before =
                non_static_functions_before.saturating_sub(late_non_static_count);
            handle.functions_before = functions_before.saturating_sub(late_function_count);
        }
        generated.push(handle);
    }

    let insertion = insertion.min(retained.len());
    retained.splice(insertion..insertion, generated);
    unit.globals = retained;
}

fn collect_class_closure<'a>(
    name: &'a str,
    classes: &HashMap<&'a str, &'a CxxAbiClass>,
    output: &mut HashSet<&'a str>,
) {
    if !output.insert(name) {
        return;
    }
    if let Some(class) = classes.get(name) {
        for base in &class.bases {
            collect_class_closure(&base.name, classes, output);
        }
    }
}

fn inheritance_entries<'a>(
    class: &'a CxxAbiClass,
    classes: &HashMap<&'a str, &'a CxxAbiClass>,
) -> Vec<(&'a CxxAbiClass, u32)> {
    fn visit<'a>(
        class: &'a CxxAbiClass,
        origin: u32,
        classes: &HashMap<&'a str, &'a CxxAbiClass>,
        output: &mut Vec<(&'a CxxAbiClass, u32)>,
    ) {
        for base in class.bases.iter().rev() {
            let Some(base_class) = classes.get(base.name.as_str()).copied() else {
                continue;
            };
            let offset = origin + base.offset;
            visit(base_class, offset, classes, output);
            output.push((base_class, offset));
        }
    }

    let mut output = Vec::new();
    visit(class, 0, classes, &mut output);
    output
}

fn materialize_vtable_headers(vtable: &mut GlobalDeclaration, class: &CxxAbiClass, rtti: &str) {
    let Some(bytes) = vtable.data_bytes.as_mut() else {
        return;
    };
    for component in &class.vtable_components {
        let header = component.table_offset as usize;
        if header + 8 > bytes.len() {
            continue;
        }
        bytes[header + 4..header + 8]
            .copy_from_slice(&(0i32.wrapping_sub(component.object_offset as i32)).to_be_bytes());
    }

    // MWCC records the primary destructor first, then keeps the later secondary
    // components in table order, followed by the first secondary. The object
    // writer reverses data relocations, producing the measured 20, 104, 88
    // presentation for four tables.
    let mut destructors = Vec::new();
    let mut other = Vec::new();
    for relocation in std::mem::take(&mut vtable.data_relocations) {
        if relocation.1.contains("__dt__") {
            destructors.push(relocation);
        } else {
            other.push(relocation);
        }
    }
    if destructors.len() > 1 {
        let primary = destructors.remove(0);
        let first_secondary = destructors.remove(0);
        other.push(primary);
        other.extend(destructors);
        other.push(first_secondary);
    } else {
        other.extend(destructors);
    }
    other.extend(
        class
            .vtable_components
            .iter()
            .map(|component| (component.table_offset, rtti.to_string(), 0)),
    );
    vtable.data_relocations = other;
}

fn data_global(
    name: String,
    bytes: Vec<u8>,
    relocations: Vec<(u32, String, i32)>,
    is_static: bool,
    is_weak: bool,
    alignment: u8,
) -> GlobalDeclaration {
    GlobalDeclaration {
        declared_type: Type::Struct {
            size: bytes.len() as u32,
            align: alignment,
        },
        source_fundamental: None,
        name,
        is_extern: false,
        is_static,
        is_volatile: false,
        is_weak,
        non_static_functions_before: 0,
        functions_before: 0,
        array_length: None,
        array_length_inferred: false,
        initializer: None,
        is_const: false,
        address_initializer: None,
        data_bytes: Some(bytes),
        data_relocations: relocations,
        section: None,
        attribute_alignment: None,
    }
}

fn anonymous_name(class: &CxxAbiClass, kind: &str) -> String {
    format!("{ANONYMOUS_PREFIX}{}:{kind}", class.encoded_name)
}

fn vtable_symbol(class: &CxxAbiClass) -> String {
    format!("__vt__{}", class.encoded_name)
}

fn rtti_symbol(class: &CxxAbiClass) -> String {
    format!("__RTTI__{}", class.encoded_name)
}

#[cfg(test)]
mod tests {
    use super::{data_global, inheritance_entries, materialize_vtable_headers};
    use mwcc_syntax_trees::{CxxAbiBase, CxxAbiClass, CxxAbiVtableComponent};
    use std::collections::HashMap;

    fn class(name: &str, bases: &[(&str, u32)]) -> CxxAbiClass {
        CxxAbiClass {
            source_name: name.to_string(),
            encoded_name: format!("{}{name}", name.len()),
            bases: bases
                .iter()
                .map(|(name, offset)| CxxAbiBase {
                    name: (*name).to_string(),
                    offset: *offset,
                })
                .collect(),
            vtable_components: Vec::new(),
        }
    }

    #[test]
    fn inheritance_table_is_reverse_depth_first_postorder() {
        let classes = [
            class("A", &[]),
            class("B", &[]),
            class("C", &[("A", 0), ("B", 4)]),
            class("D", &[]),
            class("E", &[("C", 0), ("D", 8)]),
        ];
        let by_name: HashMap<_, _> = classes
            .iter()
            .map(|class| (class.source_name.as_str(), class))
            .collect();
        let entries: Vec<_> = inheritance_entries(&classes[4], &by_name)
            .into_iter()
            .map(|(class, offset)| (class.source_name.as_str(), offset))
            .collect();
        assert_eq!(entries, [("D", 8), ("B", 4), ("A", 0), ("C", 0)]);
    }

    #[test]
    fn vtable_headers_and_relocations_have_independent_abi_order() {
        let class = CxxAbiClass {
            source_name: "E".to_string(),
            encoded_name: "1E".to_string(),
            bases: Vec::new(),
            vtable_components: vec![
                CxxAbiVtableComponent { table_offset: 0, object_offset: 0 },
                CxxAbiVtableComponent { table_offset: 12, object_offset: 4 },
                CxxAbiVtableComponent { table_offset: 24, object_offset: 8 },
                CxxAbiVtableComponent { table_offset: 36, object_offset: 12 },
            ],
        };
        let mut vtable = data_global(
            "__vt__1E".to_string(),
            vec![0; 48],
            vec![
                (8, "__dt__1EFv".to_string(), 0),
                (20, "@4@__dt__1EFv".to_string(), 0),
                (32, "@8@__dt__1EFv".to_string(), 0),
                (44, "@12@__dt__1EFv".to_string(), 0),
            ],
            false,
            false,
            4,
        );

        materialize_vtable_headers(&mut vtable, &class, "__RTTI__1E");

        let bytes = vtable.data_bytes.unwrap();
        assert_eq!(&bytes[16..20], &(-4i32).to_be_bytes());
        assert_eq!(&bytes[28..32], &(-8i32).to_be_bytes());
        assert_eq!(&bytes[40..44], &(-12i32).to_be_bytes());
        assert_eq!(
            vtable.data_relocations,
            vec![
                (8, "__dt__1EFv".to_string(), 0),
                (32, "@8@__dt__1EFv".to_string(), 0),
                (44, "@12@__dt__1EFv".to_string(), 0),
                (20, "@4@__dt__1EFv".to_string(), 0),
                (0, "__RTTI__1E".to_string(), 0),
                (12, "__RTTI__1E".to_string(), 0),
                (24, "__RTTI__1E".to_string(), 0),
                (36, "__RTTI__1E".to_string(), 0),
            ]
        );
    }
}
