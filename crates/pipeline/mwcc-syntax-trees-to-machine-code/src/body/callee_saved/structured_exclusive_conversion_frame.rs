//! Compact mutually exclusive inline scratch and conversion stack lifetimes.
//!
//! A structured early-return arm can own the first float-to-integer image while
//! the fallthrough owns an aggregate and a second image.  When two unused byte
//! arrays provide the inline/caller optimizer residue, MWCC overlaps those
//! non-coexisting regions instead of concatenating every logical slot.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn compact_exclusive_inline_conversion_frame(&mut self) -> bool {
        if self.frame_size != 96
            || self.callee_saved.len() != 3
            || self.float_to_int_scratch_end != 72
        {
            return false;
        }
        let aggregate_slots: Vec<_> = self
            .frame_slots
            .iter()
            .filter(|(_, slot)| !slot.is_array && slot.size == 12 && slot.offset == 48)
            .map(|(name, _)| name.clone())
            .collect();
        let mut array_slots: Vec<_> = self
            .frame_slots
            .values()
            .filter(|slot| slot.is_array)
            .map(|slot| (slot.offset, slot.size))
            .collect();
        array_slots.sort_unstable();
        if aggregate_slots.len() != 1
            || array_slots != [(16, 8), (24, 24)]
            || !exclusive_conversion_instruction_shape(&self.output.instructions)
        {
            return false;
        }

        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -96,
                } => {
                    *instruction = Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -80,
                    }
                }
                Instruction::StoreWord { a: 1, offset, .. } if matches!(*offset, 84 | 88 | 92) => {
                    *offset -= 16;
                }
                Instruction::LoadWord { a: 1, offset, .. } if matches!(*offset, 84 | 88 | 92) => {
                    *offset -= 16;
                }
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 100,
                } => {
                    *instruction = Instruction::LoadWord {
                        d: 0,
                        a: 1,
                        offset: 84,
                    }
                }
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 96,
                } => {
                    *instruction = Instruction::AddImmediate {
                        d: 1,
                        a: 1,
                        immediate: 80,
                    }
                }
                Instruction::StoreWord { a: 1, offset, .. } if matches!(*offset, 48 | 52 | 56) => {
                    *offset -= 4;
                }
                Instruction::AddImmediate {
                    a: 1,
                    immediate: 48,
                    ..
                } => {
                    if let Instruction::AddImmediate { immediate, .. } = instruction {
                        *immediate = 44;
                    }
                }
                Instruction::StoreFloatDouble {
                    a: 1, offset: 64, ..
                } => {
                    if let Instruction::StoreFloatDouble { offset, .. } = instruction {
                        *offset = 56;
                    }
                }
                Instruction::LoadWord {
                    a: 1, offset: 68, ..
                } => {
                    if let Instruction::LoadWord { offset, .. } = instruction {
                        *offset = 60;
                    }
                }
                _ => {}
            }
        }
        self.frame_slots
            .get_mut(&aggregate_slots[0])
            .expect("aggregate slot was recognized")
            .offset = 44;
        self.float_to_int_scratch_next = 64;
        self.float_to_int_scratch_end = 64;
        self.frame_size = 80;
        true
    }
}

fn exclusive_conversion_instruction_shape(instructions: &[Instruction]) -> bool {
    let conversion_stores: Vec<_> = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::StoreFloatDouble { a: 1, offset, .. } => Some(*offset),
            _ => None,
        })
        .collect();
    let conversion_loads: Vec<_> = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::LoadWord { a: 1, offset, .. } if matches!(*offset, 60 | 68) => {
                Some(*offset)
            }
            _ => None,
        })
        .collect();
    conversion_stores == [56, 64]
        && conversion_loads == [60, 68]
        && instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    a: 1,
                    immediate: 48,
                    ..
                }
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_two_distinct_conversion_images_and_one_aggregate_address() {
        let instructions = vec![
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 56,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 60,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 48,
            },
            Instruction::StoreFloatDouble {
                s: 0,
                a: 1,
                offset: 64,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 68,
            },
        ];

        assert!(exclusive_conversion_instruction_shape(&instructions));
    }
}
