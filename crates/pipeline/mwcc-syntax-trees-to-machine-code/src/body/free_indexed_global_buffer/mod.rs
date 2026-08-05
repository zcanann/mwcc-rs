//! Acquire the first unused object from a small indexed global pool.
//!
//! This transaction composes three source helpers: the range-guarded indexed
//! lookup, a reset whose false-only clearing arm is dead for `keep_data = 1`,
//! and a scalar member setter. Recognition proves all three bodies before the
//! measured predecrement schedule inlines them.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct Plan {
    array: String,
    bound: i16,
    stride: i16,
    used_offset: i16,
    length_offset: i16,
    position_offset: i16,
    unavailable: i16,
    success: i16,
    acquire: String,
    release: String,
    report: String,
    report_text: Vec<u8>,
}

impl Generator {
    pub(crate) fn try_free_indexed_global_buffer(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.global_addressing != GlobalAddressing::Absolute
            || !self.behavior.use_lmw_stmw
            || !self.behavior.repeatable_scalar_member_setter_inlining
        {
            return Ok(false);
        }
        let Some(plan) = recognize::classify(self, function) else {
            return Ok(false);
        };
        emit::emit(self, &plan)?;
        Ok(true)
    }
}
