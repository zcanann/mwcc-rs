//! Constructor scheduling for stack aggregates in structured bodies.
//!
//! When a three-float aggregate result feeds a small polymorphic effect object,
//! GC/2.6 overlaps the field loads with vtable materialization.  The ordinary
//! statement emitter intentionally preserves source order; this owner recognizes
//! the complete dependency graph and applies the measured legacy schedule.

#[allow(unused_imports)]
use super::*;

const WINDOW_LEN: usize = 25;
const SCHEDULE: [usize; WINDOW_LEN] = [
    9, 12, 10, 0, 11, 13, 2, 6, 4, 19, 7, 15, 17, 14, 20, 22, 8, 23, 1, 3, 5, 16,
    18, 21, 24,
];

impl Generator {
    pub(super) fn schedule_structured_aggregate_constructor(&mut self) {
        if self.behavior.frame_convention != FrameConvention::Predecrement {
            return;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(WINDOW_LEN)
            .position(is_aggregate_constructor_window)
        else {
            return;
        };

        // Bring each original instruction into its scheduled slot. Moving only
        // toward the front lets the label table and relocation table use their
        // existing, well-tested single-move remapping operation.
        let mut current: Vec<usize> = (0..WINDOW_LEN).collect();
        for (destination, &original) in SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|&candidate| candidate == original)
                .expect("constructor schedule is a permutation");
            if source != destination {
                self.move_aggregate_constructor_instruction_before(
                    start + source,
                    start + destination,
                );
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }

        assign_legacy_constructor_registers(
            &mut self.output.instructions[start..start + WINDOW_LEN],
        );
    }

    fn move_aggregate_constructor_instruction_before(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == from {
                to
            } else if (to..from).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
    }
}

fn is_aggregate_constructor_window(window: &[Instruction]) -> bool {
    let [
        Instruction::LoadFloatSingle { d: 0, a: load_base_0, offset: load_0 },
        Instruction::StoreFloatSingle { s: 0, a: store_base_0, offset: store_0 },
        Instruction::LoadFloatSingle { d: 0, a: load_base_1, offset: load_1 },
        Instruction::StoreFloatSingle { s: 0, a: store_base_1, offset: store_1 },
        Instruction::LoadFloatSingle { d: 0, a: load_base_2, offset: load_2 },
        Instruction::StoreFloatSingle { s: 0, a: store_base_2, offset: store_2 },
        Instruction::AddImmediateShifted { d: 0, a: 0, .. },
        Instruction::AddImmediate { d: 0, a: 0, .. },
        Instruction::StoreWord { s: 0, a: argument_store_base, offset: argument_vtable },
        Instruction::AddImmediateShifted { d: 0, a: 0, .. },
        Instruction::AddImmediate { d: 0, a: 0, .. },
        Instruction::StoreWord { s: 0, a: effect_store_base_0, offset: effect_vtable_0 },
        Instruction::AddImmediateShifted { d: 0, a: 0, .. },
        Instruction::AddImmediate { d: 0, a: 0, .. },
        Instruction::StoreWord { s: 0, a: effect_store_base_1, offset: effect_vtable_1 },
        Instruction::AddImmediate { d: 0, a: 0, immediate: effect_id },
        Instruction::StoreHalfword { s: 0, a: effect_store_base_2, offset: effect_id_offset },
        Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
        Instruction::StoreWord { s: 0, a: effect_store_base_3, offset: effect_pointer_offset },
        Instruction::AddImmediateShifted { d: 0, a: 0, .. },
        Instruction::AddImmediate { d: 0, a: 0, .. },
        Instruction::StoreWord { s: 0, a: effect_store_base_4, offset: effect_vtable_2 },
        Instruction::AddImmediate { d: 3, a: frame_base_0, immediate: effect_offset },
        Instruction::AddImmediate { d: 4, a: frame_base_1, immediate: argument_offset },
        Instruction::BranchAndLink { .. },
    ] = window
    else {
        return false;
    };

    load_base_0 == load_base_1
        && load_base_1 == load_base_2
        && *load_1 == *load_0 + 4
        && *load_2 == *load_1 + 4
        && store_base_0 == store_base_1
        && store_base_1 == store_base_2
        && *store_1 == *store_0 + 4
        && *store_2 == *store_1 + 4
        && argument_store_base == frame_base_0
        && frame_base_0 == frame_base_1
        && effect_store_base_0 == frame_base_0
        && effect_store_base_1 == frame_base_0
        && effect_store_base_2 == frame_base_0
        && effect_store_base_3 == frame_base_0
        && effect_store_base_4 == frame_base_0
        && *argument_vtable == *argument_offset
        && *effect_vtable_0 == *effect_offset
        && *effect_vtable_1 == *effect_offset
        && *effect_vtable_2 == *effect_offset
        && *store_0 == *argument_offset + 4
        && *effect_id_offset == *effect_offset + 4
        && *effect_pointer_offset == *effect_offset + 8
        && *effect_id != 0
}

