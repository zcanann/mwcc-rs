//! Instruction-count normalization for a guarded TLUT display-list packet.
//!
//! The 48-byte packet contains three zero words and one packed count field.
//! MWCC retains one zero register across the packet and combines the unsigned
//! narrowing plus left shift into one rotate-mask instruction.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_structured_tlut_packet(&mut self) {
        if let Some(zeros) = tlut_zero_stores(&self.output.instructions) {
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[zeros.first_load]
            else {
                unreachable!("the first TLUT zero was matched")
            };
            *d = 7;
            for store in [zeros.first_store, zeros.second_store, zeros.third_store] {
                let Instruction::StoreWord { s, .. } = &mut self.output.instructions[store] else {
                    unreachable!("the TLUT zero store was matched")
                };
                *s = 7;
            }
            self.remove_tlut_instruction(zeros.third_load);
            self.remove_tlut_instruction(zeros.second_load);
        }

        let Some(mask) = tlut_count_mask(&self.output.instructions) else {
            return;
        };
        let Instruction::AddImmediate { d, .. } = &mut self.output.instructions[mask.subtract]
        else {
            unreachable!("the count decrement was matched")
        };
        *d = Eabi::FIRST_GENERAL_ARGUMENT;
        self.output.instructions[mask.clear] = Instruction::RotateAndMask {
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            s: Eabi::FIRST_GENERAL_ARGUMENT,
            shift: 14,
            begin: 8,
            end: 17,
        };
        let Instruction::OrImmediateShifted { a, s, .. } =
            &mut self.output.instructions[mask.combine]
        else {
            unreachable!("the count command merge was matched")
        };
        *a = Eabi::FIRST_GENERAL_ARGUMENT;
        *s = Eabi::FIRST_GENERAL_ARGUMENT;
        let Instruction::StoreWord { s, .. } = &mut self.output.instructions[mask.store] else {
            unreachable!("the packed count store was matched")
        };
        *s = Eabi::FIRST_GENERAL_ARGUMENT;
        self.remove_tlut_instruction(mask.shift);
    }

    fn remove_tlut_instruction(&mut self, index: usize) {
        let old_len = self.output.instructions.len();
        self.output.instructions.remove(index);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != index);
        let permutation: Vec<usize> = (0..old_len)
            .map(|old| {
                if old < index {
                    old
                } else if old == index {
                    index.saturating_sub(1)
                } else {
                    old - 1
                }
            })
            .collect();
        crate::remap_instruction_indices(self, &permutation);
    }
}

#[derive(Clone, Copy)]
struct TlutZeroStores {
    first_load: usize,
    first_store: usize,
    second_load: usize,
    second_store: usize,
    third_load: usize,
    third_store: usize,
}

fn tlut_zero_stores(instructions: &[Instruction]) -> Option<TlutZeroStores> {
    let (first_load, base) = instructions
        .windows(2)
        .enumerate()
        .find_map(|(index, window)| zero_word_store(window, 12).map(|base| (index, base)))?;
    let second_load = (first_load + 2..instructions.len().saturating_sub(1))
        .find(|&index| zero_word_store(&instructions[index..index + 2], 28) == Some(base))?;
    let third_load = (second_load + 2..instructions.len().saturating_sub(1))
        .find(|&index| zero_word_store(&instructions[index..index + 2], 44) == Some(base))?;
    if instructions[first_load + 2..third_load]
        .iter()
        .any(is_tlut_barrier)
    {
        return None;
    }
    Some(TlutZeroStores {
        first_load,
        first_store: first_load + 1,
        second_load,
        second_store: second_load + 1,
        third_load,
        third_store: third_load + 1,
    })
}

fn zero_word_store(window: &[Instruction], offset: i16) -> Option<u8> {
    match window {
        [Instruction::AddImmediate {
            d: zero,
            a: 0,
            immediate: 0,
        }, Instruction::StoreWord {
            s,
            a,
            offset: store_offset,
        }, ..]
            if zero == s && *store_offset == offset =>
        {
            Some(*a)
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct TlutCountMask {
    subtract: usize,
    clear: usize,
    shift: usize,
    combine: usize,
    store: usize,
}

fn tlut_count_mask(instructions: &[Instruction]) -> Option<TlutCountMask> {
    instructions
        .windows(5)
        .enumerate()
        .find_map(|(start, window)| {
            matches!(
                window,
                [
                    Instruction::AddImmediate {
                        d: decremented,
                        immediate: -1,
                        ..
                    },
                Instruction::AndContiguousMask {
                    a: narrowed,
                    s: decrement_source,
                    begin: 22,
                    end: 31,
                    },
                    Instruction::ShiftLeftImmediate {
                        a: shifted,
                        s: narrow_source,
                        shift: 14,
                    },
                    Instruction::OrImmediateShifted {
                        a: combined,
                        s: shift_source,
                        immediate: 1792,
                    },
                    Instruction::StoreWord {
                        s: stored,
                        offset: 36,
                        ..
                    },
                ] if decremented == decrement_source
                    && narrowed == narrow_source
                    && shifted == shift_source
                    && combined == stored
            )
            .then_some(TlutCountMask {
                subtract: start,
                clear: start + 1,
                shift: start + 2,
                combine: start + 3,
                store: start + 4,
            })
        })
}

fn is_tlut_barrier(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::BranchConditionalForward { .. }
            | Instruction::Branch { .. }
            | Instruction::BranchAndLink { .. }
            | Instruction::BranchExternal { .. }
            | Instruction::BranchToLinkRegister
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_three_zero_words_in_one_tlut_packet() {
        let instructions = vec![
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 12,
            },
            Instruction::load_immediate_shifted(0, -2816),
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 28,
            },
            Instruction::load_immediate_shifted(0, -4096),
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 44,
            },
        ];
        let zeros = tlut_zero_stores(&instructions).expect("the zero stores should match");
        assert_eq!(
            (
                zeros.first_load,
                zeros.second_load,
                zeros.third_load,
                zeros.third_store,
            ),
            (0, 3, 6, 7)
        );
    }

    #[test]
    fn recognizes_the_narrowed_shifted_tlut_count() {
        let instructions = vec![
            Instruction::AddImmediate {
                d: 0,
                a: 47,
                immediate: -1,
            },
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 22,
                end: 31,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 14,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 1792,
            },
            Instruction::StoreWord {
                s: 0,
                a: 37,
                offset: 36,
            },
        ];
        let mask = tlut_count_mask(&instructions).expect("the count mask should match");
        assert_eq!(
            (
                mask.subtract,
                mask.clear,
                mask.shift,
                mask.combine,
                mask.store
            ),
            (0, 1, 2, 3, 4)
        );
    }
}
