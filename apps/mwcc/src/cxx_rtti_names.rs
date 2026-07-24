//! Late anonymous-ordinal assignment for compiler-generated RTTI data.
//!
//! Class parsing and ABI data construction intentionally happen before code
//! generation, while MWCC assigns the auxiliary `@N` names after its function
//! analysis walk. This small driver boundary reconciles those two timelines.

use std::collections::HashMap;

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

pub fn resolve(globals: &mut [DefinedGlobal], mut counter: u32) {
    let analysis_base = counter;
    let mut renames = HashMap::new();
    for global in globals.iter() {
        if global.name.starts_with(PREFIX) {
            // Weak all-inline vtables are first owned only after their source
            // constructor frontier. Keep ordinary key-function RTTI on the
            // early class-analysis timeline, but let a late generated object
            // establish the corresponding source-function floor.
            counter = counter.max(
                analysis_base.saturating_add(
                    u32::try_from(global.functions_before).unwrap_or(u32::MAX),
                ),
            );
            renames.insert(global.name.clone(), format!("@{counter}"));
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

#[cfg(test)]
mod tests {
    use super::{analysis_counter, fragmented_debug_counter, AnalysisWeights};
    use mwcc_syntax_trees::CxxInlineOrdinalFacts;

    fn facts(methods: usize, destructors: usize) -> CxxInlineOrdinalFacts {
        CxxInlineOrdinalFacts {
            virtual_method_declarations: methods,
            virtual_destructor_declarations: destructors,
            ..CxxInlineOrdinalFacts::default()
        }
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
