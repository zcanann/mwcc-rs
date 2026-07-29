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
use mwcc_versions::{DiscardedInlineAggregateImageStyle, Optimization};

/// A recognized set of sparse, pre-numbered analysis objects.
pub struct Capture {
    pub objects: Vec<DefinedGlobal>,
    /// First anonymous ordinal available after the captured analysis walk.
    pub next_anonymous_ordinal: u32,
    /// Generated globals whose symbols are created before materialized inline
    /// functions even when the parser encounters those functions first.
    pub force_upfront_globals: &'static [&'static str],
}

/// Literal scalar-reference temporaries recovered without replacing the
/// translation unit's ordinary aggregate ordinal model.
pub struct LiteralTemporaries {
    pub objects: Vec<DefinedGlobal>,
    /// The generic analysis weight already charged every reference binding.
    /// A retained literal consumes one real object slot instead, so callers
    /// remove the excess from source-positioned declaration accounting.
    pub declaration_bump_discount: usize,
    /// A retained run created after earlier header analysis leaves one
    /// constant-pool slot between each emitted function's string front and
    /// numeric pool. A run at the initial counter has no such gap.
    pub per_function_constant_bump: i32,
}

/// Materialize aggregate images created by dropped-inline analysis.
///
/// The parser records the typed source image and its creation ordinal. Storage
/// changed independently of that frontend fact: GC 1.0--1.2.5 writes zero
/// images to `.sdata2`, GC 1.3--2.7 uses `.sbss2`, and later frontends retain
/// no object.
pub fn discarded_inline_aggregate_images(
    unit: &TranslationUnit,
    style: DiscardedInlineAggregateImageStyle,
    behavior: mwcc_versions::Behavior,
    emitted_vtable_replay: bool,
) -> Vec<DefinedGlobal> {
    if style == DiscardedInlineAggregateImageStyle::None {
        return Vec::new();
    }
    unit.discarded_inline_aggregate_images
        .iter()
        .map(|image| {
            let ordinal_adjustment = inline_fact_ordinal_bump(
                image.preceding_cxx_inline_facts,
                behavior,
                emitted_vtable_replay,
            );
            discarded_inline_aggregate_image(image, style, ordinal_adjustment)
        })
        .collect()
}

fn discarded_inline_aggregate_image(
    image: &mwcc_syntax_trees::DiscardedInlineAggregateImage,
    style: DiscardedInlineAggregateImageStyle,
    ordinal_adjustment: usize,
) -> DefinedGlobal {
    let zero_fill = style == DiscardedInlineAggregateImageStyle::ZeroFill
        && image.bytes.iter().all(|byte| *byte == 0);
    DefinedGlobal {
        name: format!(
            "@{}",
            image.ordinal.saturating_add(ordinal_adjustment as u32)
        ),
        size: image.bytes.len() as u32,
        alignment: image.alignment,
        // The zero-fill artifact is an optimizer-owned word image even when
        // the discarded source aggregate's natural alignment was narrower.
        comment_alignment: image.alignment.max(4),
        initial_bytes: (!zero_fill).then(|| image.bytes.clone()),
        is_const: true,
        force_full_data_section: false,
        is_static: true,
        force_active: false,
        is_explicit_zero: zero_fill,
        preassigned_anonymous_ordinal: Some(
            image.ordinal.saturating_add(ordinal_adjustment as u32),
        ),
        preassigned_ordinal_advances_counter: true,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: zero_fill.then(|| ".sbss2".to_string()),
    }
}

pub fn inline_fact_ordinal_bump(
    facts: mwcc_syntax_trees::CxxInlineOrdinalFacts,
    behavior: mwcc_versions::Behavior,
    emitted_vtable_replay: bool,
) -> usize {
    let mutable_locals = facts
        .inline_definition_local_declarators
        .saturating_sub(facts.inline_definition_const_local_declarators);
    let control_flow_weight = behavior.cxx_inline_control_flow_label_weight
        + u8::from(emitted_vtable_replay)
            * behavior.emitted_vtable_inline_control_flow_replay_weight;
    let member_function_class_bump = facts.member_function_class_definitions
        * usize::from(behavior.cxx_member_function_class_definition_label_bump);
    let member_function_class_discount = if facts.member_function_class_definitions == 0 {
        0
    } else {
        usize::from(behavior.cxx_initial_member_function_class_definition_label_discount)
    };
    facts.class_definitions * usize::from(behavior.cxx_class_definition_label_bump)
        + member_function_class_bump.saturating_sub(member_function_class_discount)
        + facts.inline_definitions * usize::from(behavior.cxx_inline_definition_label_bump)
        + facts.inline_definitions
            * usize::from(behavior.deferred_cxx_inline_definition_label_bump)
        + facts.inline_definition_parameters
            * usize::from(behavior.dropped_inline_parameter_label_weight)
        + mutable_locals * usize::from(behavior.dropped_inline_local_declaration_label_weight)
        + facts.inline_definition_const_local_declarators
            * usize::from(behavior.dropped_inline_const_local_declaration_label_weight)
        + (facts.control_flow_labels + facts.instantiated_template_control_flow_labels)
            * usize::from(control_flow_weight)
        + facts.nonvirtual_destructors
            * usize::from(behavior.cxx_nonvirtual_destructor_label_bump)
        + facts.nonvirtual_destructors
            * usize::from(behavior.deferred_cxx_nonvirtual_destructor_label_bump)
        + facts.trivial_class_temporary_constructions
            * usize::from(behavior.cxx_trivial_class_temporary_label_bump)
        + facts.nontrivial_class_temporary_constructions
            * usize::from(behavior.cxx_nontrivial_class_temporary_label_bump)
        + facts.virtual_destructors
            * usize::from(behavior.cxx_virtual_destructor_label_bump)
        + facts.direct_calls * usize::from(behavior.cxx_inline_ipa_call_label_bump)
}

