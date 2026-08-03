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
pub fn materialize(
    unit: &mut TranslationUnit,
    orphaned_handle_is_local: bool,
    materialize_inline_primary_base_vtables: bool,
    owned_closure_schedule: bool,
) {
    if materialize_inline_primary_base_vtables {
        synthesize_inline_primary_base_vtables(unit);
    }
    // RTTI ownership is fixed during the ordinary definition walk, before weak
    // inline bodies are materialized at the end of the translation unit. Keep
    // those late bodies out of the RTTI symbol's source-position count.
    let weak_materialized: HashSet<&str> =
        unit.weak_materialized.iter().map(String::as_str).collect();
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

    let required_direct_bases: HashSet<&str> = unit
        .cxx_abi_classes
        .iter()
        .filter(|class| required.contains(class.source_name.as_str()))
        .flat_map(|class| class.bases.iter().map(|base| base.name.as_str()))
        .collect();
    let roots: Vec<&CxxAbiClass> = unit
        .cxx_abi_classes
        .iter()
        .rev()
        .filter(|class| {
            owned_vtables.contains(&vtable_symbol(class))
                && !required_direct_bases.contains(class.source_name.as_str())
        })
        .collect();
    let mut closure_frontiers = HashMap::<&str, (usize, usize)>::new();
    if owned_closure_schedule {
        for root in &roots {
            let Some(vtable) = vtables.get(&vtable_symbol(root)) else {
                continue;
            };
            let frontier = if vtable.is_weak {
                (
                    vtable
                        .non_static_functions_before
                        .saturating_sub(late_non_static_count),
                    vtable.functions_before.saturating_sub(late_function_count),
                )
            } else {
                (vtable.non_static_functions_before, vtable.functions_before)
            };
            for (class, _) in inheritance_entries(root, &classes)
                .into_iter()
                .chain(std::iter::once((*root, 0)))
            {
                closure_frontiers
                    .entry(class.source_name.as_str())
                    .or_insert(frontier);
            }
        }
    }

    let mut generated = Vec::new();
    for class in unit.cxx_abi_classes.iter().rev() {
        if !required.contains(class.source_name.as_str()) {
            continue;
        }
        let rtti = rtti_symbol(class);
        let mut owner_position = None;
        let mut late_weak_vtable = None;
        if let Some(mut vtable) = vtables.remove(&vtable_symbol(class)) {
            owner_position = Some((vtable.non_static_functions_before, vtable.functions_before));
            materialize_vtable_headers(&mut vtable, class, &rtti);
            if vtable.is_weak {
                late_weak_vtable = Some(vtable);
            } else {
                generated.push(vtable);
            }
        }

        let name = anonymous_name(class, "name");
        let mut name_bytes = class.rtti_name.as_bytes().to_vec();
        name_bytes.push(0);
        let mut name_global = data_global(name.clone(), name_bytes, Vec::new(), true, false, 4);
        if let Some(&(non_static_functions_before, functions_before)) =
            closure_frontiers.get(class.source_name.as_str())
        {
            name_global.non_static_functions_before = non_static_functions_before;
            name_global.functions_before = functions_before;
        } else if late_weak_vtable.is_some() {
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
        let late_weak_owner = late_weak_vtable.is_some();
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
            let mut hierarchy =
                data_global(hierarchy_name.clone(), bytes, relocations, true, false, 4);
            if let Some(&(non_static_functions_before, functions_before)) =
                closure_frontiers.get(class.source_name.as_str())
            {
                hierarchy.non_static_functions_before = non_static_functions_before;
                hierarchy.functions_before = functions_before;
            }
            generated.push(hierarchy);
        }

        let mut relocations = Vec::new();
        if let Some(hierarchy_name) = hierarchy_name {
            relocations.push((4, hierarchy_name, 0));
        }
        // The object writer emits ordinary data relocations in reverse source
        // order. Store field 1 before field 0 so RTTI handles appear in their
        // measured address order (`name`, then optional base table).
        relocations.push((0, name, 0));
        let (handle_is_static, handle_is_weak) = rtti_handle_linkage(
            late_weak_owner,
            owner_position.is_some(),
            orphaned_handle_is_local,
            owned_closure_schedule,
        );
        let mut handle = data_global(
            rtti,
            vec![0; 8],
            relocations,
            handle_is_static,
            handle_is_weak,
            4,
        );
        if let Some(&(non_static_functions_before, functions_before)) =
            closure_frontiers.get(class.source_name.as_str())
        {
            handle.non_static_functions_before = non_static_functions_before;
            handle.functions_before = functions_before;
        } else if let Some((non_static_functions_before, functions_before)) = owner_position {
            handle.non_static_functions_before =
                non_static_functions_before.saturating_sub(late_non_static_count);
            handle.functions_before = functions_before.saturating_sub(late_function_count);
        }
        generated.push(handle);
    }

    if owned_closure_schedule && !closure_frontiers.is_empty() {
        order_owned_closure_globals(&mut generated, &roots, &classes);
    }

    let insertion = insertion.min(retained.len());
    retained.splice(insertion..insertion, generated);
    unit.globals = retained;
}

