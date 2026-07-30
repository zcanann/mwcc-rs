//! Fixed-count aggregate-copy scheduling for object-state transfers.
//!
//! A scalar member and several independent vector members precede the input
//! aggregate copy. MWCC starts the count-register transaction immediately
//! after loading that scalar: the independent scalar store and member copies
//! cover the setup latency before the first updating loop load.

use super::structured_state_transfer_layout::is_unused_array_state_transfer;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn finalize_structured_state_transfer_copy_schedule(&mut self, function: &Function) {
        if !is_unused_array_state_transfer(function) {
            return;
        }
        let Some(start) = allocated_state_transfer_copy_packet(&self.output.instructions) else {
            return;
        };

        let original = self.output.instructions[start..start + 25].to_vec();
        let mut scalar_load = original[0].clone();
        let mut scalar_store = original[1].clone();
        let Instruction::LoadWord { d, .. } = &mut scalar_load else {
            unreachable!("the scalar source load was matched")
        };
        *d = 3;
        let Instruction::StoreWord { s, .. } = &mut scalar_store else {
            unreachable!("the scalar destination store was matched")
        };
        *s = 3;

        let mut scheduled = Vec::with_capacity(25);
        scheduled.push(scalar_load);
        scheduled.extend_from_slice(&original[20..23]);
        scheduled.push(scalar_store);
        scheduled.push(original[23].clone());
        scheduled.extend_from_slice(&original[2..20]);
        scheduled.push(original[24].clone());
        self.output.instructions[start..start + 25].clone_from_slice(&scheduled);
    }
}

fn allocated_state_transfer_copy_packet(instructions: &[Instruction]) -> Option<usize> {
    instructions
        .windows(25)
        .enumerate()
        .find_map(|(start, window)| {
            let member_copies = same_word_member_copy(&window[0], &window[1], 0)
                && same_word_member_copy(&window[2], &window[4], 3)
                && same_word_member_copy(&window[3], &window[5], 0)
                && same_word_member_copy(&window[6], &window[7], 0)
                && same_word_member_copy(&window[8], &window[9], 0)
                && same_float_member_copy(&window[10], &window[11])
                && same_float_member_copy(&window[12], &window[13])
                && same_float_member_copy(&window[14], &window[15])
                && same_float_member_copy(&window[16], &window[17])
                && same_float_member_copy(&window[18], &window[19]);
            let loop_setup = matches!(
                &window[20..25],
                [
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 10,
                    },
                    Instruction::MoveToCountRegister { s: 0 },
                    Instruction::AddImmediate {
                        d: 5,
                        a: 29,
                        immediate: 1560,
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        a: 31,
                        immediate: 1560,
                    },
                    Instruction::LoadWordWithUpdate {
                        d: 3,
                        a: 4,
                        offset: 8,
                    },
                ]
            );
            (member_copies && loop_setup).then_some(start)
        })
}

fn same_word_member_copy(load: &Instruction, store: &Instruction, register: u8) -> bool {
    matches!(
        (load, store),
        (
            Instruction::LoadWord {
                d,
                a: 31,
                offset: source_offset,
            },
            Instruction::StoreWord {
                s,
                a: 29,
                offset: destination_offset,
            },
        ) if *d == register && *s == register && source_offset == destination_offset
    )
}

fn same_float_member_copy(load: &Instruction, store: &Instruction) -> bool {
    matches!(
        (load, store),
        (
            Instruction::LoadFloatSingle {
                d: 0,
                a: 31,
                offset: source_offset,
            },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 29,
                offset: destination_offset,
            },
        ) if source_offset == destination_offset
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_copy_requires_the_same_source_and_destination_offset() {
        assert!(!same_word_member_copy(
            &Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 16,
            },
            &Instruction::StoreWord {
                s: 0,
                a: 29,
                offset: 20,
            },
            0,
        ));
    }
}
