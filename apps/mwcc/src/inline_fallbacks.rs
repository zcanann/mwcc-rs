//! Materialize retained inline bodies that exceed automatic nesting depth.
//!
//! CodeWarrior restarts its inline budget at each emitted weak fallback. Keep
//! that orchestration out of parsing and instruction selection: the retained
//! bodies remain semantic definitions, while this module decides which ones
//! become callable object members and where they enter definition order.

use mwcc_syntax_trees::TranslationUnit;
use mwcc_syntax_trees_to_machine_code::InlineBodySet;
use std::collections::HashMap;

const MAXIMUM_NESTED_INLINE_DEPTH: usize = 2;

pub(crate) fn materialize_depth_limited(unit: &mut TranslationUnit) {
    let groups = InlineBodySet::depth_limited_fallbacks(
        &unit.functions,
        &unit.skipped_inline_definitions,
        MAXIMUM_NESTED_INLINE_DEPTH,
    );
    if groups.iter().all(Vec::is_empty) {
        return;
    }

    let retained: HashMap<_, _> = unit
        .skipped_inline_definitions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect();
    let definitions = std::mem::take(&mut unit.functions);
    let sources = std::mem::take(&mut unit.function_sources);
    let fallback_count = groups.iter().map(Vec::len).sum::<usize>();
    unit.functions = Vec::with_capacity(definitions.len() + fallback_count);
    unit.function_sources = Vec::with_capacity(definitions.len() + fallback_count);

    for (index, function) in definitions.into_iter().enumerate() {
        unit.functions.push(function);
        unit.function_sources
            .push(sources.get(index).cloned().unwrap_or(None));
        for name in &groups[index] {
            let Some(mut fallback) = retained.get(name.as_str()).map(|body| (*body).clone()) else {
                continue;
            };
            if !fallback.is_static {
                fallback.is_weak = true;
            }
            unit.functions.push(fallback);
            // Retained inline provenance is not yet public on TranslationUnit.
            // A missing entry is preferable to attaching the caller's line.
            unit.function_sources.push(None);
        }
    }
}