fn order_owned_closure_globals(
    generated: &mut Vec<GlobalDeclaration>,
    roots: &[&CxxAbiClass],
    classes: &HashMap<&str, &CxxAbiClass>,
) {
    let by_name: HashMap<String, GlobalDeclaration> = generated
        .iter()
        .cloned()
        .map(|global| (global.name.clone(), global))
        .collect();
    let mut ordered = Vec::with_capacity(generated.len());
    let mut seen = HashSet::new();

    for root in roots {
        let Some(chain) = primary_base_chain(root, classes) else {
            continue;
        };
        for class in &chain {
            for name in [
                anonymous_name(class, "name"),
                anonymous_name(class, "bases"),
                rtti_symbol(class),
            ] {
                if let Some(global) = by_name.get(&name) {
                    if seen.insert(name) {
                        ordered.push(global.clone());
                    }
                }
            }
        }
        // Every owned construction vtable in this closure shares the root's
        // source frontier. An external intermediate vtable does not split the
        // ownership transaction: the stack still unwinds most-derived first
        // across the gap.
        for owner in chain.iter().rev() {
            let name = vtable_symbol(owner);
            if let Some(global) = by_name.get(&name) {
                if seen.insert(name) {
                    ordered.push(global.clone());
                }
            }
        }
    }
    for global in generated.iter() {
        if seen.insert(global.name.clone()) {
            ordered.push(global.clone());
        }
    }
    debug_assert_eq!(generated.len(), ordered.len());
    *generated = ordered;
}

fn synthesize_inline_primary_base_vtables(unit: &mut TranslationUnit) {
    let classes: HashMap<_, _> = unit
        .cxx_abi_classes
        .iter()
        .map(|class| (class.source_name.as_str(), class))
        .collect();
    let weak_functions: HashSet<&str> = unit.weak_materialized.iter().map(String::as_str).collect();
    let mut existing: HashSet<String> = unit
        .globals
        .iter()
        .filter(|global| global.name.starts_with("__vt__"))
        .map(|global| global.name.clone())
        .collect();
    let owned: Vec<_> = unit
        .globals
        .iter()
        .filter(|global| global.is_weak && global.name.starts_with("__vt__"))
        .cloned()
        .collect();
    let mut generated = Vec::new();

    for owner_vtable in owned {
        let Some(owner) = unit
            .cxx_abi_classes
            .iter()
            .find(|class| vtable_symbol(class) == owner_vtable.name)
        else {
            continue;
        };
        let Some(chain) = primary_base_chain(owner, &classes) else {
            continue;
        };
        for class in chain.into_iter().take_while(|class| *class != owner) {
            let symbol = vtable_symbol(class);
            if existing.contains(&symbol) {
                continue;
            }
            let Some(layout) =
                inline_base_vtable_slots(class, &classes, &owner_vtable, &weak_functions)
            else {
                continue;
            };
            let mut vtable = data_global(
                symbol.clone(),
                vec![0; layout.size],
                layout.relocations,
                false,
                true,
                4,
            );
            vtable.functions_before = owner_vtable.functions_before;
            vtable.non_static_functions_before = owner_vtable.non_static_functions_before;
            existing.insert(symbol);
            generated.push(vtable);
        }
    }
    unit.globals.extend(generated);
}

