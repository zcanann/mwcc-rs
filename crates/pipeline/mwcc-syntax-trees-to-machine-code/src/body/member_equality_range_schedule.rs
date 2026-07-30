//! Coalesce adjacent member-equality branches into an unsigned range test.
//!
//! Build 163 turns `x == A || x == B || x == B + 1 || x == C` into a
//! singleton, one unsigned two-value range, and a final singleton. When the
//! compared member still uses incoming `r3` as its base, preserve that base in
//! `r4` before reusing `r3` for the loaded value.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn coalesce_member_equality_branch_runs(&mut self) {
        while let Some(mut plan) = member_equality_branch_run(&self.output.instructions) {
            if plan.member_base == 3 {
                let Some(link_read) =
                    self.output.instructions[..plan.start]
                        .iter()
                        .rposition(|instruction| {
                            matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 })
                        })
                else {
                    return;
                };
                if !matches!(
                    self.output.instructions.get(plan.body..plan.body + 2),
                    Some([
                        Instruction::LoadWord { d: 4, a: 3, .. },
                        Instruction::LoadWord { d: 3, a: 3, .. },
                    ])
                ) {
                    return;
                }
                crate::insert_instruction_retargeting(
                    self,
                    link_read + 1,
                    Instruction::AddImmediate {
                        d: 4,
                        a: 3,
                        immediate: 0,
                    },
                );
                plan = member_equality_branch_run(&self.output.instructions)
                    .expect("inserting the independent base copy preserves the equality run");
                let Instruction::LoadWord { a, .. } = &mut self.output.instructions[plan.start]
                else {
                    unreachable!("the member load was matched")
                };
                *a = 4;
                for index in plan.body..plan.body + 2 {
                    let Instruction::LoadWord { a, .. } = &mut self.output.instructions[index]
                    else {
                        unreachable!("the outgoing member loads were matched")
                    };
                    *a = 4;
                }
                // These are relocation-free member loads in one basic block.
                // The branch enters the block at its first index, so exchange
                // their contents rather than preserving the old first load's
                // instruction identity.
                self.output.instructions.swap(plan.body, plan.body + 1);
                plan = member_equality_branch_run(&self.output.instructions)
                    .expect("reordering the outgoing loads preserves the equality run");
            }

            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.start] else {
                unreachable!("the equality-run member load was matched")
            };
            *d = 3;
            for index in [plan.start + 1, plan.start + 7] {
                let Instruction::CompareLogicalWordImmediate { a, .. } =
                    &mut self.output.instructions[index]
                else {
                    unreachable!("the singleton equality was matched")
                };
                *a = 3;
            }
            self.output.instructions[plan.start + 3] = Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -i16::try_from(plan.range_minimum)
                    .expect("the matched range minimum fits addi"),
            };
            self.output.instructions[plan.start + 5] = Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: plan.range_span,
            };
            let Instruction::BranchConditionalForward {
                options,
                condition_bit,
                ..
            } = &mut self.output.instructions[plan.start + 6]
            else {
                unreachable!("the second range equality branch was matched")
            };
            *options = 4;
            *condition_bit = 1;
            crate::remove_instruction_retargeting_to_next(self, plan.start + 4);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MemberEqualityBranchRun {
    start: usize,
    body: usize,
    member_base: u8,
    range_minimum: u16,
    range_span: u16,
}

fn member_equality_branch_run(instructions: &[Instruction]) -> Option<MemberEqualityBranchRun> {
    instructions
        .windows(9)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::LoadWord {
                d: loaded,
                a: member_base,
                ..
            }, Instruction::CompareLogicalWordImmediate {
                a: first_source,
                immediate: first,
            }, Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: first_body,
            }, Instruction::CompareLogicalWordImmediate {
                a: second_source,
                immediate: second,
            }, Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: second_body,
            }, Instruction::CompareLogicalWordImmediate {
                a: third_source,
                immediate: third,
            }, Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: third_body,
            }, Instruction::CompareLogicalWordImmediate {
                a: final_source,
                immediate: final_value,
            }, Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: skip,
            }] = window
            else {
                return None;
            };
            if loaded != first_source
                || loaded != second_source
                || loaded != third_source
                || loaded != final_source
                || first_body != second_body
                || first_body != third_body
                || *second == *first
                || second.checked_add(1) != Some(*third)
                || *final_value == *third
                || *first_body <= start + 8
                || *skip <= *first_body
                || !matches!(
                    instructions.get(*skip),
                    Some(
                        Instruction::BranchAndLink { .. }
                            | Instruction::LoadWord { d: 0, a: 1, .. }
                    )
                )
            {
                return None;
            }
            let body = *first_body;
            if !matches!(
                instructions.get(body..body + 2),
                Some([
                    Instruction::LoadWord { d: first_result, .. },
                    Instruction::LoadWord { d: second_result, .. },
                ]) if (*first_result == 3 && *second_result == 4)
                    || (*first_result == 4 && *second_result == 3)
            ) {
                return None;
            }
            Some(MemberEqualityBranchRun {
                start,
                body,
                member_base: *member_base,
                range_minimum: *second,
                range_span: third - second,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_singleton_range_singleton_member_branches() {
        let mut instructions = vec![
            Instruction::LoadWord {
                d: 0,
                a: 31,
                offset: 8,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 1 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 9,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 4 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 9,
            },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 5 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 9,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 14,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 12,
            },
            Instruction::LoadWord {
                d: 3,
                a: 31,
                offset: 24,
            },
            Instruction::LoadWord {
                d: 4,
                a: 31,
                offset: 20,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
            Instruction::BranchAndLink {
                target: "continue".into(),
            },
        ];

        let plan = member_equality_branch_run(&instructions).expect("measured equality run");
        assert_eq!(plan.start, 0);
        assert_eq!(plan.body, 9);
        assert_eq!(plan.range_minimum, 4);
        assert_eq!(plan.range_span, 1);
    }
}
