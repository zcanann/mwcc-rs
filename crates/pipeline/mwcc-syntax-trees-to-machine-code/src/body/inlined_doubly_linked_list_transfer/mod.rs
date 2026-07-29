//! Compose a verified doubly-linked-list extraction into a free-list transfer.
//!
//! The small extraction helper is inlined while the larger insertion helper
//! remains a call. Their shared descriptor, cell pointer, and frame schedule
//! form one interprocedural transaction.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

impl Generator {
    pub(crate) fn try_inlined_doubly_linked_list_transfer(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(mut shape) = recognize::classify(function) else {
            return Ok(false);
        };
        let Some(extract) = self
            .inline_bodies
            .definition_body(shape.extract_helper)
            .and_then(super::doubly_linked_list_extract::summarize)
        else {
            return Ok(false);
        };
        if self.skipped_inline_names.contains(shape.insert_helper) || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        shape.previous_offset = extract.previous_offset;
        shape.next_offset = extract.next_offset;
        match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => self.emit_legacy_list_transfer(&shape),
            FrameConvention::Predecrement => self.emit_modern_list_transfer(&shape),
        }
        Ok(true)
    }
}