fn primary_base_chain<'a>(
    class: &'a CxxAbiClass,
    classes: &HashMap<&str, &'a CxxAbiClass>,
) -> Option<Vec<&'a CxxAbiClass>> {
    let mut chain = vec![class];
    let mut current = class;
    while let [base] = current.bases.as_slice() {
        current = classes.get(base.name.as_str()).copied()?;
        chain.push(current);
    }
    if current.bases.is_empty() {
        chain.reverse();
        Some(chain)
    } else {
        None
    }
}

fn inline_base_vtable_slots(
    class: &CxxAbiClass,
    classes: &HashMap<&str, &CxxAbiClass>,
    owner_vtable: &GlobalDeclaration,
    weak_functions: &HashSet<&str>,
) -> Option<InlineBaseVtable> {
    let component = class.vtable_components.first()?;
    let component_size = 8usize.checked_add(
        usize::try_from(component.virtual_slots.max(1))
            .ok()?
            .checked_mul(4)?,
    )?;
    let inherited_slots = class
        .bases
        .first()
        .and_then(|base| classes.get(base.name.as_str()))
        .and_then(|base| base.vtable_components.first())
        .map_or(0, |component| component.virtual_slots);
    let ancestry: HashSet<&str> = primary_base_chain(class, classes)?
        .into_iter()
        .map(|ancestor| ancestor.source_name.as_str())
        .collect();
    let mut relocations = Vec::new();
    let mut owns_slot = false;
    for (offset, target, _) in &owner_vtable.data_relocations {
        if *offset < 8 || usize::try_from(*offset).ok()? >= component_size {
            continue;
        }
        let Some(target_owner) = mangled_member_owner(target) else {
            continue;
        };
        if target_owner == class.source_name {
            owns_slot = true;
            if !weak_functions.contains(target.as_str()) {
                return None;
            }
        }
        if ancestry.contains(target_owner) {
            relocations.push((*offset, target.clone(), 0));
        }
    }
    let adds_slots = component.virtual_slots > inherited_slots;
    (owns_slot || adds_slots).then_some(InlineBaseVtable {
        size: component_size,
        relocations,
    })
}

#[derive(Debug, PartialEq, Eq)]
struct InlineBaseVtable {
    size: usize,
    relocations: Vec<(u32, String, i32)>,
}

fn mangled_member_owner(name: &str) -> Option<&str> {
    for (delimiter, _) in name.match_indices("__") {
        let suffix = &name[delimiter + 2..];
        let digits = suffix
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits == 0 {
            continue;
        }
        let length = suffix[..digits].parse::<usize>().ok()?;
        return suffix.get(digits..digits.checked_add(length)?);
    }
    None
}

