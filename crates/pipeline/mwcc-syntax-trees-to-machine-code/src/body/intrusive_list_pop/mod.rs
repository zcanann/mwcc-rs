//! Pop the head of a singly linked intrusive list.
//!
//! The returned node stays live across two stores while the null arm returns
//! immediately.  MWCC assigns a stable register schedule to that whole region;
//! splitting it into an if and sequential stores loses the load-latency moves.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct IntrusiveListPop {
    next_offset: i16,
    owner_offset: i16,
}

impl Generator {
    pub(crate) fn try_intrusive_list_pop(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = recognize::classify(function) else {
            return Ok(false);
        };
        if !self.behavior.schedule_latency_slots
            || self.behavior.integer_select_style
                != mwcc_versions::IntegerSelectStyle::BranchPreserving
        {
            return Ok(false);
        }
        emit::emit(self, &plan);
        Ok(true)
    }
}
