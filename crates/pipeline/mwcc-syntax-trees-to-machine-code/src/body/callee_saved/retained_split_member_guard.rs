//! Scheduling for a retained result with repeated reads from a split-tag input.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Preserve the fourth incoming argument across a short-circuit member
    /// guard and fill the callee-saved prologue's ready load slot.
    ///
    /// The physical allocator can otherwise coalesce the temporary pointer
    /// loaded from `r6+16` back into r6 even though a later guard still reads
    /// `r6+28`. MWCC keeps that temporary in r3. The same measured region also
    /// schedules its first independent member load before the r31 save and
    /// splits a later wide literal around a ready `li`.
    pub(crate) fn schedule_retained_split_member_guard(&mut self) {
        schedule_retained_split_member_guard(&mut self.output);
    }
}

fn schedule_retained_split_member_guard(
    output: &mut mwcc_machine_code::MachineFunction,
) -> bool {
    let Some(prefix) = output.instructions.windows(3).position(|window| {
        matches!(
            window,
            [
                Instruction::StoreWord {
                    s: 31,
                    a: 1,
                    offset: 12
                },
                Instruction::Or { a: 31, s: 3, b: 3 },
                Instruction::LoadWord {
                    d: 0,
                    a: 6,
                    offset: 20
                }
            ]
        )
    }) else {
        return false;
    };
    let Some(pointer) = output.instructions.windows(2).position(|window| {
        matches!(
            window,
            [
                Instruction::LoadWord {
                    d: 6,
                    a: 6,
                    offset: 16
                },
                Instruction::LoadByteZero {
                    d: 0,
                    a: 6,
                    offset: 0
                }
            ]
        )
    }) else {
        return false;
    };
    let retained_base_uses = output.instructions[pointer + 2..].iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::LoadWord {
                d: 0,
                a: 6,
                offset: 28
            }
        )
    }) && output.instructions[pointer + 2..].iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 24
            }
        )
    });
    if !retained_base_uses {
        return false;
    }
    let Some(wide) = output.instructions.windows(5).position(|window| {
        matches!(
            window,
            [
                Instruction::AddImmediateShifted {
                    d: 3,
                    a: 0,
                    immediate: 1
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: 3,
                    immediate: -1024
                },
                Instruction::StoreHalfword {
                    s: 0,
                    a: 31,
                    offset: 12
                },
                Instruction::LoadByteZero {
                    d: 0,
                    a: 31,
                    offset: 9
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: 1
                }
            ]
        )
    }) else {
        return false;
    };
    let moved_ranges = [prefix..prefix + 3, wide..wide + 5];
    if output.relocations.iter().any(|relocation| {
        moved_ranges
            .iter()
            .any(|range| range.contains(&relocation.instruction_index))
    }) || output.instructions.iter().any(|instruction| {
        let target = match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target } => *target,
            _ => return false,
        };
        moved_ranges.iter().any(|range| range.contains(&target))
    }) {
        return false;
    }

    output.instructions[prefix..prefix + 3].rotate_right(1);
    output.instructions[pointer] = Instruction::LoadWord {
        d: 3,
        a: 6,
        offset: 16,
    };
    output.instructions[pointer + 1] = Instruction::LoadByteZero {
        d: 0,
        a: 3,
        offset: 0,
    };
    output.instructions[wide] = Instruction::AddImmediateShifted {
        d: 4,
        a: 0,
        immediate: 1,
    };
    output.instructions[wide + 1] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: -1024,
    };
    output.instructions[wide + 1..wide + 5].rotate_right(1);
    true
}

#[cfg(test)]
mod tests {
    use super::schedule_retained_split_member_guard;
    use mwcc_machine_code::{Instruction, MachineFunction};

    #[test]
    fn preserves_the_split_tag_base_and_fills_both_dependency_slots() {
        let mut output = MachineFunction::new("probe");
        output.instructions = vec![
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::move_register(31, 3),
            Instruction::LoadWord {
                d: 0,
                a: 6,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 6,
                a: 6,
                offset: 16,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 6,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 6,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 6,
                immediate: 24,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 1,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: -1024,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: 31,
                offset: 12,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 31,
                offset: 9,
            },
            Instruction::load_immediate(3, 1),
        ];

        assert!(schedule_retained_split_member_guard(&mut output));
        assert!(matches!(
            output.instructions[0],
            Instruction::LoadWord {
                d: 0,
                a: 6,
                offset: 20
            }
        ));
        assert!(matches!(
            output.instructions[3],
            Instruction::LoadWord {
                d: 3,
                a: 6,
                offset: 16
            }
        ));
        assert!(matches!(
            output.instructions[4],
            Instruction::LoadByteZero { d: 0, a: 3, .. }
        ));
        assert!(matches!(
            output.instructions[7],
            Instruction::AddImmediateShifted { d: 4, .. }
        ));
        assert!(matches!(
            output.instructions[8],
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 1
            }
        ));
        assert!(matches!(
            output.instructions[9],
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: -1024
            }
        ));
    }
}
