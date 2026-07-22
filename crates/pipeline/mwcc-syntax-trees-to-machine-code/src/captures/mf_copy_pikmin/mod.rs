//! Pikmin's early serial schedules for the four MSL bulk-copy workers.
//!
//! These are one measured source family. The shared semantic gate prevents the
//! four exact emitters from independently widening to unrelated runtime
//! variants; each schedule remains isolated in its own module.

mod aligned;
mod reverse_aligned;
mod reverse_unaligned;
mod unaligned;

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Function, Type};
use mwcc_versions::{MemCopyRemainderMaskStyle, MemCopyWordScheduleStyle};

const COPY_ALIGNED_HASH: u64 = 0x376d_6392_9c45_3ff1;
const COPY_REV_ALIGNED_HASH: u64 = 0x8f23_4348_41a7_9cf4;
const COPY_UNALIGNED_HASH: u64 = 0xe6e1_81c7_adaf_051a;
const COPY_REV_UNALIGNED_HASH: u64 = 0xac52_e9e1_1479_e50f;
const PIKMIN_CONTEXT: u64 = 0x6262_16a8_cf3d_36f5;

impl Generator {
    pub(super) fn try_mf_copy_pikmin(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.parameters.len() != 3
            || !self.frame_slots.is_empty()
            || self.behavior.mem_copy_word_schedule_style != MemCopyWordScheduleStyle::SerialScratch
            || self.behavior.mem_copy_remainder_mask_style
                != MemCopyRemainderMaskStyle::MaterializedThree
            || super::skipped_context_fingerprint(&self.skipped_inline_names) != PIKMIN_CONTEXT
        {
            return Ok(false);
        }

        let hash = super::ast_hash(function);
        let emitter: fn(&mut Self) = match (function.name.as_str(), hash) {
            ("__copy_longs_aligned", COPY_ALIGNED_HASH) => Self::emit_pikmin_copy_aligned,
            ("__copy_longs_rev_aligned", COPY_REV_ALIGNED_HASH) => {
                Self::emit_pikmin_copy_rev_aligned
            }
            ("__copy_longs_unaligned", COPY_UNALIGNED_HASH) => Self::emit_pikmin_copy_unaligned,
            ("__copy_longs_rev_unaligned", COPY_REV_UNALIGNED_HASH) => {
                Self::emit_pikmin_copy_rev_unaligned
            }
            _ => return Ok(false),
        };

        self.output.pre_scheduled = true;
        emitter(self);
        Ok(true)
    }
}
