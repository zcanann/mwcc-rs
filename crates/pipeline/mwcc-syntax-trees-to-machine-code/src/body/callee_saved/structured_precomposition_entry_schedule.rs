//! Final entry shaping for a pre-composition guarded value graph.
//!
//! Allocation has already retained the four distinct caller homes. Build 163
//! fills the linkage latency slot with the incoming guard, saves that bank in
//! descending register order, folds the fixed error address, and represents
//! the adjacent category alternatives as one unsigned range.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
struct PrecompositionEntrySchedule {
    fixed_error_high: usize,
    category_range: usize,
    restore_start: usize,
}

impl Generator {
    pub(crate) fn schedule_structured_precomposition_entry(&mut self) {
        if self.inline_source_call_survivors.len() < 2 {
            return;
        }
        let Some(plan) =
            precomposition_entry_schedule(&self.output.instructions, &self.output.relocations)
        else {
            return;
        };

        for (index, register) in (3..7).zip([31, 30, 29, 28]) {
            let Instruction::StoreWord { s, .. } = &mut self.output.instructions[index] else {
                unreachable!("validated pre-composition save changed form")
            };
            *s = register;
        }
        for (index, register) in
            (plan.restore_start + 1..plan.restore_start + 5).zip([31, 30, 29, 28])
        {
            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[index] else {
                unreachable!("validated pre-composition restore changed form")
            };
            *d = register;
        }
        crate::move_instruction_before_retargeting(self, 7, 1);

        let high = plan.fixed_error_high;
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[high] else {
            unreachable!("validated fixed-address high half changed form")
        };
        *d = 3;
        let Instruction::LoadWord { d, a, offset } = &mut self.output.instructions[high + 2] else {
            unreachable!("validated fixed-address load changed form")
        };
        *d = 29;
        *a = 3;
        *offset = 0x6020;
        crate::remove_instruction_retargeting_to_next(self, high + 1);
        crate::move_instruction_before_retargeting(self, high + 3, high + 2);
        self.output.instructions[high + 2] = Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 0,
        };

        let range = plan.category_range - 1;
        let fallback = match self.output.instructions[range + 3] {
            Instruction::BranchConditionalForward { target, .. } => target,
            _ => unreachable!("validated category fallback changed form"),
        };
        self.output.instructions[range] = Instruction::AddImmediate {
            d: 0,
            a: 31,
            immediate: -2,
        };
        self.output.instructions[range + 1] =
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 };
        self.output.instructions[range + 2] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 1,
            target: fallback,
        };
        crate::remove_instruction_retargeting_to_next(self, range + 3);
    }
}

fn precomposition_entry_schedule(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<PrecompositionEntrySchedule> {
    if !matches!(
        instructions.get(0..9),
        Some([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                a: 1,
                offset: 28,
                ..
            },
            Instruction::StoreWord {
                a: 1,
                offset: 24,
                ..
            },
            Instruction::StoreWord {
                a: 1,
                offset: 20,
                ..
            },
            Instruction::StoreWord {
                a: 1,
                offset: 16,
                ..
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 16,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                ..
            },
        ])
    ) {
        return None;
    }

    let category_call = relocations.iter().find_map(|relocation| {
        (relocation.kind == RelocationKind::Rel24
            && matches!(
                &relocation.target,
                mwcc_machine_code::RelocationTarget::External(name)
                    if name == "CategorizeError"
            ))
        .then_some(relocation.instruction_index)
    })?;
    let fixed_error_high = category_call.checked_sub(5)?;
    if !matches!(
        instructions.get(fixed_error_high..category_call + 1),
        Some([
            Instruction::AddImmediateShifted {
                d: 29,
                a: 0,
                immediate: -13312,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 29,
                immediate: 24576,
            },
            Instruction::LoadWord {
                d: 29,
                a: 29,
                offset: 32,
            },
            _,
            Instruction::Or { a: 3, s: 29, b: 29 },
            _,
        ])
    ) {
        return None;
    }

    let category_range = instructions[category_call + 1..]
        .windows(5)
        .position(|window| {
            matches!(
                window,
                [
                    Instruction::CompareLogicalWordImmediate {
                        a: 31,
                        immediate: 2,
                    },
                    Instruction::BranchConditionalForward {
                        options: 12,
                        condition_bit: 2,
                        ..
                    },
                    Instruction::CompareLogicalWordImmediate {
                        a: 31,
                        immediate: 3,
                    },
                    Instruction::BranchConditionalForward {
                        options: 4,
                        condition_bit: 2,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 3,
                        a: 0,
                        immediate: 0,
                    },
                ]
            )
        })
        .map(|relative| category_call + 1 + relative)?;

    let restore_start = instructions.len().checked_sub(8)?;
    if !matches!(
        instructions.get(restore_start..),
        Some([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                a: 1,
                offset: 28,
                ..
            },
            Instruction::LoadWord {
                a: 1,
                offset: 24,
                ..
            },
            Instruction::LoadWord {
                a: 1,
                offset: 20,
                ..
            },
            Instruction::LoadWord {
                a: 1,
                offset: 16,
                ..
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ])
    ) {
        return None;
    }

    Some(PrecompositionEntrySchedule {
        fixed_error_high,
        category_range,
        restore_start,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn recognizes_the_four_home_dynamic_category_entry() {
        let mut instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 28,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 24,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: 28,
                a: 1,
                offset: 16,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 3,
                immediate: 16,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 29,
            },
            Instruction::AddImmediateShifted {
                d: 29,
                a: 0,
                immediate: -13312,
            },
            Instruction::AddImmediate {
                d: 29,
                a: 29,
                immediate: 24576,
            },
            Instruction::LoadWord {
                d: 29,
                a: 29,
                offset: 32,
            },
            Instruction::Or {
                a: 28,
                s: 29,
                b: 29,
            },
            Instruction::Or { a: 3, s: 29, b: 29 },
            Instruction::BranchAndLink {
                target: "CategorizeError".into(),
            },
            Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 2,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 19,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 31,
                immediate: 3,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 20,
            },
            Instruction::load_immediate(3, 0),
            Instruction::Or { a: 3, s: 3, b: 3 },
        ];
        instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: 29,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 24,
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 28,
                a: 1,
                offset: 16,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        let relocations = [Relocation {
            instruction_index: 14,
            kind: RelocationKind::Rel24,
            target: RelocationTarget::External("CategorizeError".into()),
        }];

        assert_eq!(
            precomposition_entry_schedule(&instructions, &relocations),
            Some(PrecompositionEntrySchedule {
                fixed_error_high: 9,
                category_range: 15,
                restore_start: 21,
            })
        );
    }
}