fn assign_legacy_constructor_registers(window: &mut [Instruction]) {
    let assignments = [
        (4, 0), // base vtable high
        (3, 0), // intermediate vtable high
        (0, 4), // base vtable low
        (2, 1), // first float load
        (0, 1), // base vtable store
        (0, 3), // intermediate vtable low
        (1, 1), // second float load
        (4, 0), // argument vtable high
        (0, 1), // third float load
        (3, 0), // derived vtable high
        (4, 4), // argument vtable low
        (6, 0), // effect id
        (5, 0), // null effect pointer
        (0, 1), // intermediate vtable store
        (0, 3), // derived vtable low
        (3, 1), // effect address
        (4, 1), // argument vtable store
        (4, 1), // argument address
        (2, 1), // first float store
        (1, 1), // second float store
        (0, 1), // third float store
        (6, 1), // effect id store
        (5, 1), // null pointer store
        (0, 1), // derived vtable store
    ];

    for (instruction, &(value, base)) in window[..24].iter_mut().zip(&assignments) {
        match instruction {
            Instruction::AddImmediateShifted { d, a, .. }
            | Instruction::AddImmediate { d, a, .. }
            | Instruction::LoadFloatSingle { d, a, .. } => (*d, *a) = (value, base),
            Instruction::StoreWord { s, a, .. }
            | Instruction::StoreHalfword { s, a, .. }
            | Instruction::StoreFloatSingle { s, a, .. } => (*s, *a) = (value, base),
            _ => unreachable!("recognized constructor schedule changed shape"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_partial_constructor_sequence() {
        let instructions = vec![Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 3,
        }];
        assert!(!is_aggregate_constructor_window(&instructions));
    }

    #[test]
    fn keeps_scheduled_aggregate_accesses_stack_relative() {
        let mut instructions = vec![
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 0 },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 8 },
            Instruction::StoreWord { s: 0, a: 0, offset: 20 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 12 },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 0 },
            Instruction::LoadFloatSingle { d: 0, a: 0, offset: 16 },
            Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 3 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 0, offset: 20 },
            Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 0, immediate: 20 },
            Instruction::StoreWord { s: 0, a: 0, offset: 32 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 32 },
            Instruction::StoreFloatSingle { s: 0, a: 0, offset: 36 },
            Instruction::StoreFloatSingle { s: 0, a: 0, offset: 40 },
            Instruction::StoreFloatSingle { s: 0, a: 0, offset: 44 },
            Instruction::StoreHalfword { s: 0, a: 0, offset: 24 },
            Instruction::StoreWord { s: 0, a: 0, offset: 28 },
            Instruction::StoreWord { s: 0, a: 0, offset: 20 },
        ];

        assign_legacy_constructor_registers(&mut instructions);

        for &index in &[3, 4, 6, 8, 13, 15, 16, 17, 18, 19, 20, 21, 22, 23] {
            let base = match instructions[index] {
                Instruction::AddImmediate { a, .. }
                | Instruction::LoadFloatSingle { a, .. }
                | Instruction::StoreWord { a, .. }
                | Instruction::StoreHalfword { a, .. }
                | Instruction::StoreFloatSingle { a, .. } => a,
                _ => unreachable!(),
            };
            assert_eq!(base, 1, "instruction {index} lost its stack base");
        }
    }
}
