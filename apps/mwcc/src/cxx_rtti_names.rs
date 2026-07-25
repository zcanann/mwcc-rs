//! Late anonymous-ordinal assignment for compiler-generated RTTI data.
//!
//! Class parsing and ABI data construction intentionally happen before code
//! generation, while MWCC assigns the auxiliary `@N` names after its function
//! analysis walk. This small driver boundary reconciles those two timelines.

use std::collections::{HashMap, HashSet};

use mwcc_machine_code_to_object::DefinedGlobal;
use mwcc_syntax_trees::CxxInlineOrdinalFacts;

const PREFIX: &str = "@@cxx_rtti:";

#[derive(Clone, Copy)]
pub struct AnalysisWeights {
    pub virtual_method: u8,
    pub virtual_destructor: u8,
    pub inherited_virtual_destructor: u8,
    pub initial_virtual_discount: u8,
}

pub fn is_single_fragmented_debug_class(facts: CxxInlineOrdinalFacts) -> bool {
    facts.class_definitions == 1
        && facts.inline_definitions == 1
        && facts.virtual_destructors == 1
        && facts.virtual_method_declarations == 0
        && facts.virtual_destructor_declarations == 1
        && facts.inherited_virtual_destructor_declarations == 0
        && facts.direct_calls == 0
        && facts.control_flow_labels == 0
}

/// Resolve the class-analysis counter independently from executable function
/// numbering. The first polymorphic declaration shares one profile-specific
/// baseline block; subsequent declarations pay their full syntax-kind weight.
pub fn analysis_counter(
    initial: u8,
    strings_before: u32,
    prior_declaration_bump: usize,
    facts: CxxInlineOrdinalFacts,
    weights: AnalysisWeights,
    sparse_floor: u32,
) -> u32 {
    let virtual_declarations =
        facts.virtual_method_declarations + facts.virtual_destructor_declarations;
    let virtual_bump = (facts.virtual_method_declarations * usize::from(weights.virtual_method)
        + facts.virtual_destructor_declarations * usize::from(weights.virtual_destructor))
    .saturating_sub(if virtual_declarations == 0 {
        0
    } else {
        usize::from(weights.initial_virtual_discount)
    }) + facts.inherited_virtual_destructor_declarations
        * usize::from(weights.inherited_virtual_destructor);
    (u32::from(initial) + strings_before + prior_declaration_bump as u32 + virtual_bump as u32)
        .max(sparse_floor)
}

/// GC 4.1's smallest owned-class debug unit shares its RTTI-name allocation
/// with the fragmented line/type preamble instead of the ordinary C++ analysis
/// counter. Return that measured base only for the fully identified shape.
pub fn fragmented_debug_counter(
    ordinary_counter: u32,
    facts: CxxInlineOrdinalFacts,
) -> Option<u32> {
    is_single_fragmented_debug_class(facts).then(|| ordinary_counter.saturating_sub(2))
}

/// Reuse a function-owned string object when it carries the exact bytes of an
/// RTTI type name. Build 163 performs this pooling before assigning the
/// remaining RTTI helper ordinals.
pub fn coalesce_name_strings(
    globals: &mut Vec<DefinedGlobal>,
    function_strings: &[DefinedGlobal],
) {
    let replacements: HashMap<String, String> = globals
        .iter()
        .filter(|global| global.name.starts_with(PREFIX) && global.name.ends_with(":name"))
        .filter_map(|global| {
            function_strings
                .iter()
                .find(|string| string.initial_bytes == global.initial_bytes)
                .map(|string| (global.name.clone(), string.name.clone()))
        })
        .collect();
    if replacements.is_empty() {
        return;
    }
    globals.retain(|global| !replacements.contains_key(&global.name));
    for global in globals {
        for relocation in &mut global.relocations {
            if let Some(replacement) = replacements.get(&relocation.target) {
                relocation.target = replacement.clone();
            }
        }
    }
}

pub fn resolve(globals: &mut [DefinedGlobal], mut counter: u32, owned_closure_schedule: bool) {
    if owned_closure_schedule {
        // Build 163 reserves one closure-ownership label between class
        // analysis and the first owned RTTI name.
        counter += 1;
    }
    let analysis_base = counter;
    let mut renames = HashMap::new();
    let ordinal_order = if owned_closure_schedule {
        owned_closure_ordinal_order(globals)
    } else {
        globals
            .iter()
            .filter(|global| global.name.starts_with(PREFIX))
            .map(|global| Some(global.name.clone()))
            .collect()
    };
    for name in ordinal_order {
        let Some(name) = name else {
            counter += 1;
            continue;
        };
        if let Some(global) = globals.iter().find(|global| global.name == name) {
            // Weak all-inline vtables are first owned only after their source
            // constructor frontier. Keep ordinary key-function RTTI on the
            // early class-analysis timeline, but let a late generated object
            // establish the corresponding source-function floor.
            if !owned_closure_schedule {
                counter = counter.max(
                    analysis_base.saturating_add(
                        u32::try_from(global.functions_before).unwrap_or(u32::MAX),
                    ),
                );
            }
            renames.insert(name, format!("@{counter}"));
            counter += 1;
        }
    }
    for global in globals {
        if let Some(name) = renames.get(&global.name) {
            global.name = name.clone();
            global.preassigned_anonymous_ordinal = name
                .strip_prefix('@')
                .and_then(|ordinal| ordinal.parse().ok());
        }
        for relocation in &mut global.relocations {
            if let Some(name) = renames.get(&relocation.target) {
                relocation.target = name.clone();
            }
        }
    }
}

