//! Late anonymous-ordinal assignment for compiler-generated RTTI data.
//!
//! Class parsing and ABI data construction intentionally happen before code
//! generation, while MWCC assigns the auxiliary `@N` names after its function
//! analysis walk. This small driver boundary reconciles those two timelines.

use std::collections::HashMap;

use mwcc_machine_code_to_object::DefinedGlobal;

const PREFIX: &str = "@@cxx_rtti:";

pub fn resolve(globals: &mut [DefinedGlobal], mut counter: u32) {
    let mut renames = HashMap::new();
    for global in globals.iter() {
        if global.name.starts_with(PREFIX) {
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
