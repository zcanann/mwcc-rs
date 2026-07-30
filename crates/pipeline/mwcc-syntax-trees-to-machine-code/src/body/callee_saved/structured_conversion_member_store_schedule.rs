//! Member stores scheduled through a floating conversion latency window.
//!
//! A converted result and an independent retained value are written to the
//! same freshly loaded object. Build 163 loads that object and publishes the
//! retained value before spilling the converted result image.

#[allow(unused_imports)]
use super::*;

use super::structured_conversion_call_schedule::permute_region;

const SCHEDULE: [usize; 6] = [0, 3, 4, 1, 2, 5];

impl Generator {
    pub(crate) fn schedule_structured_conversion_member_stores(&mut self) -> bool {
        let mut changed = false;
        while let Some(start) = self
            .output
            .instructions
            .windows(SCHEDULE.len())
            .position(conversion_member_stores)
        {
            assign_build_163_registers(
                &mut self.output.instructions[start..start + SCHEDULE.len()],
            );
            permute_region(&mut self.output, start, &SCHEDULE);
            changed = true;
        }
        changed
    }
}

fn conversion_member_stores(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::ConvertToIntegerWordZero { d: converted, .. },
            Instruction::StoreFloatDouble {
                s: stored,
                a: 1,
                offset: image_offset,
            },
            Instruction::LoadWord {
                d: converted_word,
                a: 1,
                offset: word_offset,
            },
            Instruction::LoadWord {
                d: object,
                a: owner,
                ..
            },
            Instruction::StoreWord {
                s: retained,
                a: first_object,
                ..
            },
            Instruction::StoreWord {
                s: stored_word,
                a: second_object,
                ..
            },
        ] if *converted == *stored
            && *converted_word == *stored_word
            && *object == *first_object
            && *object == *second_object
            && *owner >= 14
            && *retained >= 14
            && image_offset.checked_add(4) == Some(*word_offset)
    )
}

fn assign_build_163_registers(window: &mut [Instruction]) {
    match &mut window[2] {
        Instruction::LoadWord { d, .. } => *d = 0,
        _ => unreachable!("converted word load changed after recognition"),
    }
    match &mut window[3] {
        Instruction::LoadWord { d, .. } => *d = 3,
        _ => unreachable!("object load changed after recognition"),
    }
    match &mut window[4] {
        Instruction::StoreWord { a, .. } => *a = 3,
        _ => unreachable!("retained member store changed after recognition"),
    }
    match &mut window[5] {
        Instruction::StoreWord { s, a, .. } => {
            *s = 0;
            *a = 3;
        }
        _ => unreachable!("converted member store changed after recognition"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Vec<Instruction> {
        vec![
            Instruction::ConvertToIntegerWordZero { d: 0, b: 1 },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 56,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 60,
            },
            Instruction::LoadWord {
                d: 4,
                a: 29,
                offset: 44,
            },
            Instruction::StoreWord {
                s: 30,
                a: 4,
                offset: 6524,
            },
            Instruction::StoreWord {
                s: 3,
                a: 4,
                offset: 8212,
            },
        ]
    }

    #[test]
    fn recognizes_independent_member_stores_after_a_conversion() {
        assert!(conversion_member_stores(&window()));
    }

    #[test]
    fn assigns_the_legacy_object_and_conversion_registers() {
        let mut instructions = window();

        assign_build_163_registers(&mut instructions);

        assert!(matches!(
            instructions[2],
            Instruction::LoadWord { d: 0, .. }
        ));
        assert!(matches!(
            instructions[3],
            Instruction::LoadWord { d: 3, .. }
        ));
        assert!(matches!(
            instructions[4],
            Instruction::StoreWord { a: 3, .. }
        ));
        assert!(matches!(
            instructions[5],
            Instruction::StoreWord { s: 0, a: 3, .. }
        ));
    }
}
