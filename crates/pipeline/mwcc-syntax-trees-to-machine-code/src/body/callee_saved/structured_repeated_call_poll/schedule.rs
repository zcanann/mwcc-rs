use mwcc_machine_code::Instruction;

use crate::Generator;

use super::recognize::MINIMUM_POLL_PAIRS;

#[derive(Clone, Copy)]
enum EntryZeroTest {
    Separate,
    RecordedCopy,
}

impl Generator {
    /// Apply the final instruction-selection and frame schedule only after the
    /// source transaction and the complete emitted skeleton both agree.
    pub(crate) fn schedule_structured_repeated_call_poll_transaction(&mut self) {
        if !self.structured_repeated_call_poll_owner {
            return;
        }
        if self
            .data_section_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.anchor_symbol == "...data.0" && anchor.register.is_some())
        {
            self.schedule_anchored_repeated_call_poll_transaction();
            return;
        }
        let instructions = &self.output.instructions;
        let (frame_size, link_offset, saved_31_offset, saved_30_offset) =
            match instructions.get(..6) {
                Some(
                    [Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: frame_offset,
                    }, Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
                        s: 0,
                        a: 1,
                        offset: link_offset,
                    }, Instruction::StoreWord {
                        s: 31,
                        a: 1,
                        offset: saved_31_offset,
                    }, Instruction::Or { a: 31, s: 4, b: 4 }, Instruction::StoreWord {
                        s: 30,
                        a: 1,
                        offset: saved_30_offset,
                    }],
                ) if *frame_offset < 0 => (
                    -*frame_offset,
                    *link_offset,
                    *saved_31_offset,
                    *saved_30_offset,
                ),
                _ => return,
            };
        let entry_zero_test = if matches!(
            instructions.get(6..10),
            Some([
                Instruction::Or { a: 30, s: 3, b: 3 },
                Instruction::CompareLogicalWordImmediate {
                    a: 30,
                    immediate: 0
                },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadWord { d: 3, a: 30, .. }
            ])
        ) {
            EntryZeroTest::Separate
        } else if matches!(
            instructions.get(6..9),
            Some([
                Instruction::OrRecord { a: 30, s: 3, b: 3 },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadWord { d: 3, a: 30, .. }
            ])
        ) {
            EntryZeroTest::RecordedCopy
        } else {
            return;
        };
        let epilogue = instructions.len().checked_sub(6).filter(|&epilogue| {
            matches!(
                instructions.get(epilogue..),
                Some([
                    Instruction::LoadWord { d: 31, a: 1, .. },
                    Instruction::LoadWord { d: 0, a: 1, .. },
                    Instruction::LoadWord { d: 30, a: 1, .. },
                    Instruction::MoveToLinkRegister { s: 0 },
                    Instruction::AddImmediate { d: 1, a: 1, .. },
                    Instruction::BranchToLinkRegister
                ])
            )
        });
        if epilogue.is_none()
            && !matches!(
                instructions.get(instructions.len().saturating_sub(3)..),
                Some([
                    Instruction::BranchAndLink { .. },
                    Instruction::CompareLogicalWordImmediate {
                        a: 3,
                        immediate: 0
                    },
                    Instruction::BranchConditionalForward { target, .. }
                ]) if *target == instructions.len() - 3
            )
        {
            return;
        }
        let poll_comparisons = (1..instructions.len().saturating_sub(1))
            .filter(|&index| {
                matches!(instructions[index - 1], Instruction::BranchAndLink { .. })
                    && matches!(instructions[index], Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 })
                    && matches!(instructions[index + 1], Instruction::BranchConditionalForward { target, .. } if target == index - 1)
            })
            .count();
        if poll_comparisons < MINIMUM_POLL_PAIRS {
            return;
        }

        match entry_zero_test {
            EntryZeroTest::Separate => {
                crate::remove_instruction_retargeting_to_next(self, 7);
            }
            EntryZeroTest::RecordedCopy => {
                self.output.instructions[6] = Instruction::Or { a: 30, s: 3, b: 3 };
            }
        }
        crate::insert_instruction_retargeting(
            self,
            2,
            Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        );
        let Instruction::LoadWord { a, .. } = &mut self.output.instructions[9] else {
            unreachable!("the repeated call-poll prefix was validated above")
        };
        *a = 3;
        if epilogue.is_some() {
            let epilogue = self.output.instructions.len() - 6;
            self.output.instructions.swap(epilogue, epilogue + 1);
        } else {
            self.output.instructions.extend([
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: link_offset,
                },
                Instruction::LoadWord {
                    d: 31,
                    a: 1,
                    offset: saved_31_offset,
                },
                Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset: saved_30_offset,
                },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: frame_size,
                },
                Instruction::BranchToLinkRegister,
            ]);
        }
        make_zero_comparisons_signed(&mut self.output.instructions);
    }

    /// The protocol optimizer selects signed zero tests for call results and
    /// nullable callback words even when their source representation is
    /// unsigned. Exact skeleton schedules also use this common policy.
    pub(crate) fn normalize_structured_call_poll_zero_comparisons(&mut self) {
        if self.structured_repeated_call_poll_owner {
            make_zero_comparisons_signed(&mut self.output.instructions);
        }
    }

    fn schedule_anchored_repeated_call_poll_transaction(&mut self) {
        let instructions = &self.output.instructions;
        let Some(epilogue) = instructions.len().checked_sub(6) else {
            return;
        };
        let Some(narrow) = instructions.windows(3).position(|window| {
            matches!(
                window,
                [
                    Instruction::LoadHalfwordZero {
                        d: 0,
                        a: 30,
                        offset: 36
                    },
                    Instruction::AndContiguousMask {
                        a: 3,
                        s: 0,
                        begin: 16,
                        end: 31
                    },
                    Instruction::BranchAndLink { .. }
                ]
            )
        }) else {
            return;
        };
        let variadic_arguments = variadic_data_anchor_arguments(instructions);
        if !matches!(
            instructions.first(),
            Some(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32
            })
        ) || !matches!(
            instructions.get(1),
            Some(Instruction::MoveFromLinkRegister { d: 0 })
        ) || !matches!(
            instructions.get(2),
            Some(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36
            })
        ) || !matches!(
            instructions.get(3),
            Some(Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28
            })
        ) || !matches!(
            instructions.get(4),
            Some(Instruction::AddImmediateShifted {
                d: 5,
                a: 0,
                immediate: 0
            })
        ) || !matches!(
            instructions.get(5),
            Some(Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0
            })
        ) || !matches!(
            instructions.get(6),
            Some(Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 24
            })
        ) || !matches!(
            instructions.get(7),
            Some(Instruction::Or { a: 30, s: 3, b: 3 })
        ) || !matches!(
            instructions.get(epilogue),
            Some(Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 28
            })
        ) || !matches!(
            instructions.get(epilogue + 1),
            Some(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36
            })
        ) || !matches!(
            instructions.get(epilogue + 2),
            Some(Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 24
            })
        ) || variadic_arguments.len() != 6
            || self.output.data_section_displacements.len() != 6
        {
            return;
        }
        let logical_zero_comparisons = instructions
            .iter()
            .filter(|instruction| {
                matches!(
                    instruction,
                    Instruction::CompareLogicalWordImmediate { immediate: 0, .. }
                )
            })
            .count();
        if logical_zero_comparisons < MINIMUM_POLL_PAIRS + 1 {
            return;
        }

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[4] else {
            unreachable!("the data-anchor high half was validated above")
        };
        *d = 31;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[5] else {
            unreachable!("the data-anchor low half was validated above")
        };
        *a = 31;
        make_zero_comparisons_signed(&mut self.output.instructions);
        let Instruction::LoadHalfwordZero { d, .. } = &mut self.output.instructions[narrow] else {
            unreachable!("the narrow member load was validated above")
        };
        *d = 3;
        crate::remove_instruction_retargeting_to_next(self, narrow + 1);

        for start in variadic_data_anchor_arguments(&self.output.instructions) {
            crate::move_instruction_before_retargeting(self, start + 1, start);
        }
        let epilogue = self.output.instructions.len() - 6;
        self.output.instructions.swap(epilogue, epilogue + 1);
    }
}

