//! Initialize every element of a global struct array with a retained byte offset.
//!
//! Non-power-of-two element sizes are represented by an independent induction
//! byte offset in MWCC, not by rematerializing `index * stride` at each use.  The
//! loop also retains a global owner and publishes each initialized element into
//! one of its intrusive lists.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct GlobalStructArrayInitialization<'a> {
    owner_global: &'a str,
    array_global: &'a str,
    owner_init: &'a str,
    element_init: &'a str,
    append: &'a str,
    count: i16,
    stride: i16,
    list_offset: i16,
    owner_offset: i16,
    count_offset: i16,
}

impl Generator {
    pub(crate) fn try_global_struct_array_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = recognize::classify(function) else {
            return Ok(false);
        };
        if !self.behavior.schedule_latency_slots
            || self.behavior.integer_loop_style
                != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.use_lmw_stmw
            || self.global_array_sizes.get(plan.array_global).copied()
                != Some(u32::from(plan.count as u16) * u32::from(plan.stride as u16))
        {
            return Ok(false);
        }
        emit::emit(self, &plan);
        Ok(true)
    }
}
