//! Final entry schedule for repeated inlined indirect-member walks.
//!
//! The whole-body planner assigns the three incoming values non-monotonic
//! saved homes. Selection emits those home copies in allocation order, while
//! Build 163 issues them in ABI source order and spells each as `mr`. Physical
//! homes are not known soon enough to make that decision during selection.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_repeated_indirect_member_loop_entry(&mut self) {
        schedule_entry_parameter_copies(
            &mut self.output.instructions,
            self.structured_repeated_indirect_member_loop_entry,
        );
    }
}

fn schedule_entry_parameter_copies(instructions: &mut [Instruction], enabled: bool) {
    if !enabled {
        return;
    }
    let Some(start) = instructions.windows(4).position(is_dense_entry_packet) else {
        return;
    };
    let copies = &mut instructions[start + 1..start + 4];
    copies.sort_by_key(|instruction| {
        entry_parameter_copy(instruction)
            .map(|(_, incoming)| incoming)
            .unwrap_or(u8::MAX)
    });
    for instruction in copies {
        let (destination, incoming) =
            entry_parameter_copy(instruction).expect("dense entry packet was recognized");
        *instruction = Instruction::move_register(destination, incoming);
    }
}

fn is_dense_entry_packet(window: &[Instruction]) -> bool {
    let [Instruction::StoreMultipleWord { s: 26, a: 1, .. }, first, second, third] = window
    else {
        return false;
    };
    let mut sources = [first, second, third]
        .map(entry_parameter_copy)
        .map(|copy| copy.map(|(_, source)| source));
    sources.sort();
    sources == [Some(3), Some(4), Some(5)]
}

fn entry_parameter_copy(instruction: &Instruction) -> Option<(u8, u8)> {
    let Instruction::AddImmediate {
        d,
        a,
        immediate: 0,
    } = instruction
    else {
        return None;
    };
    (*d >= 14 && (3..=5).contains(a)).then_some((*d, *a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_source_order_and_move_spelling_after_home_allocation() {
        let mut instructions = vec![
            Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 26,
                a: 3,
                immediate: 0,
            },
        ];

        schedule_entry_parameter_copies(&mut instructions, true);

        assert_eq!(
            &instructions[1..],
            &[
                Instruction::move_register(26, 3),
                Instruction::move_register(30, 4),
                Instruction::move_register(31, 5),
            ]
        );
    }

    #[test]
    fn leaves_an_unowned_packet_untouched() {
        let mut instructions = vec![
            Instruction::StoreMultipleWord {
                s: 26,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 4,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 26,
                a: 3,
                immediate: 0,
            },
        ];
        let original = instructions.clone();

        schedule_entry_parameter_copies(&mut instructions, false);

        assert_eq!(instructions, original);
    }
}
