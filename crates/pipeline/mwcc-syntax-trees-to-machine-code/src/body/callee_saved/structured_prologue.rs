//! Saved-home store scheduling for general structured bodies.
//!
//! Build 163 establishes every entry-time home before it evaluates pointer
//! initializers when all three lifetime classes are present: entry-initialized
//! locals, an incoming parameter, and a local assigned later in the body. The
//! incoming parameter is copied immediately after its own save; deferred homes
//! follow that copy. Keeping this decision outside the body emitter makes the
//! prologue schedule explicit and independently testable.

#[allow(unused_imports)]
use super::*;

pub(super) fn saved_home_stores_precede_initialization(
    frame_convention: FrameConvention,
    eager_local_count: usize,
    saved_parameter_count: usize,
    deferred_home_count: usize,
) -> bool {
    frame_convention == FrameConvention::LinkageFirst
        && eager_local_count >= 2
        && saved_parameter_count != 0
        && deferred_home_count != 0
}

impl Generator {
    pub(super) fn emit_structured_saved_home_store(
        &mut self,
        home: u8,
        home_index: usize,
        frame_size: i16,
    ) {
        self.output.instructions.push(Instruction::StoreWord {
            s: home,
            a: 1,
            offset: frame_size - 4 * (home_index as i16 + 1),
        });
    }

    pub(super) fn emit_structured_saved_home_store_range(
        &mut self,
        homes: &[u8],
        first_home_index: usize,
        frame_size: i16,
    ) {
        for (range_index, &home) in homes.iter().enumerate() {
            self.emit_structured_saved_home_store(
                home,
                first_home_index + range_index,
                frame_size,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_mixed_lifetime_classes_batch_their_saved_home_stores() {
        assert!(saved_home_stores_precede_initialization(
            FrameConvention::LinkageFirst,
            2,
            1,
            1,
        ));
    }

    #[test]
    fn simpler_or_predecrement_prologues_keep_their_existing_schedule() {
        assert!(!saved_home_stores_precede_initialization(
            FrameConvention::LinkageFirst,
            1,
            1,
            1,
        ));
        assert!(!saved_home_stores_precede_initialization(
            FrameConvention::Predecrement,
            2,
            1,
            1,
        ));
    }
}
