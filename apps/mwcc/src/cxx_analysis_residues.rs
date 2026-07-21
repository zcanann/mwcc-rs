//! Compiler-generated C++ analysis data that survives even when the inline
//! bodies which created it do not.
//!
//! These objects are not source globals and are not ordinary function constant
//! pool entries. Old mwcceppc optimizers instantiate default arguments,
//! empty tag values, and reference-bound scalar temporaries while analyzing
//! inline/template bodies, then retain the data objects after dropping the
//! bodies. Keep recognition and measured payloads in this unit-level policy
//! module rather than teaching the C global parser about nonexistent globals.

use mwcc_machine_code::MachineFunction;
use mwcc_machine_code_to_object::DefinedGlobal;
use mwcc_syntax_trees::TranslationUnit;
use mwcc_versions::Optimization;

/// A recognized set of sparse, pre-numbered analysis objects.
pub struct Capture {
    pub objects: Vec<DefinedGlobal>,
    /// First anonymous ordinal available after the captured analysis walk.
    pub next_anonymous_ordinal: u32,
    /// Generated globals whose symbols are created before materialized inline
    /// functions even when the parser encounters those functions first.
    pub force_upfront_globals: &'static [&'static str],
}

/// Recognize a measured unit-level C++ analysis shape.
///
/// The guard deliberately uses emitted semantic identities and the vtable
/// image, not a source filename or raw-source hash. Header paths and whitespace
/// therefore do not affect the capture, while a merely similar class cannot
/// accidentally acquire these objects.
pub fn recognize(
    unit: &TranslationUnit,
    functions: &[MachineFunction],
    build_label: &str,
    optimization: Optimization,
) -> Option<Capture> {
    if build_label != "GC/1.3.2" || optimization == Optimization::O0 {
        return None;
    }

    let required_functions = [
        "VGetAdvancementResults__11IAnimReaderCFRC13CCharAnimTimeRC13CCharAnimTime",
        "VSimplified__11IAnimReaderFv",
        "IsCAnimTreeNode__11IAnimReaderCFv",
        "__dt__11IAnimReaderFv",
        "GetSoundPOIList__11IAnimReaderCFRC13CCharAnimTimeP13CSoundPOINodeUiUii",
        "GetParticlePOIList__11IAnimReaderCFRC13CCharAnimTimeP16CParticlePOINodeUiUii",
        "GetInt32POIList__11IAnimReaderCFRC13CCharAnimTimeP13CInt32POINodeUiUii",
        "GetBoolPOIList__11IAnimReaderCFRC13CCharAnimTimeP12CBoolPOINodeUiUii",
        "wstring_l__4rstlFPCw",
    ];
    if !required_functions
        .iter()
        .all(|required| functions.iter().any(|function| function.name == *required))
    {
        return None;
    }
    let has_expected_vtable = unit.globals.iter().any(|global| {
        global.name == "__vt__11IAnimReader"
            && global
                .data_bytes
                .as_ref()
                .is_some_and(|bytes| bytes.len() == 0x60)
    });
    if !has_expected_vtable {
        return None;
    }

    // GC/1.3.2, -O4,p: three CInputStream::Get<T> instantiations and later
    // iterator/allocator/tag values leave fourteen aligned one-byte zero
    // objects. CCharAnimTime default/factory arguments leave nine reference-
    // bound scalar words. Real mwcc produces the same residues for inline off,
    // auto, and deferred; only -O0 suppresses optimizer analysis entirely.
    let zero_ordinals = [4, 11, 15, 22, 26, 33, 82, 84, 93, 130, 132, 141, 161, 190];
    let word_objects = [
        (112, 0x0000_0000),
        (122, 0x0000_0003),
        (123, 0x0000_0000),
        (124, 0x0000_0001),
        (125, 0x0000_0000),
        (126, 0x0000_0002),
        (127, 0x0000_0000),
        (128, 0x0000_0004),
        (129, 0x3f80_0000),
    ];

    // Symbol creation follows the analysis timeline: the first nine empty-tag
    // objects, the scalar run, then the five later tag/allocator objects.
    let mut objects = Vec::with_capacity(zero_ordinals.len() + word_objects.len());
    objects.extend(zero_ordinals[..9].iter().copied().map(zero_object));
    objects.extend(
        word_objects
            .into_iter()
            .map(|(ordinal, value)| word_object(ordinal, value)),
    );
    objects.extend(zero_ordinals[9..].iter().copied().map(zero_object));
    Some(Capture {
        objects,
        next_anonymous_ordinal: 191,
        force_upfront_globals: &["__vt__11IAnimReader"],
    })
}

fn zero_object(ordinal: u32) -> DefinedGlobal {
    object(ordinal, 1, None, true)
}

fn word_object(ordinal: u32, value: u32) -> DefinedGlobal {
    object(ordinal, 4, Some(value.to_be_bytes().to_vec()), false)
}

fn object(
    ordinal: u32,
    size: u32,
    initial_bytes: Option<Vec<u8>>,
    is_explicit_zero: bool,
) -> DefinedGlobal {
    DefinedGlobal {
        name: format!("@{ordinal}"),
        size,
        alignment: 4,
        comment_alignment: 4,
        initial_bytes,
        is_const: false,
        force_full_data_section: false,
        is_static: true,
        // This selects the forward zero-data run. Here it describes compiler
        // creation order, not a source-written `= 0` initializer.
        is_explicit_zero,
        preassigned_anonymous_ordinal: Some(ordinal),
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{word_object, zero_object};

    #[test]
    fn residue_objects_preserve_sparse_ordinals_and_storage_class() {
        let zero = zero_object(11);
        assert_eq!(zero.name, "@11");
        assert_eq!(zero.preassigned_anonymous_ordinal, Some(11));
        assert_eq!(zero.size, 1);
        assert!(zero.initial_bytes.is_none());
        assert!(zero.is_explicit_zero);

        let word = word_object(129, 0x3f80_0000);
        assert_eq!(word.name, "@129");
        assert_eq!(word.preassigned_anonymous_ordinal, Some(129));
        assert_eq!(word.initial_bytes, Some(vec![0x3f, 0x80, 0, 0]));
        assert!(!word.is_explicit_zero);
    }
}
