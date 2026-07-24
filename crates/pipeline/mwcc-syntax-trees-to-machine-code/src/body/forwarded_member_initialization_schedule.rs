//! Schedule a forwarded member initializer around its final float selection.
//!
//! Whole-file inlining can expose a straight-line aggregate initializer followed
//! by a conditional member value. Store-to-load forwarding leaves both select
//! arms in their incoming float registers. MWCC then hoists the shared integer
//! zero and the select comparison ahead of the independent stores, retaining
//! the zero for both the word and byte fields.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_forwarded_member_initialization(&mut self) {
        if !self.behavior.schedule_latency_slots {
            return;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(15)
            .enumerate()
            .find_map(|(start, window)| {
                is_forwarded_member_initialization(window, start).then_some(start)
            })
        else {
            return;
        };
        if self.output.relocations.iter().any(|relocation| {
            (start..start + 15).contains(&relocation.instruction_index)
        }) {
            return;
        }

        let zero = match self.output.instructions[start + 4] {
            Instruction::AddImmediate { d, .. } => d,
            _ => unreachable!(),
        };
        match &mut self.output.instructions[start + 7] {
            Instruction::StoreByte { s, .. } => *s = zero,
            _ => unreachable!(),
        }

        // Drop the second zero first. The remaining zero and compare are then
        // at stable indices 4 and 7 in the shortened stream.
        self.remove_structured_condition_instruction(start + 6);
        self.move_forwarded_initializer_instruction(start + 4, start);
        self.move_forwarded_initializer_instruction(start + 7, start + 1);
    }

    fn move_forwarded_initializer_instruction(&mut self, from: usize, to: usize) {
        debug_assert!(to < from);
        let instruction = self.output.instructions.remove(from);
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
        self.output.instructions.insert(to, instruction);
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

fn is_forwarded_member_initialization(window: &[Instruction], start: usize) -> bool {
    let [
        Instruction::StoreWord {
            s: kind,
            a: base,
            offset: kind_offset,
        },
        Instruction::StoreFloatSingle {
            s: maximum,
            a: maximum_base,
            offset: maximum_offset,
        },
        Instruction::StoreFloatSingle {
            s: minimum,
            a: minimum_base,
            offset: minimum_offset,
        },
        Instruction::StoreFloatSingle {
            a: delta_base,
            offset: delta_offset,
            ..
        },
        first_zero,
        Instruction::StoreWord {
            s: first_zero_store,
            a: state_base,
            offset: state_offset,
        },
        second_zero,
        Instruction::StoreByte {
            s: second_zero_store,
            a: alternate_base,
            offset: alternate_offset,
        },
        Instruction::CompareWordImmediate { a: compared, .. },
        Instruction::BranchConditionalForward {
            target: false_target,
            ..
        },
        Instruction::RoundToSingle {
            d: selected_true,
            b: selected_maximum,
        },
        Instruction::Branch { target: join_target },
        Instruction::RoundToSingle {
            d: selected_false,
            b: selected_minimum,
        },
        Instruction::StoreFloatSingle {
            s: selected_store,
            a: selected_base,
            offset: selected_offset,
        },
        Instruction::BranchToLinkRegister,
    ] = window
    else {
        return false;
    };
    let offsets = [
        *kind_offset,
        *maximum_offset,
        *minimum_offset,
        *delta_offset,
        *state_offset,
        *alternate_offset,
        *selected_offset,
    ];
    is_zero(first_zero, *first_zero_store)
        && is_zero(second_zero, *second_zero_store)
        && kind == compared
        && maximum == selected_maximum
        && minimum == selected_minimum
        && selected_true == selected_false
        && selected_true == selected_store
        && [
            maximum_base,
            minimum_base,
            delta_base,
            state_base,
            alternate_base,
            selected_base,
        ]
        .into_iter()
        .all(|candidate| candidate == base)
        && *false_target == start + 12
        && *join_target == start + 13
        && offsets
            .iter()
            .enumerate()
            .all(|(index, offset)| !offsets[..index].contains(offset))
}

fn is_zero(instruction: &Instruction, register: u8) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediate {
            d,
            a: 0,
            immediate: 0
        } if *d == register
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_forwarded_float_selection_after_member_stores() {
        let instructions = [
            Instruction::StoreWord {
                s: 4,
                a: 3,
                offset: 24,
            },
            Instruction::StoreFloatSingle {
                s: 1,
                a: 3,
                offset: 4,
            },
            Instruction::StoreFloatSingle {
                s: 2,
                a: 3,
                offset: 8,
            },
            Instruction::StoreFloatSingle {
                s: 3,
                a: 3,
                offset: 16,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 20,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreByte {
                s: 0,
                a: 3,
                offset: 28,
            },
            Instruction::CompareWordImmediate { a: 4, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::RoundToSingle { d: 0, b: 1 },
            Instruction::Branch { target: 13 },
            Instruction::RoundToSingle { d: 0, b: 2 },
            Instruction::StoreFloatSingle {
                s: 0,
                a: 3,
                offset: 12,
            },
            Instruction::BranchToLinkRegister,
        ];

        assert!(is_forwarded_member_initialization(&instructions, 0));
    }
}
