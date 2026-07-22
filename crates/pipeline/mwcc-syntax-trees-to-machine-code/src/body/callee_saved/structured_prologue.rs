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

/// Select MWCC's contiguous GPR save form for structured frames. Eager locals
/// are safe here only when lifetime coloring has merged a later local into an
/// expired parameter home; that is the measured shape where the legacy
/// allocator changes from individually scheduled saves to one dense range.
pub(super) fn uses_dense_saved_register_range(
    with_frame_array: bool,
    eager_local_count: usize,
    saved_home_count: usize,
    global_member_search_entry: bool,
    reuses_parameter_home: bool,
) -> bool {
    with_frame_array
        && saved_home_count <= 18
        && (saved_home_count >= 5 || (global_member_search_entry && saved_home_count >= 4))
        && (eager_local_count == 0 || reuses_parameter_home)
}

impl Generator {
    pub(super) fn schedule_dense_eager_initializer(&mut self, start: usize) {
        if !matches!(
            self.output.instructions.get(start),
            Some(Instruction::MultiplyImmediate { .. })
        ) || !matches!(
            self.output.instructions.get(start + 1),
            Some(Instruction::AddImmediateShifted { a: 0, .. })
        ) {
            return;
        }
        self.output.instructions.swap(start, start + 1);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start => start + 1,
                index if index == start + 1 => start,
                index => index,
            };
        }
    }

    pub(super) fn try_emit_structured_wide_saved_initializer(
        &mut self,
        initializer: &Expression,
        home: u8,
    ) -> bool {
        let Some(value) = constant_value(initializer) else {
            return false;
        };
        let value = value as i32;
        if (-0x8000..=0x7fff).contains(&value) {
            return false;
        }
        let low = (value as u32 & 0xffff) as i16;
        if low == 0 {
            return false;
        }
        let high_adjusted = ((value - i32::from(low)) >> 16) as i16;
        let scratch = Eabi::general_result().number;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(scratch, high_adjusted));
        self.output.instructions.push(Instruction::AddImmediate {
            d: home,
            a: scratch,
            immediate: low,
        });
        true
    }

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
            self.emit_structured_saved_home_store(home, first_home_index + range_index, frame_size);
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

    #[test]
    fn expired_parameter_reuse_enables_a_dense_eager_frame() {
        assert!(uses_dense_saved_register_range(true, 4, 12, false, true));
        assert!(!uses_dense_saved_register_range(true, 4, 12, false, false));
    }
}