fn rtti_handle_linkage(
    late_weak_owner: bool,
    has_vtable_owner: bool,
    orphaned_handle_is_local: bool,
    owned_closure_schedule: bool,
) -> (bool, bool) {
    let local = owned_closure_schedule
        || late_weak_owner
        || (!has_vtable_owner && orphaned_handle_is_local);
    (local, !local)
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
        force_active: false,
        non_static_functions_before: 0,
        functions_before: 0,
        array_length: None,
        array_length_inferred: false,
        initializer: None,
        is_const: false,
        pointer_pointee_const: false,
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
    use super::{
        data_global, inheritance_entries, inline_base_vtable_slots, mangled_member_owner,
        materialize_vtable_headers, order_owned_closure_globals, rtti_handle_linkage,
        InlineBaseVtable,
    };
    use mwcc_syntax_trees::{CxxAbiBase, CxxAbiClass, CxxAbiVtableComponent};
    use std::collections::{HashMap, HashSet};

    fn class(name: &str, bases: &[(&str, u32)]) -> CxxAbiClass {
        CxxAbiClass {
            source_name: name.to_string(),
            rtti_name: name.to_string(),
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

    fn polymorphic_class(name: &str, bases: &[(&str, u32)], virtual_slots: u32) -> CxxAbiClass {
        let mut class = class(name, bases);
        class.vtable_components.push(CxxAbiVtableComponent {
            table_offset: 0,
            object_offset: 0,
            virtual_slots,
        });
        class
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
    fn legacy_orphaned_rtti_handles_remain_local() {
        assert_eq!(
            rtti_handle_linkage(false, false, true, false),
            (true, false)
        );
        assert_eq!(
            rtti_handle_linkage(false, false, false, false),
            (false, true)
        );
        assert_eq!(
            rtti_handle_linkage(true, true, false, false),
            (true, false)
        );
        assert_eq!(
            rtti_handle_linkage(false, true, false, true),
            (true, false)
        );
    }

    #[test]
    fn owned_closure_schedules_helpers_and_vtable_dependency_runs() {
        let base = class("Base", &[]);
        let boss = class("Boss", &[("Base", 0)]);
        let mut generated = vec![
            data_global(
                "@@cxx_rtti:4Boss:name".into(),
                b"Boss\0".to_vec(),
                vec![],
                true,
                false,
                4,
            ),
            data_global("__vt__4Boss".into(), vec![0; 12], vec![], false, true, 4),
            data_global(
                "@@cxx_rtti:4Boss:bases".into(),
                vec![0; 12],
                vec![],
                true,
                false,
                4,
            ),
            data_global("__RTTI__4Boss".into(), vec![0; 8], vec![], true, false, 4),
            data_global(
                "@@cxx_rtti:4Base:name".into(),
                b"Base\0".to_vec(),
                vec![],
                true,
                false,
                4,
            ),
            data_global("__vt__4Base".into(), vec![0; 12], vec![], false, true, 4),
            data_global("__RTTI__4Base".into(), vec![0; 8], vec![], true, false, 4),
        ];

        let classes = HashMap::from([
            (base.source_name.as_str(), &base),
            (boss.source_name.as_str(), &boss),
        ]);
        order_owned_closure_globals(&mut generated, &[&boss], &classes);

        assert_eq!(
            generated
                .iter()
                .map(|global| global.name.as_str())
                .collect::<Vec<_>>(),
            [
                "@@cxx_rtti:4Base:name",
                "__RTTI__4Base",
                "@@cxx_rtti:4Boss:name",
                "@@cxx_rtti:4Boss:bases",
                "__RTTI__4Boss",
                "__vt__4Boss",
                "__vt__4Base",
            ]
        );
    }

    #[test]
    fn owned_vtable_transaction_crosses_an_external_intermediate_table() {
        let a_node = class("ANode", &[]);
        let core_node = class("CoreNode", &[("ANode", 0)]);
        let node = class("Node", &[("CoreNode", 0)]);
        let boss = class("Boss", &[("Node", 0)]);
        let mut generated = vec![
            data_global(
                "__vt__8CoreNode".into(),
                vec![0; 16],
                vec![],
                false,
                true,
                4,
            ),
            data_global("__vt__5ANode".into(), vec![0; 12], vec![], false, true, 4),
            data_global("__vt__4Boss".into(), vec![0; 20], vec![], false, true, 4),
        ];
        let classes = HashMap::from([
            (a_node.source_name.as_str(), &a_node),
            (core_node.source_name.as_str(), &core_node),
            (node.source_name.as_str(), &node),
            (boss.source_name.as_str(), &boss),
        ]);

        order_owned_closure_globals(&mut generated, &[&boss], &classes);

        assert_eq!(
            generated
                .iter()
                .map(|global| global.name.as_str())
                .collect::<Vec<_>>(),
            ["__vt__4Boss", "__vt__8CoreNode", "__vt__5ANode"]
        );
    }

    #[test]
    fn overlapping_owned_closures_schedule_shared_helpers_once() {
        let base = class("Base", &[]);
        let left = class("Left", &[("Base", 0)]);
        let right = class("Right", &[("Base", 0)]);
        let mut generated = vec![
            data_global(
                "@@cxx_rtti:4Base:name".into(),
                b"Base\0".to_vec(),
                vec![],
                true,
                false,
                4,
            ),
            data_global("__RTTI__4Base".into(), vec![0; 8], vec![], true, false, 4),
            data_global(
                "@@cxx_rtti:4Left:name".into(),
                b"Left\0".to_vec(),
                vec![],
                true,
                false,
                4,
            ),
            data_global(
                "@@cxx_rtti:4Left:bases".into(),
                vec![0; 12],
                vec![],
                true,
                false,
                4,
            ),
            data_global("__RTTI__4Left".into(), vec![0; 8], vec![], true, false, 4),
            data_global(
                "@@cxx_rtti:5Right:name".into(),
                b"Right\0".to_vec(),
                vec![],
                true,
                false,
                4,
            ),
            data_global(
                "@@cxx_rtti:5Right:bases".into(),
                vec![0; 12],
                vec![],
                true,
                false,
                4,
            ),
            data_global("__RTTI__5Right".into(), vec![0; 8], vec![], true, false, 4),
        ];
        let classes = HashMap::from([
            (base.source_name.as_str(), &base),
            (left.source_name.as_str(), &left),
            (right.source_name.as_str(), &right),
        ]);

        order_owned_closure_globals(&mut generated, &[&left, &right], &classes);

        let names = generated
            .iter()
            .map(|global| global.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names.len(), 8);
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "@@cxx_rtti:4Base:name")
                .count(),
            1
        );
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "__RTTI__4Base")
                .count(),
            1
        );
    }

    #[test]
    fn member_owner_parsing_distinguishes_inherited_slots() {
        assert_eq!(
            mangled_member_owner("read__8CoreNodeFR18RandomAccessStream"),
            Some("CoreNode")
        );
        assert_eq!(mangled_member_owner("__dt__4NodeFv"), Some("Node"));
    }

    #[test]
    fn all_inline_primary_bases_recover_their_vtable_prefixes() {
        let classes = [
            polymorphic_class("ANode", &[], 1),
            polymorphic_class("CoreNode", &[("ANode", 0)], 2),
            polymorphic_class("Node", &[("CoreNode", 0)], 4),
        ];
        let by_name: HashMap<_, _> = classes
            .iter()
            .map(|class| (class.source_name.as_str(), class))
            .collect();
        let vtable = data_global(
            "__vt__4Node".into(),
            vec![0; 24],
            vec![
                (8, "age__5ANodeFv".into(), 0),
                (12, "read__8CoreNodeFv".into(), 0),
                (16, "update__4NodeFv".into(), 0),
                (20, "concat__4NodeFv".into(), 0),
            ],
            false,
            true,
            4,
        );
        let weak = HashSet::from(["age__5ANodeFv", "read__8CoreNodeFv", "concat__4NodeFv"]);

        assert_eq!(
            inline_base_vtable_slots(&classes[0], &by_name, &vtable, &weak),
            Some(InlineBaseVtable {
                size: 12,
                relocations: vec![(8, "age__5ANodeFv".into(), 0)],
            })
        );
        assert_eq!(
            inline_base_vtable_slots(&classes[1], &by_name, &vtable, &weak),
            Some(InlineBaseVtable {
                size: 16,
                relocations: vec![
                    (8, "age__5ANodeFv".into(), 0),
                    (12, "read__8CoreNodeFv".into(), 0),
                ],
            })
        );
        assert_eq!(
            inline_base_vtable_slots(&classes[2], &by_name, &vtable, &weak),
            None
        );
    }

    #[test]
    fn vtable_headers_and_relocations_have_independent_abi_order() {
        let class = CxxAbiClass {
            source_name: "E".to_string(),
            rtti_name: "E".to_string(),
            encoded_name: "1E".to_string(),
            bases: Vec::new(),
            vtable_components: vec![
                CxxAbiVtableComponent {
                    table_offset: 0,
                    object_offset: 0,
                    virtual_slots: 1,
                },
                CxxAbiVtableComponent {
                    table_offset: 12,
                    object_offset: 4,
                    virtual_slots: 1,
                },
                CxxAbiVtableComponent {
                    table_offset: 24,
                    object_offset: 8,
                    virtual_slots: 1,
                },
                CxxAbiVtableComponent {
                    table_offset: 36,
                    object_offset: 12,
                    virtual_slots: 1,
                },
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
