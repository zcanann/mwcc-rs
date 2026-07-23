//! The build registry: one [`CompilerBuild`] per mwcceppc build we reproduce,
//! each pointing at the [`CodegenProfile`] that says how its codegen diverges
//! from the mainline. Adding a build is one entry here; adding a *behavior* is a
//! profile in [`crate::profile`].

use crate::profile::{
    CodegenProfile, Gc132Build81, Gc13Build53, Gc20Patch1, Gc41Build51213, Mainline,
    MainlineEarlyAggregateLoads,
    Wii43Build145, GC233_BUILD159_PATCH1, GC233_BUILD163,
};

/// A specific mwcceppc build we aim to reproduce byte-for-byte.
#[derive(Debug, Clone, Copy)]
pub struct CompilerBuild {
    /// The decomp-community label, e.g. "GC/1.3.2".
    pub label: &'static str,
    /// Marketed product line, e.g. "CodeWarrior for GameCube 1.3.2".
    pub product: &'static str,
    /// Internal compiler version, e.g. (2, 4, 2).
    pub version: (u8, u8, u8),
    /// Internal build number, e.g. 81.
    pub build: u16,
    /// Marker byte used by this compiler's Metrowerks `.comment` record.
    pub comment_marker: u8,
    /// Version tuple encoded in `.comment`.  This is deliberately separate
    /// from `version`: legacy 2.3.3 executables identify their object format as
    /// 2.3.0.
    pub comment_version: (u8, u8, u8),
    /// Byte offset within an instruction used by SDA21 relocation records.
    pub emb_sda21_offset: u8,
    /// Required alignment of the code section and every function within it.
    pub code_alignment: u8,
    /// Whether `.sdata2` carries ELF's writable flag. GameCube compilers mark
    /// their constant pool writable; Wii build 145 marks it read-only.
    pub sdata2_writable: bool,
    /// Whether a global function symbol precedes externals first referenced by
    /// its body (2.3.3), instead of following prototyped externals (2.4.x).
    pub function_symbol_before_references: bool,
    /// Initial value of the translation unit's anonymous `@N` counter.
    pub initial_anonymous_counter: u8,
    /// Anonymous-ordinal gap inserted after a frameless leaf function.
    pub post_leaf_function_anonymous_bump: u8,
    /// Anonymous-ordinal gap inserted after a function with unwind metadata.
    pub post_framed_function_anonymous_bump: u8,
    /// How this build's code generation diverges from the 2.4.x mainline.
    pub profile: &'static dyn CodegenProfile,
}

/// GC/1.1 — mwcceppc 2.3.3 build 159.
pub const GC_1_1: CompilerBuild = CompilerBuild {
    label: "GC/1.1",
    product: "CodeWarrior for GameCube 1.1",
    build: 159,
    profile: &GC233_BUILD163,
    ..GC_1_2_5
};

/// GC/1.1p1 — patch 1 of the same preserved build 159 compiler.
pub const GC_1_1P1: CompilerBuild = CompilerBuild {
    label: "GC/1.1p1",
    product: "CodeWarrior for GameCube 1.1 (patch 1)",
    profile: &GC233_BUILD159_PATCH1,
    ..GC_1_1
};

/// GC/1.2.5 — mwcceppc 2.3.3 build 163. Kept experimental until its complete
/// frame scheduler and object conventions pass the full corpus.
pub const GC_1_2_5: CompilerBuild = CompilerBuild {
    label: "GC/1.2.5",
    product: "CodeWarrior for GameCube 1.2.5",
    version: (2, 3, 3),
    build: 163,
    comment_marker: 0x08,
    comment_version: (2, 3, 0),
    emb_sda21_offset: 2,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: true,
    initial_anonymous_counter: 2,
    post_leaf_function_anonymous_bump: 1,
    post_framed_function_anonymous_bump: 1,
    profile: &GC233_BUILD163,
};

/// GC/1.2.5n — Nintendo's distribution of the same compiler build.
pub const GC_1_2_5N: CompilerBuild = CompilerBuild {
    label: "GC/1.2.5n",
    product: "CodeWarrior for GameCube 1.2.5 (Nintendo)",
    version: (2, 3, 3),
    build: 163,
    comment_marker: 0x08,
    comment_version: (2, 3, 0),
    emb_sda21_offset: 2,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: true,
    initial_anonymous_counter: 2,
    post_leaf_function_anonymous_bump: 1,
    post_framed_function_anonymous_bump: 1,
    profile: &GC233_BUILD163,
};

/// GC/1.3 — mwcceppc 2.4.2 build 53. The earliest preserved 2.4.2 build; the
/// only one in the supported range that defaults plain `char` to unsigned.
pub const GC_1_3: CompilerBuild = CompilerBuild {
    label: "GC/1.3",
    product: "CodeWarrior for GameCube 1.3",
    version: (2, 4, 2),
    build: 53,
    comment_marker: 0x0a,
    comment_version: (2, 4, 2),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc13Build53,
};

