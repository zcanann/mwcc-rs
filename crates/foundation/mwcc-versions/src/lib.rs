//! The compiler-build registry.
//!
//! Byte-matching is per-build: "GC/1.3.2" is a label for mwcceppc internal
//! version 2.4.2 build 81, and its register allocator and scheduler differ from
//! adjacent builds. Codegen is parameterized by [`CompilerBuild`] so a single
//! source tree can target many versions; behavior differences are expressed as
//! explicit knobs rather than scattered version checks.

/// A specific mwcceppc build we aim to reproduce byte-for-byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompilerBuild {
    /// The decomp-community label, e.g. "GC/1.3.2".
    pub label: &'static str,
    /// Marketed product line, e.g. "CodeWarrior for GameCube 1.3.2".
    pub product: &'static str,
    /// Internal compiler version, e.g. (2, 4, 2).
    pub version: (u8, u8, u8),
    /// Internal build number, e.g. 81.
    pub build: u16,
}

/// GC/1.3.2 — mwcceppc 2.4.2 build 81 (built 2002-05-07). The first target.
pub const GC_1_3_2: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2",
    product: "CodeWarrior for GameCube 1.3.2",
    version: (2, 4, 2),
    build: 81,
};

/// The default build new compilations target until one is selected.
pub const DEFAULT: CompilerBuild = GC_1_3_2;

/// Look up a build by its decomp label (e.g. "GC/1.3.2").
pub fn by_label(label: &str) -> Option<CompilerBuild> {
    [GC_1_3_2].into_iter().find(|build| build.label == label)
}
