//! Frontend-analysis residues shared by discarded SDK inlines and deferred bodies.
//!
//! Most dropped-inline work advances every later anonymous-symbol owner. The
//! Dolphin fast-cast/GX header group is different: its retained inline-assembly
//! analysis overlaps three slots of a later timer-wait body's own ordinal block.
//! Keeping that translation-unit interaction here avoids teaching either the
//! timer scheduler or the object writer about unrelated SDK declarations.

use mwcc_machine_code::MachineFunction;
use mwcc_versions::DeferredFunctionEmissionStyle;
use std::collections::HashSet;

const DOLPHIN_GX_MARKERS: [&str; 4] = [
    "GXPosition2f32",
    "GXPosition3f32",
    "GXTexCoord2f32",
    "GXEnd",
];

pub(crate) fn apply(
    functions: &mut [MachineFunction],
    inline_asm_symbols: &[String],
    skipped_inline_names: &HashSet<String>,
    emission_style: DeferredFunctionEmissionStyle,
) {
    if emission_style != DeferredFunctionEmissionStyle::ImmediateAsmThenReverseCompiled
        || !inline_asm_symbols
            .iter()
            .any(|name| name == "OSInitFastCast")
        || !DOLPHIN_GX_MARKERS
            .iter()
            .all(|name| skipped_inline_names.contains(*name))
    {
        return;
    }

    // The source-order timer wait owns the only characterized capture prefix
    // in this SDK transaction. Its seven-slot local block overlaps three slots
    // already consumed by OSInitFastCast's discarded assembly analysis.
    if let Some(function) = functions
        .iter_mut()
        .find(|function| function.deferred_source_prefix_bump == 7)
    {
        function.anonymous_label_bump = function.anonymous_label_bump.saturating_sub(3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlaps_the_dolphin_fast_cast_analysis_with_a_timer_wait() {
        let mut ordinary = MachineFunction::new("ordinary".to_string());
        ordinary.anonymous_label_bump = 2;
        let mut wait = MachineFunction::new("wait".to_string());
        wait.anonymous_label_bump = 7;
        wait.deferred_source_prefix_bump = 7;
        let mut functions = vec![wait, ordinary];
        let symbols = vec!["OSInitFastCast".to_string()];
        let skipped = DOLPHIN_GX_MARKERS
            .iter()
            .map(|name| (*name).to_string())
            .collect();

        apply(
            &mut functions,
            &symbols,
            &skipped,
            DeferredFunctionEmissionStyle::ImmediateAsmThenReverseCompiled,
        );

        assert_eq!(functions[0].anonymous_label_bump, 4);
        assert_eq!(functions[1].anonymous_label_bump, 2);
    }

    #[test]
    fn leaves_unrelated_inline_assembly_transactions_unchanged() {
        let mut wait = MachineFunction::new("wait".to_string());
        wait.anonymous_label_bump = 7;
        wait.deferred_source_prefix_bump = 7;
        let mut functions = vec![wait];

        apply(
            &mut functions,
            &["helper".to_string()],
            &HashSet::new(),
            DeferredFunctionEmissionStyle::ImmediateAsmThenReverseCompiled,
        );

        assert_eq!(functions[0].anonymous_label_bump, 7);
    }
}
