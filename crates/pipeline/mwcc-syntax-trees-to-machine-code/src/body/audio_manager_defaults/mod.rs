//! Initialize the fixed scalar, filter-bank, and routing defaults of an audio manager.
//!
//! Two small counted fills sit inside a long heterogeneous store schedule.  MWCC
//! keeps the shared integer constants and pointer-relative induction values live
//! across that complete region, so ordinary statement-at-a-time lowering cannot
//! reproduce its register assignment or latency slots.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

impl Generator {
    pub(crate) fn try_audio_manager_defaults(&mut self, function: &Function) -> Compilation<bool> {
        if !recognize::matches(function) {
            return Ok(false);
        }
        if !self.behavior.schedule_latency_slots
            || self.behavior.integer_loop_style
                != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.read_only_global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        emit::emit(self);
        Ok(true)
    }
}