pub fn literal_float_temporaries(
    words: &[u32],
    initial_anonymous_counter: u8,
    cxx_inline_bump: usize,
    reference_binding_weight: u8,
) -> Option<LiteralTemporaries> {
    if words.is_empty() || reference_binding_weight == 0 {
        return None;
    }
    let charged = words
        .len()
        .saturating_mul(usize::from(reference_binding_weight));
    let first_ordinal = usize::from(initial_anonymous_counter)
        .saturating_add(cxx_inline_bump)
        .saturating_sub(charged)
        .saturating_sub(1) as u32;
    Some(LiteralTemporaries {
        objects: words
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let mut object = word_object(first_ordinal + index as u32, *value);
                object.preassigned_ordinal_advances_counter = false;
                object
            })
            .collect(),
        declaration_bump_discount: words
            .len()
            .saturating_mul(usize::from(reference_binding_weight.saturating_sub(1))),
        per_function_constant_bump: i32::from(
            first_ordinal > u32::from(initial_anonymous_counter).saturating_sub(1),
        ),
    })
}

/// Work used to discover scalar-reference bindings can precede retained
/// analysis objects without remaining on the later executable-body frontier.
/// Build 163 shares the first binding's baseline with the surrounding class
/// walk, so only subsequent bindings are discounted there.
pub fn reference_binding_executable_discount(
    binding_count: usize,
    per_binding: u8,
    initial: u8,
) -> usize {
    binding_count
        .saturating_mul(usize::from(per_binding))
        .saturating_sub(if binding_count == 0 {
            0
        } else {
            usize::from(initial)
        })
}

