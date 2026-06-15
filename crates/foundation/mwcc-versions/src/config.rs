//! The compiler configuration: which build, with which flags.
//!
//! [`CompilerConfig`] is the single source of truth a compilation is parameterized
//! by — the version we reproduce ([`CompilerBuild`]) and the codegen-affecting
//! [`Flags`]. Every pipeline stage reads its decisions from here (directly for now,
//! through a resolved `Behavior` as the system grows), so there is one place that
//! says "what compiler are we reproducing, under what options".

use crate::build::CompilerBuild;
use crate::flags::{CharDefault, Flags};

/// A fully specified compilation target: a build and its flags.
#[derive(Debug, Clone, Copy)]
pub struct CompilerConfig {
    pub build: CompilerBuild,
    pub flags: Flags,
}

impl CompilerConfig {
    /// A config for `build` with the default (canary-corpus) flags.
    pub fn new(build: CompilerBuild) -> Self {
        CompilerConfig { build, flags: Flags::default() }
    }

    /// Whether plain `char` is signed for this configuration: the build default,
    /// unless `-char signed`/`-char unsigned` overrides it.
    pub fn char_is_signed(&self) -> bool {
        match self.flags.char_default {
            CharDefault::Signed => true,
            CharDefault::Unsigned => false,
            CharDefault::BuildDefault => self.build.profile.char_is_signed(),
        }
    }
}
