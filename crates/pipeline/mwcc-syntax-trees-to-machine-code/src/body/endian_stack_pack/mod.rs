//! Endian-dependent scalar packing through address-taken stack storage.
//!
//! Recognition is source-structural. Emission is split by frame generation and
//! by the measured deferred-inlining policy so a verified append helper can be
//! composed without teaching the generic expression emitter about 64-bit frame
//! images.

#[allow(unused_imports)]
use super::*;

mod emit_linkage_first;
mod emit_predecrement_call;
mod emit_predecrement_inline;
mod recognize;

use recognize::{classify, classify_inline_append};

impl Generator {
    pub(crate) fn try_endian_stack_pack(&mut self, function: &Function) -> Compilation<bool> {
        let Some(plan) = classify(function, &self.globals) else {
            return Ok(false);
        };

        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst
                if self.behavior.plain_linkage_epilogue_style
                    == PlainLinkageEpilogueStyle::StackRestoreBeforeReload =>
            {
                emit_linkage_first::emit(self, &plan);
                Ok(true)
            }
            FrameConvention::Predecrement if plan.width == 8 => {
                let should_inline = self.behavior.deferred_inlining
                    && self.behavior.automatic_inlining_enabled
                    && match self.behavior.endian_stack_pack_inlining_style {
                        mwcc_versions::EndianStackPackInliningStyle::InlineVerifiedAppend => true,
                        mwcc_versions::EndianStackPackInliningStyle::InlineSingleUseAppend => {
                            self.inline_bodies.definition_call_count(plan.callee) == 1
                        }
                    };
                let inline = should_inline
                    .then(|| self.inline_bodies.definition_body(plan.callee))
                    .flatten()
                    .and_then(classify_inline_append);
                if let Some(inline) = inline {
                    emit_predecrement_inline::emit(self, &plan, &inline);
                } else {
                    emit_predecrement_call::emit(self, &plan);
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}