fn make_zero_comparisons_signed(instructions: &mut [Instruction]) {
    for instruction in instructions {
        let Instruction::CompareLogicalWordImmediate { a, immediate: 0 } = *instruction else {
            continue;
        };
        *instruction = Instruction::CompareWordImmediate { a, immediate: 0 };
    }
}

fn variadic_data_anchor_arguments(instructions: &[Instruction]) -> Vec<usize> {
    instructions
        .windows(4)
        .enumerate()
        .filter_map(|(index, window)| {
            (matches!(window[0], Instruction::AddImmediate { d: 3, a: 31, .. })
                && writes_variadic_word(&window[1])
                && matches!(window[2], Instruction::ConditionRegisterClear { d: 6 })
                && matches!(window[3], Instruction::BranchAndLink { .. }))
            .then_some(index)
        })
        .collect()
}

fn writes_variadic_word(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::Or { a: 4, .. }
            | Instruction::LoadWord { d: 4, .. }
            | Instruction::LoadHalfwordZero { d: 4, .. }
    )
}

#[cfg(test)]
mod tests {
    use super::make_zero_comparisons_signed;
    use mwcc_machine_code::Instruction;

    #[test]
    fn converts_only_logical_zero_comparisons() {
        let mut instructions = [
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 7 },
        ];
        make_zero_comparisons_signed(&mut instructions);
        assert!(matches!(
            instructions[0],
            Instruction::CompareWordImmediate { a: 3, immediate: 0 }
        ));
        assert!(matches!(
            instructions[1],
            Instruction::CompareLogicalWordImmediate { a: 4, immediate: 7 }
        ));
    }
}