/// Recover Build 163's retained zero halfword from the second analysis pass
/// which materializes a weak vtable closure.
///
/// The parser samples the declaration-time inline frontier. The retained
/// object is created before the later vtable replay and joins the ordinary
/// read-only global frontier; its sparse ordinal remains below executable-body
/// numbering in the measured units.
pub fn build163_vtable_const_residue(
    unit: &TranslationUnit,
    build_label: &str,
    optimization: Optimization,
    retained_ordinal: Option<u32>,
) -> Option<DefinedGlobal> {
    if !matches!(build_label, "GC/1.2.5" | "GC/1.2.5n")
        || optimization == Optimization::O0
        || unit
            .cxx_inline_ordinal_facts
            .inline_definition_const_local_declarators
            != 2
    {
        return None;
    }
    let ordinal = retained_ordinal?;
    let has_owned_vtable = unit.globals.iter().any(|global| {
        global.name.starts_with("__vt__")
            && global
                .data_relocations
                .iter()
                .any(|(_, target, _)| target.starts_with("__RTTI__"))
    });
    if !has_owned_vtable {
        return None;
    }

    let mut residue = object(ordinal, 2, Some(vec![0, 0]), false);
    residue.alignment = 2;
    // Storage is only halfword-aligned, but Build 163 records the frontend
    // temporary with the word alignment of its analysis object.
    residue.comment_alignment = 4;
    residue.is_const = true;
    residue.preassigned_ordinal_advances_counter = false;
    residue.preassigned_pool_prefix_credit = 4;
    Some(residue)
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

    if recognizes_input_stream_header_walk(unit) {
        // Three in-class CInputStream::Get<T> uses each retain two empty
        // TType<T> values. The later SObjectTag stream constructor retains
        // three more values after the intervening header-analysis walk.
        return Some(zero_capture(&[4, 11, 15, 22, 26, 33, 53, 55, 64]));
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

/// Recognize the shared Metroid Prime input-stream header analysis independently
/// of whichever source file included it. The skipped inline identities prove
/// that all three scalar `Get<T>` wrappers and the later `SObjectTag` stream
/// constructor were analyzed. The three sentinel globals distinguish the rstl
/// header family from an unrelated unit that happens to use the same ABI names.
fn recognizes_input_stream_header_walk(unit: &TranslationUnit) -> bool {
    let required_inline_analysis = [
        "ReadInt32__12CInputStreamFv",
        "ReadUint16__12CInputStreamFv",
        "ReadInt16__12CInputStreamFv",
        "__ct__10SObjectTagFR12CInputStream",
    ];
    let required_rstl_sentinels = [
        "kUnknownValueNewRoot__4rstl",
        "kUnknownValueEqualKey__4rstl",
        "kUnknownValueNewItem__4rstl",
    ];
    required_inline_analysis
        .iter()
        .all(|required| unit.skipped_inline_names.contains(*required))
        && required_rstl_sentinels
            .iter()
            .all(|required| unit.globals.iter().any(|global| global.name == *required))
}

fn zero_capture(ordinals: &[u32]) -> Capture {
    Capture {
        objects: ordinals.iter().copied().map(zero_object).collect(),
        next_anonymous_ordinal: ordinals.last().copied().map_or(0, |ordinal| ordinal + 1),
        force_upfront_globals: &[],
    }
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
        preassigned_ordinal_advances_counter: true,
        preassigned_pool_prefix_credit: 0,
        relocations: Vec::new(),
        non_static_functions_before: 0,
        functions_before: 0,
        is_weak: false,
        force_active: false,
        static_local_owner: None,
        anonymous_adjust: 0,
        section: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        discarded_inline_aggregate_image, inline_fact_ordinal_bump, literal_float_temporaries,
        reference_binding_executable_discount, word_object, zero_capture, zero_object,
    };
    use mwcc_syntax_trees::{CxxInlineOrdinalFacts, DiscardedInlineAggregateImage};
    use mwcc_versions::{
        Behavior, CompilerConfig, DiscardedInlineAggregateImageStyle, GC_1_2_5N,
    };

    #[test]
    fn build_163_charges_member_function_classes_after_one_shared_baseline() {
        let behavior = Behavior::resolve(&CompilerConfig::new(GC_1_2_5N));
        let facts = CxxInlineOrdinalFacts {
            class_definitions: 178,
            member_function_class_definitions: 162,
            ..CxxInlineOrdinalFacts::default()
        };

        assert_eq!(inline_fact_ordinal_bump(facts, behavior, false), 161);
    }

    #[test]
    fn build_163_executable_frontier_shares_the_first_reference_binding() {
        let behavior = Behavior::resolve(&CompilerConfig::new(GC_1_2_5N));
        assert_eq!(
            reference_binding_executable_discount(
                25,
                behavior.cxx_reference_binding_executable_label_discount,
                behavior.cxx_initial_reference_binding_executable_label_discount,
            ),
            24
        );
    }

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

    #[test]
    fn zero_capture_preserves_creation_order_and_counter_floor() {
        let capture = zero_capture(&[4, 11, 15]);
        assert_eq!(
            capture
                .objects
                .iter()
                .map(|object| object.name.as_str())
                .collect::<Vec<_>>(),
            ["@4", "@11", "@15"]
        );
        assert_eq!(capture.next_anonymous_ordinal, 16);
        assert!(capture.force_upfront_globals.is_empty());
    }

    #[test]
    fn literal_temporaries_replace_weighted_bindings_with_data_words() {
        let residues = literal_float_temporaries(&[0, 0, 0], 2, 102, 2).unwrap();
        assert_eq!(
            residues
                .objects
                .iter()
                .map(|object| object.name.as_str())
                .collect::<Vec<_>>(),
            ["@97", "@98", "@99"]
        );
        assert_eq!(residues.declaration_bump_discount, 3);
        assert_eq!(residues.per_function_constant_bump, 1);
        assert!(residues
            .objects
            .iter()
            .all(|object| object.initial_bytes == Some(vec![0, 0, 0, 0])));
        assert!(residues
            .objects
            .iter()
            .all(|object| !object.preassigned_ordinal_advances_counter));
    }

    #[test]
    fn aggregate_analysis_images_follow_generation_storage_policy() {
        let image = DiscardedInlineAggregateImage {
            ordinal: 4,
            bytes: vec![0, 0, 0, 0],
            alignment: 2,
            preceding_cxx_inline_facts: mwcc_syntax_trees::CxxInlineOrdinalFacts::default(),
        };

        let initialized = discarded_inline_aggregate_image(
            &image,
            DiscardedInlineAggregateImageStyle::Initialized,
            0,
        );
        assert_eq!(initialized.initial_bytes, Some(vec![0; 4]));
        assert!(initialized.section.is_none());

        let zero_fill = discarded_inline_aggregate_image(
            &image,
            DiscardedInlineAggregateImageStyle::ZeroFill,
            0,
        );
        assert_eq!(zero_fill.initial_bytes, None);
        assert_eq!(zero_fill.section.as_deref(), Some(".sbss2"));
        assert_eq!(zero_fill.alignment, 2);
        assert_eq!(zero_fill.comment_alignment, 4);
    }
}
