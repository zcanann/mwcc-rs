//! The build registry: one [`CompilerBuild`] per mwcceppc build we reproduce,
//! each pointing at the [`CodegenProfile`] that says how its codegen diverges
//! from the mainline. Adding a build is one entry here; adding a *behavior* is a
//! profile in [`crate::profile`].

use crate::profile::{CodegenProfile, Gc13Build53, Gc20Patch1, Mainline};

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
    /// How this build's code generation diverges from the 2.4.x mainline.
    pub profile: &'static dyn CodegenProfile,
}

/// GC/1.3 — mwcceppc 2.4.2 build 53. The earliest preserved 2.4.2 build; the
/// only one in the supported range that defaults plain `char` to unsigned.
pub const GC_1_3: CompilerBuild = CompilerBuild {
    label: "GC/1.3",
    product: "CodeWarrior for GameCube 1.3",
    version: (2, 4, 2),
    build: 53,
    profile: &Gc13Build53,
};

/// GC/1.3.2 — mwcceppc 2.4.2 build 81 (built 2002-05-07), the reference build.
pub const GC_1_3_2: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2",
    product: "CodeWarrior for GameCube 1.3.2",
    version: (2, 4, 2),
    build: 81,
    profile: &Mainline,
};

/// GC/1.3.2r — mwcceppc 2.4.2 build 81, byte-identical to GC/1.3.2 (a re-release).
pub const GC_1_3_2R: CompilerBuild = CompilerBuild {
    label: "GC/1.3.2r",
    product: "CodeWarrior for GameCube 1.3.2 (r)",
    version: (2, 4, 2),
    build: 81,
    profile: &Mainline,
};

/// GC/2.0 — mwcceppc 2.4.7 build 92.
pub const GC_2_0: CompilerBuild = CompilerBuild {
    label: "GC/2.0",
    product: "CodeWarrior for GameCube 2.0",
    version: (2, 4, 7),
    build: 92,
    profile: &Mainline,
};

/// GC/2.0p1 — mwcceppc 2.4.7 build 92, patch 1. Identical to GC/2.0 except the
/// int->float conversion schedules the value store before the bias load.
pub const GC_2_0P1: CompilerBuild = CompilerBuild {
    label: "GC/2.0p1",
    product: "CodeWarrior for GameCube 2.0 (patch 1)",
    version: (2, 4, 7),
    build: 92,
    profile: &Gc20Patch1,
};

/// GC/2.5 — mwcceppc 2.4.7 build 105.
pub const GC_2_5: CompilerBuild = CompilerBuild {
    label: "GC/2.5",
    product: "CodeWarrior for GameCube 2.5",
    version: (2, 4, 7),
    build: 105,
    profile: &Mainline,
};

/// GC/2.6 — mwcceppc 2.4.7 build 107.
pub const GC_2_6: CompilerBuild = CompilerBuild {
    label: "GC/2.6",
    product: "CodeWarrior for GameCube 2.6",
    version: (2, 4, 7),
    build: 107,
    profile: &Mainline,
};

/// GC/2.7 — mwcceppc 2.4.7 build 108.
pub const GC_2_7: CompilerBuild = CompilerBuild {
    label: "GC/2.7",
    product: "CodeWarrior for GameCube 2.7",
    version: (2, 4, 7),
    build: 108,
    profile: &Mainline,
};

/// Every build the generator reproduces byte-for-byte across the canary suite.
pub const SUPPORTED: &[CompilerBuild] =
    &[GC_1_3, GC_1_3_2, GC_1_3_2R, GC_2_0, GC_2_0P1, GC_2_5, GC_2_6, GC_2_7];

/// The default build new compilations target until one is selected.
pub const DEFAULT: CompilerBuild = GC_1_3_2;

/// Look up a build by its decomp label (e.g. "GC/1.3.2" or the bare "1.3.2").
pub fn by_label(label: &str) -> Option<CompilerBuild> {
    SUPPORTED
        .iter()
        .copied()
        .find(|build| build.label == label || build.label.strip_prefix("GC/") == Some(label))
}
