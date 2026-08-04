//! Release a manager's fixed intrusive-list bank into a global manager.
//!
//! The final bank has distinct semantics: each waiting object is cancelled and
//! published to the primary global list. Recognition and build-specific emission
//! stay separate so that exception cannot be silently treated as a uniform copy.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct ReleaseListBankToGlobal<'a> {
    global: &'a str,
    source_offsets: [i16; 4],
    destination_offsets: [i16; 4],
    count_offset: i16,
    object_owner_offset: i16,
    take: &'a str,
    append: &'a str,
    cancel: &'a str,
}

impl Generator {
    pub(crate) fn try_release_list_bank_to_global(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = recognize::classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.schedule_latency_slots
            || !self.behavior.use_lmw_stmw
            || !self.full_bss_globals.contains(plan.global)
        {
            return Ok(false);
        }
        emit::emit(self, &plan);
        Ok(true)
    }
}
