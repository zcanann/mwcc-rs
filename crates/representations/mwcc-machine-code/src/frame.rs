//! Frame metadata: the saved-register shape a function's prologue establishes.
//!
//! A function with a stack frame carries CodeWarrior unwind tables (`extab` /
//! `extabindex`) so the runtime can walk the stack. The bytes are a deterministic
//! function of what the prologue saves — the link register, callee-saved GPRs and
//! FPRs — which is exactly what this records. The object writer encodes it; see
//! `extab_header` for the bitfield (decoded empirically from mwcceppc output).

/// What a function's prologue saves. Present only for functions that allocate a
/// stack frame (non-leaf calls, or leaf functions that need stack scratch such as
/// the float<->int conversion bounce).
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameInfo {
    /// Number of callee-saved general registers stored (r31, r30, … downward).
    pub saved_gpr_count: u8,
    /// Number of callee-saved float registers stored (f31, f30, … downward).
    pub saved_fpr_count: u8,
    /// Whether the function both makes a call and uses the FPU. This sets one bit
    /// in the `extab` header independent of whether any FPR is actually saved
    /// (a float-returning non-leaf sets it even with zero saved FPRs).
    pub fpu_in_non_leaf: bool,
}

impl FrameInfo {
    /// The 32-bit big-endian `extab` header word, decoded from mwcceppc output:
    /// `gpr_count` in bits 27‑31, `fpr_count` in bits 22‑23, the FPU/non‑leaf flag
    /// at `0x0002_0000`, and the always-present frame flag at `0x0008_0000`.
    pub fn extab_header(&self) -> u32 {
        ((self.saved_gpr_count as u32) << 27)
            | ((self.saved_fpr_count as u32) << 22)
            | if self.fpu_in_non_leaf { 0x0002_0000 } else { 0 }
            | 0x0008_0000
    }
}