fn owned_closure_ordinal_order(globals: &[DefinedGlobal]) -> Vec<Option<String>> {
    let helper = |name: &str| name.starts_with(PREFIX);
    let handles_referenced_by_bases: HashSet<&str> = globals
        .iter()
        .filter(|global| global.name.starts_with(PREFIX) && global.name.ends_with(":bases"))
        .flat_map(|global| global.relocations.iter())
        .map(|relocation| relocation.target.as_str())
        .filter(|target| target.starts_with("__RTTI__"))
        .collect();
    let mut root_handles = Vec::new();
    let mut seen_roots = HashSet::new();
    for target in globals
        .iter()
        .filter(|global| global.name.starts_with("__vt__"))
        .flat_map(|global| global.relocations.iter())
        .map(|relocation| relocation.target.as_str())
        .filter(|target| target.starts_with("__RTTI__"))
    {
        if !handles_referenced_by_bases.contains(target) && seen_roots.insert(target) {
            root_handles.push(target);
        }
    }

    let mut order = Vec::new();
    let mut seen = HashSet::new();
    let mut append_handle_helpers = |handle_name: &str, order: &mut Vec<Option<String>>| {
        let Some(handle) = globals.iter().find(|global| global.name == handle_name) else {
            return;
        };
        for offset in [0, 4] {
            if let Some(target) = handle
                .relocations
                .iter()
                .find(|relocation| relocation.offset == offset)
                .map(|relocation| relocation.target.as_str())
            {
                if !helper(target) {
                    if offset == 0 && target.starts_with('@') {
                        order.push(None);
                    }
                    continue;
                }
                if seen.insert(target.to_string()) {
                    order.push(Some(target.to_string()));
                }
            }
        }
    };
    for root in root_handles {
        append_handle_helpers(root, &mut order);
        let base_handles: Vec<String> = globals
            .iter()
            .find(|global| global.name == root)
            .into_iter()
            .flat_map(|handle| handle.relocations.iter())
            .find_map(|relocation| {
                (helper(&relocation.target) && relocation.target.ends_with(":bases"))
                    .then_some(relocation.target.as_str())
            })
            .and_then(|bases| globals.iter().find(|global| global.name == bases))
            .into_iter()
            .flat_map(|bases| bases.relocations.iter())
            .map(|relocation| relocation.target.clone())
            .collect();
        for base in base_handles {
            append_handle_helpers(&base, &mut order);
        }
    }
    for global in globals {
        if helper(&global.name) && seen.insert(global.name.clone()) {
            order.push(Some(global.name.clone()));
        }
    }
    order
}

#[cfg(test)]
mod tests {
    use super::{
        analysis_counter, coalesce_name_strings, fragmented_debug_counter,
        owned_closure_ordinal_order, AnalysisWeights,
    };
    use mwcc_machine_code_to_object::{DataRelocation, DefinedGlobal};
    use mwcc_syntax_trees::CxxInlineOrdinalFacts;

    fn facts(methods: usize, destructors: usize) -> CxxInlineOrdinalFacts {
        CxxInlineOrdinalFacts {
            virtual_method_declarations: methods,
            virtual_destructor_declarations: destructors,
            ..CxxInlineOrdinalFacts::default()
        }
    }

    fn object(name: &str, bytes: &[u8], target: Option<&str>) -> DefinedGlobal {
        DefinedGlobal {
            name: name.into(),
            size: bytes.len() as u32,
            alignment: 4,
            comment_alignment: 4,
            initial_bytes: Some(bytes.to_vec()),
            is_const: false,
            force_full_data_section: false,
            is_static: true,
            force_active: false,
            is_explicit_zero: false,
            preassigned_anonymous_ordinal: None,
            preassigned_ordinal_advances_counter: false,
            relocations: target
                .map(|target| DataRelocation {
                    offset: 0,
                    target: target.into(),
                    addend: 0,
                })
                .into_iter()
                .collect(),
            non_static_functions_before: 0,
            functions_before: 0,
            is_weak: false,
            static_local_owner: None,
            anonymous_adjust: 0,
            section: None,
        }
    }

