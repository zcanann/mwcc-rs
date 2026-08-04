//! Transfer every object in a fixed bank of intrusive lists between managers.
//!
//! Recognition owns the source-level transaction while emission owns the
//! measured linkage-first schedule. Keeping them separate lets later builds
//! retain the semantic family even when their frame or loop spelling differs.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct FixedListBankTransfer<'a> {
    list_offsets: [i16; 4],
    count_offsets: [i16; 2],
    object_owner_offset: i16,
    take: &'a str,
    append: &'a str,
}

impl Generator {
    pub(crate) fn try_fixed_list_bank_transfer(
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
        {
            return Ok(false);
        }
        emit::emit(self, &plan);
        Ok(true)
    }
}
