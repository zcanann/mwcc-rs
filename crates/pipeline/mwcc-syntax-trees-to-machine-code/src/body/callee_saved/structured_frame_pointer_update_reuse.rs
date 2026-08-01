//! Keep an addressable pointer while loading its pointee for a cursor update.
//!
//! Selection can load the pointee back into the frame pointer's register and
//! then reload the pointer for `pointer += retained_size`. MWCC puts the
//! pointee in the otherwise-free reload destination, preserving the original
//! pointer and eliminating the frame reload.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn reuse_structured_frame_pointer_updates(&mut self) {
        while let Some(plan) = frame_pointer_update(&self.output.instructions) {
            let Instruction::LoadWord { d, .. } =
                &mut self.output.instructions[plan.pointee]
            else {
                unreachable!("the frame pointee load was matched")
            };
            *d = plan.value;
            let Instruction::Add { a, .. } = &mut self.output.instructions[plan.add] else {
                unreachable!("the frame pointer update was matched")
            };
            *a = plan.pointer;
            let Instruction::Or { s, b, .. } = &mut self.output.instructions[plan.copy] else {
                unreachable!("the frame pointee publication was matched")
            };
            *s = plan.value;
            *b = plan.value;
            crate::remove_instruction_retargeting_to_next(self, plan.reload);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FramePointerUpdate {
    pointee: usize,
    reload: usize,
    add: usize,
    copy: usize,
    pointer: u8,
    value: u8,
}

fn frame_pointer_update(instructions: &[Instruction]) -> Option<FramePointerUpdate> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord {
                d: pointer,
                a: 1,
                offset: frame_offset,
            },
            Instruction::LoadWord {
                d: overwritten_pointer,
                a: pointee_base,
                offset: 0,
            },
            Instruction::LoadWord {
                d: value,
                a: 1,
                offset: reload_offset,
            },
            Instruction::Add { a: add_base, .. },
            Instruction::StoreWord { .. },
            Instruction::Or {
                a: retained,
                s: published,
                b: duplicate,
            },
        ] = window
        else {
            return None;
        };
        (*pointer == *overwritten_pointer
            && *pointer == *pointee_base
            && *frame_offset == *reload_offset
            && *value == *add_base
            && *published == *pointer
            && *duplicate == *pointer
            && *retained != *pointer
            && *value != *pointer)
            .then_some(FramePointerUpdate {
                pointee: start + 1,
                reload: start + 2,
                add: start + 3,
                copy: start + 5,
                pointer: *pointer,
                value: *value,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_pointer_destroyed_before_its_frame_reload() {
        let instructions = vec![
            Instruction::LoadWord { d: 3, a: 1, offset: 16 },
            Instruction::LoadWord { d: 3, a: 3, offset: 0 },
            Instruction::LoadWord { d: 4, a: 1, offset: 16 },
            Instruction::Add { d: 0, a: 4, b: 28 },
            Instruction::StoreWord { s: 0, a: 1, offset: 16 },
            Instruction::Or { a: 28, s: 3, b: 3 },
        ];

        assert_eq!(
            frame_pointer_update(&instructions),
            Some(FramePointerUpdate {
                pointee: 1,
                reload: 2,
                add: 3,
                copy: 5,
                pointer: 3,
                value: 4,
            })
        );
    }
}
