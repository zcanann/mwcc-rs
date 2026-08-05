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
mod recognize;

use recognize::{classify, classify_inline_read, StackUnpack};

impl Generator {
    pub(crate) fn try_endian_stack_unpack(&mut self, function: &Function) -> Compilation<bool> {
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
