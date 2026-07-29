//! Physical register schedule for indexed global callback-table loads.
//!
//! After a preceding call has killed volatile GPRs, MWCC loads the index into
//! `r4`, completes the relocated table address in `r3`/`r0`, then reuses `r3`
//! for both the scaled index and the final slot address. The callback itself
//! remains in the ABI indirect-call register `r12`.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_structured_indexed_callback_lookup(&mut self) {
        let Some(start) = indexed_callback_lookup(&self.output.instructions) else {
            return;
        };
        if !is_relocated_callback_table(&self.output, start) {
            return;
        }
        self.move_instruction_before(start + 3, start + 2);
        self.output.instructions[start] = match self.output.instructions[start] {
            Instruction::LoadWord { a, offset, .. } => {
                Instruction::LoadWord { d: 4, a, offset }
            }
            _ => unreachable!("indexed callback lookup was recognized"),
        };
        self.output.instructions[start + 1] = Instruction::AddImmediateShifted {
            d: 3,
            a: 0,
            immediate: 0,
        };
        self.output.instructions[start + 2] = Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 0,
        };
        self.output.instructions[start + 3] = Instruction::ShiftLeftImmediate {
            a: 3,
            s: 4,
            shift: 2,
        };
        self.output.instructions[start + 4] = Instruction::Add {
            d: 3,
            a: 0,
            b: 3,
        };
        self.output.instructions[start + 5] = Instruction::LoadWord {
            d: 12,
            a: 3,
            offset: 0,
        };
    }
}

fn indexed_callback_lookup(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        let [
            Instruction::LoadWord { d: index, .. },
            Instruction::AddImmediateShifted {
                d: high,
                a: 0,
                immediate: 0,
            },
            Instruction::ShiftLeftImmediate {
                a: scaled,
                s: scale_source,
                shift: 2,
            },
            Instruction::AddImmediate {
                d: low,
                a: low_base,
                immediate: 0,
            },
            Instruction::Add {
                d: address,
                a: add_left,
                b: add_right,
            },
            Instruction::LoadWord {
                d: 12,
                a: callback_base,
                offset: 0,
            },
        ] = window
        else {
            return None;
        };
        let add_uses_low_and_scale =
            (*add_left == *low && *add_right == *scaled)
                || (*add_left == *scaled && *add_right == *low);
        let preceded_by_call = instructions[start.saturating_sub(8)..start]
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }));
        if preceded_by_call
            && index == scale_source
            && high == low_base
            && address == callback_base
            && add_uses_low_and_scale
            && has_callback_guard_and_call(instructions, start + 6)
        {
            Some(start)
        } else {
            None
        }
    })
}

fn is_relocated_callback_table(
    output: &mwcc_machine_code::MachineFunction,
    start: usize,
) -> bool {
    let high = output.relocations.iter().find(|relocation| {
        relocation.instruction_index == start + 1
            && relocation.kind == RelocationKind::Addr16Ha
    });
    let low = output.relocations.iter().find(|relocation| {
        relocation.instruction_index == start + 3
            && relocation.kind == RelocationKind::Addr16Lo
    });
    high.zip(low).is_some_and(|_| {
        super::super::schedule_relocations::same_target_value(
            &output.relocations,
            &output.constants,
            start + 1,
            start + 3,
        )
    })
}

fn has_callback_guard_and_call(instructions: &[Instruction], start: usize) -> bool {
    let end = (start + 9).min(instructions.len());
    let tail = &instructions[start..end];
    tail.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0
            }
        )
    }) && tail
        .iter()
        .any(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { s: 12 }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_indexed_callback_slot_load() {
        let instructions = [
            Instruction::BranchAndLink {
                target: "prepare".into(),
            },
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 4,
            },
            Instruction::AddImmediateShifted {
                d: 12,
                a: 0,
                immediate: 0,
            },
            Instruction::ShiftLeftImmediate {
                a: 3,
                s: 3,
                shift: 2,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 12,
                immediate: 0,
            },
            Instruction::Add {
                d: 12,
                a: 0,
                b: 3,
            },
            Instruction::LoadWord {
                d: 12,
                a: 12,
                offset: 0,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 12,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 10,
            },
            Instruction::MoveToLinkRegister { s: 12 },
        ];

        assert_eq!(indexed_callback_lookup(&instructions), Some(1));
    }
}
