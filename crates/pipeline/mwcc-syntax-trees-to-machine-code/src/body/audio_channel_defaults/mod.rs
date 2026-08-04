//! Initialize a logical audio channel from either built-in or manager defaults.
//!
//! The null-manager arm is a constant packet, while the manager arm contains
//! two counted copies.  Both rejoin a pointer-array clear and a wrapping serial
//! update, forming one physical register schedule in legacy MWCC.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

impl Generator {
    pub(crate) fn try_audio_channel_defaults(&mut self, function: &Function) -> Compilation<bool> {
        if !recognize::matches(function) {
            return Ok(false);
        }
        if !self.behavior.schedule_latency_slots
            || self.behavior.integer_loop_style
                != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.integer_select_style
                != mwcc_versions::IntegerSelectStyle::BranchPreserving
        {
            return Ok(false);
        }
        emit::emit(self);
        Ok(true)
    }
}