/// GC/1.3.2 — mwcceppc 2.4.2 build 81 (built 2002-05-07), the reference build.
pub const GC_1_3_2: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2",
    product: "CodeWarrior for GameCube 1.3.2",
    version: (2, 4, 2),
    build: 81,
    comment_marker: 0x0a,
    comment_version: (2, 4, 2),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc132Build81,
};

/// GC/1.3.2r — Animal Crossing's hacked build 81 variant. It disabled `.rodata`
/// pooling; retained as a compatibility label but excluded from required parity
/// inventories now that the underlying stock GC/1.3.2 bug is understood.
pub const GC_1_3_2R: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2r",
    product: "CodeWarrior for GameCube 1.3.2 (r)",
    version: (2, 4, 2),
    build: 81,
    comment_marker: 0x0a,
    comment_version: (2, 4, 2),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc132Build81,
};

/// GC/2.0 — mwcceppc 2.4.7 build 92.
pub const GC_2_0: CompilerBuild = CompilerBuild {
    label: "GC/2.0",
    product: "CodeWarrior for GameCube 2.0",
    version: (2, 4, 7),
    build: 92,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Mainline,
};

/// GC/2.0p1 — mwcceppc 2.4.7 build 92, patch 1. Identical to GC/2.0 except the
/// int->float conversion schedules the value store before the bias load.
pub const GC_2_0P1: CompilerBuild = CompilerBuild {
    label: "GC/2.0p1",
    product: "CodeWarrior for GameCube 2.0 (patch 1)",
    version: (2, 4, 7),
    build: 92,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc20Patch1,
};

/// GC/2.5 — mwcceppc 2.4.7 build 105.
pub const GC_2_5: CompilerBuild = CompilerBuild {
    label: "GC/2.5",
    product: "CodeWarrior for GameCube 2.5",
    version: (2, 4, 7),
    build: 105,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &MainlineEarlyAggregateLoads,
};

/// GC/2.6 — mwcceppc 2.4.7 build 107.
pub const GC_2_6: CompilerBuild = CompilerBuild {
    label: "GC/2.6",
    product: "CodeWarrior for GameCube 2.6",
    version: (2, 4, 7),
    build: 107,
    comment_marker: 0x0a,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &MainlineEarlyAggregateLoads,
};

/// GC/2.7 — mwcceppc 2.4.7 build 108.
pub const GC_2_7: CompilerBuild = CompilerBuild {
    label: "GC/2.7",
    product: "CodeWarrior for GameCube 2.7",
    version: (2, 4, 7),
    build: 108,
    comment_marker: 0x0b,
    comment_version: (2, 4, 7),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: false,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &MainlineEarlyAggregateLoads,
};

/// GC/3.0a3 — mwcceppc 4.1 build 51213. Its object conventions include a newer
/// `.comment` identity and four-ordinal ordinary post-function steps, alongside
/// a substantial optimizer transition. Fragmented debug can redistribute one
/// framed-function ordinal into the preceding debug-visible analysis block;
/// that flag-dependent adjustment belongs to the fragmented object plan.
pub const GC_3_0A3: CompilerBuild = CompilerBuild {
    label: "GC/3.0a3",
    product: "CodeWarrior for GameCube 3.0 alpha 3",
    version: (4, 1, 0),
    build: 51213,
    comment_marker: 0x0e,
    comment_version: (4, 0, 0),
    emb_sda21_offset: 0,
    code_alignment: 4,
    sdata2_writable: true,
    function_symbol_before_references: true,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Gc41Build51213,
};

/// GC/3.0a3p1 — Twilight Princess' patched build 51213. The patch accepts
/// multi-character constants; its measured object identity and ordinary
/// codegen are otherwise the GC/3.0a3 build above.
pub const GC_3_0A3P1: CompilerBuild = CompilerBuild {
    label: "GC/3.0a3p1",
    product: "CodeWarrior for GameCube 3.0 alpha 3 (patch 1)",
    ..GC_3_0A3
};

/// Wii/1.0 — mwcceppc 4.3 build 145. The newer optimizer remains under
/// characterization; measured object conventions include 16-byte code
/// alignment and a read-only `.sdata2` constant pool.
pub const WII_1_0: CompilerBuild = CompilerBuild {
    label: "Wii/1.0",
    product: "CodeWarrior for Wii 1.0",
    version: (4, 3, 0),
    build: 145,
    comment_marker: 0x0f,
    comment_version: (4, 0, 0),
    emb_sda21_offset: 0,
    code_alignment: 16,
    sdata2_writable: false,
    function_symbol_before_references: true,
    initial_anonymous_counter: 5,
    post_leaf_function_anonymous_bump: 4,
    post_framed_function_anonymous_bump: 4,
    profile: &Wii43Build145,
};

