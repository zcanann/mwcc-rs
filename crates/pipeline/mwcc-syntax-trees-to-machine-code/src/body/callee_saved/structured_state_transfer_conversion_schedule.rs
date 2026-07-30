//! Integer-to-float scheduling for an object-state pointer transaction.
//!
//! The secondary guarded pointer call converts a signed state member while
//! loading two other arguments. MWCC assigns the conversion its final frame
//! image first, then overlaps independent receiver and pointer loads with the
//! conversion latency.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

const SCHEDULE: [usize; 14] = [3, 6, 7, 0, 4, 5, 1, 8, 2, 9, 10, 11, 12, 13];

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_conversion_schedule(
        &mut self,
        function: &Function,
    ) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let Some(start) = allocated_state_conversion(&self.output.instructions) else {
            return;
        };

        rewrite_state_conversion_operands(&mut self.output.instructions[start..start + 14]);
        self.apply_state_conversion_schedule(start);
        self.output.instructions[start + 12] = Instruction::move_register(3, 27);
    }

    fn apply_state_conversion_schedule(&mut self, start: usize) {
        let mut current: Vec<_> = (0..SCHEDULE.len()).collect();
        for (destination, &original) in SCHEDULE.iter().enumerate() {
            let source = current
                .iter()
                .position(|candidate| *candidate == original)
                .expect("the state conversion schedule is a permutation");
            if source != destination {
                self.move_instruction_before(start + source, start + destination);
                let moved = current.remove(source);
                current.insert(destination, moved);
            }
        }
    }
}

fn rewrite_state_conversion_operands(window: &mut [Instruction]) {
    match &mut window[3] {
        Instruction::LoadWord { d, .. } => *d = 4,
        _ => unreachable!("the converted state member load was matched"),
    }
    match &mut window[4] {
        Instruction::XorImmediateShifted { a, s, .. } => {
            *a = 4;
            *s = 4;
        }
        _ => unreachable!("the signed state conversion xor was matched"),
    }
    match &mut window[5] {
        Instruction::StoreWord { s, offset, .. } => {
            *s = 4;
            *offset = 36;
        }
        _ => unreachable!("the conversion low-word store was matched"),
    }
    match &mut window[8] {
        Instruction::StoreWord { offset, .. } => *offset = 32,
        _ => unreachable!("the conversion high-word store was matched"),
    }
    match &mut window[9] {
        Instruction::LoadFloatDouble { offset, .. } => *offset = 32,
        _ => unreachable!("the conversion image load was matched"),
    }
}

fn allocated_state_conversion(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(14).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: 3,
                    a: 30,
                    immediate: 0,
                },
                Instruction::LoadWord { d: 4, a: 31, .. },
                Instruction::LoadWord { d: 5, a: 31, .. },
                Instruction::LoadWord { d: 6, a: 31, .. },
                Instruction::XorImmediateShifted {
                    a: 0,
                    s: 6,
                    immediate: 32768,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                },
                Instruction::AddImmediateShifted {
                    d: 0,
                    a: 0,
                    immediate: 17200,
                },
                Instruction::LoadFloatDouble {
                    d: 1,
                    a: 0,
                    offset: 0,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 16,
                },
                Instruction::LoadFloatDouble {
                    d: 0,
                    a: 1,
                    offset: 16,
                },
                Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate {
                    d: 3,
                    a: 27,
                    immediate: 0,
                },
                Instruction::BranchAndLink { .. },
            ]
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_partial_state_conversion_packet() {
        assert!(allocated_state_conversion(&[
            Instruction::XorImmediateShifted {
                a: 0,
                s: 6,
                immediate: 32768,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
        ])
        .is_none());
    }

    #[test]
    fn schedule_is_a_complete_permutation() {
        let mut schedule = SCHEDULE;
        schedule.sort_unstable();
        assert_eq!(schedule, std::array::from_fn(|index| index));
    }
}
