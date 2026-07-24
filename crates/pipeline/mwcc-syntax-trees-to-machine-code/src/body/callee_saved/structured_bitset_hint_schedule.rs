//! Schedule a volatile ready-bit update followed by its wakeup hint.
//!
//! Intrusive queue insertion commonly ends by setting one priority bit and a
//! volatile reschedule flag. MWCC materializes the shared `1` before the final
//! queue stores, retains it for both writes, and fills the priority-load latency
//! slot with the independent bitset load. This pass runs before allocation so
//! those extended live ranges color naturally instead of pinning physical
//! registers after the fact.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::RelocationTarget;

impl Generator {
    pub(super) fn schedule_volatile_bitset_hint_tail(&mut self) {
        let Some(tail) = volatile_bitset_hint_tail(&self.output.instructions) else {
            return;
        };
        let bits_load = tail.start + 8;
        let bits_store = tail.start + 10;
        let hint_store = tail.start + 12;
        if !schedule_relocations::same_relocated_value(
            &self.output.relocations,
            &self.output.constants,
            bits_load,
            bits_store,
        ) {
            return;
        }
        let Some(bits_name) = self.relocated_external_name(bits_load) else {
            return;
        };
        let Some(hint_name) = self.relocated_external_name(hint_store) else {
            return;
        };
        if bits_name == hint_name
            || !self.volatile_globals.contains(hint_name)
            || self.output.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target }
                        if *target == tail.start + 11
                )
            })
        {
            return;
        }

        self.prefer_virtual_general(tail.one, 4);
        self.prefer_virtual_general(tail.queue, 5);
        self.prefer_virtual_general(tail.global, 3);

        match &mut self.output.instructions[hint_store] {
            Instruction::StoreWord { s, .. } => *s = tail.one,
            _ => unreachable!("shape checked"),
        }
        self.remove_structured_condition_instruction(tail.start + 11);

        // li 0; li 1; zero store; queue tail store; priority load; bitset load;
        // subfic; slw; or; bitset store; hint store.
        self.move_bitset_hint_instruction_before(tail.start + 6, tail.start + 1);
        self.move_bitset_hint_instruction_before(tail.start + 8, tail.start + 6);
    }

    fn relocated_external_name(&self, instruction_index: usize) -> Option<&str> {
        self.output
            .relocations
            .iter()
            .find(|relocation| relocation.instruction_index == instruction_index)
            .and_then(|relocation| match &relocation.target {
                RelocationTarget::External(name)
                | RelocationTarget::ExternalWithAddend(name, _) => Some(name.as_str()),
                _ => None,
            })
    }

    fn move_bitset_hint_instruction_before(&mut self, from: usize, to: usize) {
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
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => {
                    *target = if *target == from {
                        to
                    } else if (to..from).contains(&*target) {
                        *target + 1
                    } else {
                        *target
                    };
                }
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VolatileBitsetHintTail {
    start: usize,
    one: u8,
    queue: u8,
    global: u8,
}

fn volatile_bitset_hint_tail(instructions: &[Instruction]) -> Option<VolatileBitsetHintTail> {
    instructions
        .windows(13)
        .enumerate()
        .find_map(|(start, window)| match window {
            [
                Instruction::AddImmediate {
                    d: zero,
                    a: 0,
                    immediate: 0,
                },
                Instruction::StoreWord {
                    s: zero_store,
                    a: root,
                    ..
                },
                Instruction::LoadWord {
                    d: queue,
                    a: queue_root,
                    ..
                },
                Instruction::StoreWord {
                    s: queue_value,
                    a: queue_base,
                    ..
                },
                Instruction::LoadWord {
                    d: distance,
                    a: priority_root,
                    ..
                },
                Instruction::SubtractFromImmediate {
                    d: subtracted,
                    a: subtraction_source,
                    immediate: 31,
                },
                Instruction::AddImmediate {
                    d: one,
                    a: 0,
                    immediate: 1,
                },
                Instruction::ShiftLeftWord {
                    a: shifted,
                    s: shift_one,
                    b: shift_distance,
                },
                Instruction::LoadWord {
                    d: global,
                    a: 0,
                    offset: 0,
                },
                Instruction::Or {
                    a: combined,
                    s: or_global,
                    b: or_shifted,
                },
                Instruction::StoreWord {
                    s: bits,
                    a: 0,
                    offset: 0,
                },
                Instruction::AddImmediate {
                    d: hint_one,
                    a: 0,
                    immediate: 1,
                },
                Instruction::StoreWord {
                    s: hint_value,
                    a: 0,
                    offset: 0,
                },
            ] if zero == zero_store
                && root == queue_root
                && root == queue_value
                && root == priority_root
                && queue == queue_base
                && distance == subtracted
                && distance == subtraction_source
                && distance == shift_distance
                && one == shift_one
                && shifted == combined
                && shifted == or_shifted
                && combined == bits
                && global == or_global
                && hint_one == hint_value =>
            {
                Some(VolatileBitsetHintTail {
                    start,
                    one: *one,
                    queue: *queue,
                    global: *global,
                })
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_unscheduled_volatile_bitset_hint_tail() {
        let instructions = vec![
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 4,
            },
            Instruction::LoadWord {
                d: 130,
                a: 3,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 3,
                a: 130,
                offset: 4,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 12,
            },
            Instruction::SubtractFromImmediate {
                d: 0,
                a: 0,
                immediate: 31,
            },
            Instruction::AddImmediate {
                d: 131,
                a: 0,
                immediate: 1,
            },
            Instruction::ShiftLeftWord {
                a: 0,
                s: 131,
                b: 0,
            },
            Instruction::LoadWord {
                d: 132,
                a: 0,
                offset: 0,
            },
            Instruction::Or {
                a: 0,
                s: 132,
                b: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 1,
            },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
        ];

        assert_eq!(
            volatile_bitset_hint_tail(&instructions),
            Some(VolatileBitsetHintTail {
                start: 0,
                one: 131,
                queue: 130,
                global: 132,
            })
        );
    }
}