/// Every build the generator reproduces byte-for-byte across the canary suite.
pub const SUPPORTED: &[CompilerBuild] = &[
    GC_1_3, GC_1_3_2, GC_1_3_2R, GC_2_0, GC_2_0P1, GC_2_5, GC_2_6, GC_2_7,
];

/// Known compiler identities whose profiles are still incomplete. They are
/// available only through the explicit experimental-build opt-in.
pub const EXPERIMENTAL: &[CompilerBuild] = &[
    GC_1_1, GC_1_1P1, GC_1_2_5, GC_1_2_5N, GC_3_0A3, GC_3_0A3P1, WII_1_0,
];

/// The default build new compilations target until one is selected.
pub const DEFAULT: CompilerBuild = GC_1_3_2;

/// Look up a build by its decomp label (e.g. "GC/1.3.2" or the bare "1.3.2").
pub fn by_label(label: &str) -> Option<CompilerBuild> {
    SUPPORTED
        .iter()
        .copied()
        .find(|build| build.label == label || build.label.strip_prefix("GC/") == Some(label))
}

/// Look up either a supported or explicitly experimental build.
pub fn by_label_experimental(label: &str) -> Option<CompilerBuild> {
    by_label(label).or_else(|| {
        EXPERIMENTAL
            .iter()
            .copied()
            .find(|build| build.label == label || build.label.strip_prefix("GC/") == Some(label))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_build_is_opt_in_and_keeps_distinct_object_conventions() {
        assert!(by_label("1.2.5n").is_none());
        let build = by_label_experimental("1.2.5n").expect("experimental build");
        assert_eq!(build.version, (2, 3, 3));
        assert_eq!(build.comment_version, (2, 3, 0));
        assert_eq!(build.comment_marker, 0x08);
        assert_eq!(build.emb_sda21_offset, 2);
        assert!(build.function_symbol_before_references);
        assert_eq!(build.initial_anonymous_counter, 2);
        assert_eq!(build.post_leaf_function_anonymous_bump, 1);
        assert_eq!(build.post_framed_function_anonymous_bump, 1);
    }

    #[test]
    fn gc11_builds_are_separate_experimental_identities() {
        for label in ["GC/1.1", "GC/1.1p1"] {
            assert!(by_label(label).is_none());
            let build = by_label_experimental(label).expect("experimental GC/1.1 build");
            assert_eq!(build.version, (2, 3, 3));
            assert_eq!(build.build, 159);
            assert_eq!(build.comment_marker, 0x08);
            assert_eq!(build.comment_version, (2, 3, 0));
        }
    }

    #[test]
    fn supported_builds_retain_mainline_object_numbering() {
        assert!(SUPPORTED.iter().all(|build| build.emb_sda21_offset == 0));
        assert!(SUPPORTED
            .iter()
            .all(|build| !build.function_symbol_before_references));
        assert!(SUPPORTED
            .iter()
            .all(|build| build.initial_anonymous_counter == 5));
        assert!(SUPPORTED
            .iter()
            .all(|build| build.post_leaf_function_anonymous_bump == 4));
        assert!(SUPPORTED
            .iter()
            .all(|build| build.post_framed_function_anonymous_bump == 4));
    }

    #[test]
    fn later_builds_register_functions_before_body_references() {
        assert!(GC_3_0A3P1.function_symbol_before_references);
        assert!(WII_1_0.function_symbol_before_references);
    }

    #[test]
    fn gc3_builds_are_measured_experimental_identities() {
        assert!(by_label("GC/3.0a3").is_none());
        for label in ["GC/3.0a3", "GC/3.0a3p1"] {
            let build = by_label_experimental(label).expect("experimental GC/3 build");
            assert_eq!(build.version, (4, 1, 0));
            assert_eq!(build.build, 51213);
            assert_eq!(build.comment_marker, 0x0e);
            assert_eq!(build.comment_version, (4, 0, 0));
            assert_eq!(build.initial_anonymous_counter, 5);
            assert_eq!(build.post_leaf_function_anonymous_bump, 4);
            assert_eq!(build.post_framed_function_anonymous_bump, 4);
            assert_eq!(build.profile.large_aggregate_comment_alignment(), 4);
        }
    }

    #[test]
    fn wii_build_145_has_measured_object_conventions() {
        assert!(by_label("Wii/1.0").is_none());
        let build = by_label_experimental("Wii/1.0").expect("experimental Wii build");
        assert_eq!(build.version, (4, 3, 0));
        assert_eq!(build.build, 145);
        assert_eq!(build.comment_marker, 0x0f);
        assert_eq!(build.comment_version, (4, 0, 0));
        assert_eq!(build.code_alignment, 16);
        assert_eq!(build.profile.large_aggregate_comment_alignment(), 8);
        assert!(!build.sdata2_writable);
    }
}
