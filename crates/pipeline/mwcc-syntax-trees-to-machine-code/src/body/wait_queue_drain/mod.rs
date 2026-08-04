//! Drain a bounded pointer queue, optionally stopping after one successful item.
//!
//! Recognition and emission are deliberately separate. The queue mutation is
//! a reusable semantic transaction; the build-163 register and branch schedule
//! is one measured realization of that transaction.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct WaitQueueDrain<'a> {
    table: &'a str,
    index: &'a str,
    count: &'a str,
    bound: u16,
    object_result_offset: i16,
    object_manager_offset: i16,
    manager_list_offset: i16,
    allocate: &'a str,
    play: &'a str,
    cut: &'a str,
    append: &'a str,
}

impl Generator {
    pub(crate) fn try_wait_queue_drain(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = recognize::classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || !self.behavior.schedule_latency_slots
            || !self.behavior.use_lmw_stmw
            || self.global_array_sizes.get(plan.table).copied()
                != Some(u32::from(plan.bound) * 4)
            || !self.globals.contains_key(plan.index)
            || !self.globals.contains_key(plan.count)
        {
            return Ok(false);
        }
        emit::emit(self, &plan);
        Ok(true)
    }
}
