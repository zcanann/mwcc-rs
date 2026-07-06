//! Per-build codegen behavior.
//!
//! Differences between mwcceppc builds are expressed as a [`CodegenProfile`]: a
//! trait whose default methods describe the GameCube 2.4.x mainline. A divergent
//! build overrides only the method that actually changed, so the shared behavior
//! — and every other build — stays untouched. Branching a new version off an
//! existing one is "add a profile struct, override one method", never a fork of
//! the whole code generator.

/// The version-varying codegen decisions. Every method defaults to the GameCube
/// 2.4.x mainline (mwcceppc build 81 through 2.4.7 build 108); a build that
/// diverges implements this trait and overrides just the differing methods.
pub trait CodegenProfile: core::fmt::Debug {
    /// Whether plain `char` (no `signed`/`unsigned` qualifier) is signed. The one
    /// knob distinguishing GC build 53 from 81+; it cascades through read/operand
    /// extension, `>>`/`/`/`%` strength reduction, comparison folding, and the
    /// int->float bias.
    fn char_is_signed(&self) -> bool {
        true
    }

    /// In the int->float conversion, whether the value store (`stw rX,12(r1)`) is
    /// scheduled before the bias load (`lfd f1,0(0)`). GC/2.0p1 alone does this —
    /// the first observed instruction-scheduling difference between builds.
    fn float_cast_value_store_first(&self) -> bool {
        false
    }

    /// In a non-leaf `if`-prologue, whether the saved-LR store (`stw r0,20(r1)`) is
    /// emitted BEFORE a leading float-constant load in the condition, rather than
    /// filling the `mflr`->store latency slot with that load. GC/2.0p1: `mflr r0;
    /// stw r0,20; lfs f0,0(0); fcmpo` vs mainline `mflr r0; lfs f0,0(0); stw r0,20;
    /// fcmpo`. The same "store before a float load" family as
    /// [`Self::float_cast_value_store_first`].
    fn lr_save_precedes_float_const(&self) -> bool {
        false
    }

    /// In a float `if`-condition comparing a LOADED value (member/global) against a
    /// pool CONSTANT, whether the value operand is loaded BEFORE the constant. GC/2.0p1:
    /// `lfs f1,(v); lfs f0,k` vs mainline `lfs f0,k; lfs f1,(v)` (which hoists the
    /// independent constant to fill the prologue latency slot). Same 2.0p1 float-reorder
    /// family; the register assignment (`fcmpo f1,f0`) is unchanged, only the load order.
    fn float_compare_value_before_const(&self) -> bool {
        false
    }

    /// In `frexp`, whether the mantissa scaling (`fmul`) is emitted before the
    /// `*eptr = <exp>` integer store. GC/2.0p1: `fmul; stw r0,0(r3)` vs mainline
    /// `stw r0,0(r3); fmul` — the two are independent, so it is purely a schedule
    /// difference. Same 2.0p1 float-reorder family.
    fn frexp_scale_before_eptr_store(&self) -> bool {
        false
    }
}

/// GameCube 2.4.x mainline — the reference behavior (all defaults). Covers
/// GC/1.3.2 (build 81), 1.3.2r, 2.0, 2.5, 2.6, and 2.7.
#[derive(Debug)]
pub struct Mainline;
impl CodegenProfile for Mainline {}

/// GC/1.3 — mwcceppc 2.4.2 build 53. The early 2.4.2 build that defaulted plain
/// `char` to unsigned, before build 81 restored signed.
#[derive(Debug)]
pub struct Gc13Build53;
impl CodegenProfile for Gc13Build53 {
    fn char_is_signed(&self) -> bool {
        false
    }
}

/// GC/2.0p1 — mwcceppc 2.4.7 build 92, patch 1. Mainline except it schedules the
/// int->float value store before the bias load.
#[derive(Debug)]
pub struct Gc20Patch1;
impl CodegenProfile for Gc20Patch1 {
    fn float_cast_value_store_first(&self) -> bool {
        true
    }
    fn lr_save_precedes_float_const(&self) -> bool {
        true
    }
    fn float_compare_value_before_const(&self) -> bool {
        true
    }
    fn frexp_scale_before_eptr_store(&self) -> bool {
        true
    }
}
