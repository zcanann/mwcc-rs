//! Scalar reads whose destination is selected through a temporary stack image.
//!
//! Wrapper and callee recognition is source-structural. Frame-specific emitters
//! own the measured instruction schedules, including deferred composition of a
//! fully verified bounded-read transaction.

#[allow(unused_imports)]
use super::*;

mod emit_linkage_first;
mod emit_predecrement_direct;
mod emit_predecrement_endian;
mod emit_predecrement_loop_direct;
mod emit_predecrement_loop_endian;
mod recognize;
mod recognize_loop;

use recognize::{classify, classify_inline_read, StackUnpack};
use recognize_loop::{classify_read_loop, ReadLoopCallee};

impl Generator {
    pub(crate) fn try_endian_stack_unpack(&mut self, function: &Function) -> Compilation<bool> {
        if let Some(loop_plan) = classify_read_loop(function) {
            if self.behavior.frame_convention != FrameConvention::Predecrement
                || !self.behavior.deferred_inlining
                || !self.behavior.automatic_inlining_enabled
            {
                return Ok(false);
            }
            let (callee, flag) = match &loop_plan.callee {
                ReadLoopCallee::Core(callee) => ((*callee).to_owned(), None),
                ReadLoopCallee::Wrapper(wrapper_callee) => {
                    let Some(wrapper) = self
                        .inline_bodies
                        .definition_body(wrapper_callee)
                        .and_then(|definition| classify(definition, &self.globals))
                    else {
                        return Ok(false);
                    };
                    match wrapper {
                        StackUnpack::Direct(wrapper) if wrapper.width == loop_plan.width => {
                            (wrapper.callee.to_owned(), None)
                        }
                        StackUnpack::Endian(wrapper) if wrapper.width == loop_plan.width => {
                            (wrapper.callee.to_owned(), Some(wrapper.flag.to_owned()))
                        }
                        _ => return Ok(false),
                    }
                }
            };
            let Some(read) = self
                .inline_bodies
                .definition_body(&callee)
                .and_then(classify_inline_read)
            else {
                return Ok(false);
            };
            match (loop_plan.width, flag) {
                (1, None) => {
                    emit_predecrement_loop_direct::emit(self, &loop_plan, &read);
                    return Ok(true);
                }
                (4, Some(flag)) if self.behavior.global_addressing == GlobalAddressing::Absolute => {
                    emit_predecrement_loop_endian::emit(self, &loop_plan, &flag, &read);
                    return Ok(true);
                }
                _ => return Ok(false),
            }
        }

        let Some(plan) = classify(function, &self.globals) else {
            return Ok(false);
        };

        match (&plan, self.behavior.frame_convention) {
            (
                StackUnpack::Endian(plan),
                FrameConvention::LinkageFirst,
            ) if self.behavior.plain_linkage_epilogue_style
                == PlainLinkageEpilogueStyle::StackRestoreBeforeReload =>
            {
                emit_linkage_first::emit(self, plan);
                Ok(true)
            }
            (_, FrameConvention::Predecrement)
                if self.behavior.deferred_inlining
                    && self.behavior.automatic_inlining_enabled =>
            {
                let Some(read) = self
                    .inline_bodies
                    .definition_body(plan.callee())
                    .and_then(classify_inline_read)
                else {
                    return Ok(false);
                };
                match &plan {
                    StackUnpack::Direct(plan) if plan.width == 1 => {
                        emit_predecrement_direct::emit(self, plan, &read);
                        Ok(true)
                    }
                    StackUnpack::Endian(plan) if matches!(plan.width, 4 | 8) => {
                        emit_predecrement_endian::emit(self, plan, &read);
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            _ => Ok(false),
        }
    }
}