    #[test]
    fn rtti_name_reuses_an_identical_function_string() {
        let name = "@@cxx_rtti:4Node:name";
        let mut globals = vec![
            object(name, b"Node\0", None),
            object("__RTTI__4Node", &[0; 8], Some(name)),
        ];
        let strings = vec![object("@388", b"Node\0", None)];

        coalesce_name_strings(&mut globals, &strings);

        assert_eq!(globals.len(), 1);
        assert_eq!(globals[0].relocations[0].target, "@388");
    }

    #[test]
    fn owned_closure_ordinals_visit_root_then_transitive_bases() {
        let boss_name = "@@cxx_rtti:4Boss:name";
        let boss_bases = "@@cxx_rtti:4Boss:bases";
        let base_name = "@@cxx_rtti:4Base:name";
        let mut boss_handle = object("__RTTI__4Boss", &[0; 8], Some(boss_name));
        boss_handle.relocations.push(DataRelocation {
            offset: 4,
            target: boss_bases.into(),
            addend: 0,
        });
        let mut boss_vtable = object("__vt__4Boss", &[0; 12], Some("__RTTI__4Boss"));
        boss_vtable.is_static = false;
        let globals = vec![
            object(base_name, b"Base\0", None),
            object("__RTTI__4Base", &[0; 8], Some(base_name)),
            object(boss_bases, &[0; 12], Some("__RTTI__4Base")),
            object(boss_name, b"Boss\0", None),
            boss_vtable,
            boss_handle,
        ];

        assert_eq!(
            owned_closure_ordinal_order(&globals),
            [Some(boss_name.into()), Some(boss_bases.into()), Some(base_name.into())]
        );
    }

    #[test]
    fn owned_closure_ordinals_reserve_a_pooled_root_name() {
        let boss_bases = "@@cxx_rtti:4Boss:bases";
        let mut boss_handle = object("__RTTI__4Boss", &[0; 8], Some("@388"));
        boss_handle.relocations.push(DataRelocation {
            offset: 4,
            target: boss_bases.into(),
            addend: 0,
        });
        let mut boss_vtable = object("__vt__4Boss", &[0; 12], Some("__RTTI__4Boss"));
        boss_vtable.is_static = false;
        let globals = vec![
            object(boss_bases, &[0; 4], None),
            boss_vtable,
            boss_handle,
        ];

        assert_eq!(
            owned_closure_ordinal_order(&globals),
            [None, Some(boss_bases.into())]
        );
    }

    #[test]
    fn measured_profile_weights_assign_rtti_name_bases() {
        let cases = [
            (
                2,
                AnalysisWeights {
                    virtual_method: 1,
                    virtual_destructor: 3,
                    inherited_virtual_destructor: 2,
                    initial_virtual_discount: 1,
                },
                [2, 3, 4, 7, 9],
            ),
            (
                5,
                AnalysisWeights {
                    virtual_method: 4,
                    virtual_destructor: 6,
                    inherited_virtual_destructor: 2,
                    initial_virtual_discount: 4,
                },
                [5, 9, 7, 13, 15],
            ),
            (
                5,
                AnalysisWeights {
                    virtual_method: 5,
                    virtual_destructor: 9,
                    inherited_virtual_destructor: 4,
                    initial_virtual_discount: 4,
                },
                [6, 11, 10, 19, 23],
            ),
            (
                5,
                AnalysisWeights {
                    virtual_method: 4,
                    virtual_destructor: 7,
                    inherited_virtual_destructor: 0,
                    initial_virtual_discount: 4,
                },
                [5, 9, 8, 15, 15],
            ),
        ];
        for (initial, weights, expected) in cases {
            assert_eq!(
                analysis_counter(initial, 0, 0, facts(1, 0), weights, 0),
                expected[0]
            );
            assert_eq!(
                analysis_counter(initial, 0, 0, facts(2, 0), weights, 0),
                expected[1]
            );
            assert_eq!(
                analysis_counter(initial, 0, 0, facts(0, 1), weights, 0),
                expected[2]
            );
            assert_eq!(
                analysis_counter(initial, 0, 0, facts(0, 2), weights, 0),
                expected[3]
            );
            let inherited = CxxInlineOrdinalFacts {
                inherited_virtual_destructor_declarations: 1,
                ..facts(0, 2)
            };
            assert_eq!(
                analysis_counter(initial, 0, 0, inherited, weights, 0),
                expected[4]
            );
        }
    }

    #[test]
    fn fragmented_single_class_reserves_the_line_and_type_preamble() {
        let facts = CxxInlineOrdinalFacts {
            class_definitions: 1,
            inline_definitions: 1,
            virtual_destructors: 1,
            virtual_destructor_declarations: 1,
            ..CxxInlineOrdinalFacts::default()
        };
        assert_eq!(fragmented_debug_counter(17, facts), Some(15));
    }
}
