//! Linkage-first entry scheduling for frame-backed variadic calls.
//!
//! Build 163 fills the two linkage hazards with ready constant arguments and
//! CR1 setup, then overlaps a stack-address argument with the independent load
//! of the call receiver. Selection and allocation deliberately keep those
//! operations serial; this owner recognizes the complete physical entry shape
//! before applying the measured permutation.

#[allow(unused_imports)]
use super::*;
use std::collections::HashSet;

impl Generator {
    pub(crate) fn schedule_linkage_first_variadic_frame_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !is_linkage_first_frame_prefix(&self.output.instructions)
        {
            return;
        }
        if let Some((from, to)) = guarded_variadic_address_clear_move(
            &self.output,
            &self.variadic_callees,
        ) {
            self.move_instruction_before(from, to);
        }
        self.schedule_variadic_constant_triplets();
        let Some(first_call) =
            entry_variadic_call(&self.output.instructions, &self.variadic_callees)
        else {
            return;
        };
        if !has_entry_argument_setup(&self.output.instructions, first_call) {
            return;
        }

        let first_constant = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate { d: 5, a: 0, .. }
                )
            })
            .expect("the setup recognizer found r5");
        self.move_instruction_before(first_constant, 1);

        let first_call = first_variadic_call(&self.output.instructions, &self.variadic_callees)
            .expect("the variadic call remains after a prefix move");
        let condition_clear = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::ConditionRegisterClear { d: 6 })
            })
            .expect("the setup recognizer found the CR clear");
        self.move_instruction_before(condition_clear, 3);

        let first_call = first_variadic_call(&self.output.instructions, &self.variadic_callees)
            .expect("the variadic call remains after a prefix move");
        let terminal_constant = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 6,
                        a: 0,
                        immediate: -1
                    }
                )
            })
            .expect("the setup recognizer found r6");
        self.move_instruction_before(terminal_constant, 4);

        self.overlap_first_frame_argument_with_receiver_load();
        self.use_linkage_first_variadic_receiver_copies();
        self.schedule_later_variadic_argument_packets();
    }

    fn schedule_variadic_constant_triplets(&mut self) {
        let mut start = 0;
        while start + 4 < self.output.instructions.len() {
            if is_variadic_constant_triplet(
                &self.output.instructions[start..start + 5],
                &self.variadic_callees,
            ) {
                self.move_instruction_before(start + 3, start);
                start += 5;
            } else {
                start += 1;
            }
        }
    }

    fn overlap_first_frame_argument_with_receiver_load(&mut self) {
        let Some(first_call) =
            first_variadic_call(&self.output.instructions, &self.variadic_callees)
        else {
            return;
        };
        let Some(frame_push) = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }
                )
            })
        else {
            return;
        };
        let Some(frame_argument) = self.output.instructions[frame_push + 1..first_call]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::AddImmediate { d: 4, a: 1, .. })
            })
            .map(|offset| frame_push + 1 + offset)
        else {
            return;
        };
        let Some(receiver_load) = self.output.instructions[frame_push + 1..frame_argument]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::LoadWord { d, a, .. } if *d >= 14 && *a >= 3)
            })
            .map(|offset| frame_push + 1 + offset)
        else {
            return;
        };
        let incoming_copy = self.output.instructions[frame_push + 1..frame_argument]
            .iter()
            .position(is_saved_incoming_r4_copy)
            .map(|offset| frame_push + 1 + offset);
        let insertion = if incoming_copy.is_some_and(|copy| copy > receiver_load) {
            self.move_instruction_before(
                incoming_copy.expect("the later incoming copy was checked"),
                receiver_load,
            );
            receiver_load + 1
        } else {
            receiver_load
        };
        self.move_instruction_before(frame_argument, insertion);
    }

    fn use_linkage_first_variadic_receiver_copies(&mut self) {
        let mut block_start = 0;
        for call in 0..self.output.instructions.len() {
            let variadic = matches!(
                &self.output.instructions[call],
                Instruction::BranchAndLink { target }
                    if self.variadic_callees.contains(target)
            );
            if !variadic {
                if matches!(
                    self.output.instructions[call],
                    Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
                ) {
                    block_start = call + 1;
                }
                continue;
            }
            if let Some(copy) =
                self.output.instructions[block_start..call]
                    .iter()
                    .rposition(|instruction| {
                        matches!(
                            instruction,
                            Instruction::Or { a: 3, s, b }
                                if s == b && *s >= 14
                        )
                    })
            {
                let copy = block_start + copy;
                let Instruction::Or { s, .. } = self.output.instructions[copy] else {
                    unreachable!("the receiver-copy recognizer selected an or")
                };
                self.output.instructions[copy] = Instruction::AddImmediate {
                    d: 3,
                    a: s,
                    immediate: 0,
                };
            }
            block_start = call + 1;
        }
    }

    fn schedule_later_variadic_argument_packets(&mut self) {
        let mut start = 0;
        while start + 5 < self.output.instructions.len() {
            let matches_packet = is_later_variadic_argument_packet(
                &self.output.instructions[start..start + 6],
                &self.variadic_callees,
            );
            if matches_packet {
                self.move_instruction_before(start + 4, start + 1);
                start += 6;
            } else {
                start += 1;
            }
        }
    }
}

fn is_saved_incoming_r4_copy(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::Or {
            a: 14..=31,
            s: 4,
            b: 4,
        } | Instruction::AddImmediate {
            d: 14..=31,
            a: 4,
            immediate: 0,
        }
    )
}

fn guarded_variadic_address_clear_move(
    output: &mwcc_machine_code::MachineFunction,
    variadic_callees: &HashSet<String>,
) -> Option<(usize, usize)> {
    let instructions = &output.instructions;
    for high in 0..instructions.len().saturating_sub(3) {
        let [
            Instruction::AddImmediateShifted {
                d: high_register,
                a: 0,
                ..
            },
            Instruction::AddImmediate {
                d: low_destination,
                a: low_base,
                ..
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] = &instructions[high..high + 4]
        else {
            continue;
        };
        let low = high + 1;
        let clear = high + 2;
        if high_register != low_destination
            || high_register != low_base
            || !variadic_callees.contains(target)
            || instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target }
                        if *target == low || *target == clear
                )
            })
        {
            continue;
        }
        let high_target = output.relocations.iter().find_map(|relocation| {
            (relocation.instruction_index == high
                && relocation.kind == RelocationKind::Addr16Ha)
                .then(|| match &relocation.target {
                    mwcc_machine_code::RelocationTarget::External(target) => Some(target.as_str()),
                    _ => None,
                })
                .flatten()
        });
        let low_target = output.relocations.iter().find_map(|relocation| {
            (relocation.instruction_index == low
                && relocation.kind == RelocationKind::Addr16Lo)
                .then(|| match &relocation.target {
                    mwcc_machine_code::RelocationTarget::External(target) => Some(target.as_str()),
                    _ => None,
                })
                .flatten()
        });
        if high_target.is_some() && high_target == low_target {
            return Some((clear, low));
        }
    }
    None
}

fn is_later_variadic_argument_packet(
    instructions: &[Instruction],
    variadic_callees: &HashSet<String>,
) -> bool {
    matches!(
        instructions,
        [
            Instruction::AddImmediate {
                d: 3,
                a: receiver,
                immediate: 0
            },
            Instruction::AddImmediate { d: 4, .. },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: -1
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] if *receiver >= 14 && variadic_callees.contains(target)
    )
}

fn is_variadic_constant_triplet(
    instructions: &[Instruction],
    variadic_callees: &HashSet<String>,
) -> bool {
    matches!(
        instructions,
        [
            Instruction::AddImmediate { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target },
        ] if variadic_callees.contains(target)
    )
}

fn is_linkage_first_frame_prefix(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4
            },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset },
            ..
        ] if *offset < 0
    )
}

fn first_variadic_call(
    instructions: &[Instruction],
    variadic_callees: &HashSet<String>,
) -> Option<usize> {
    instructions.iter().position(|instruction| {
        matches!(
            instruction,
            Instruction::BranchAndLink { target } if variadic_callees.contains(target)
        )
    })
}

fn entry_variadic_call(
    instructions: &[Instruction],
    variadic_callees: &HashSet<String>,
) -> Option<usize> {
    let first_call = instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))?;
    matches!(
        &instructions[first_call],
        Instruction::BranchAndLink { target } if variadic_callees.contains(target)
    )
    .then_some(first_call)
}

fn has_entry_argument_setup(instructions: &[Instruction], first_call: usize) -> bool {
    let prefix = &instructions[..first_call];
    prefix.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::AddImmediate { d: 5, a: 0, .. }
        )
    }) && prefix.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: -1
            }
        )
    }) && prefix
        .iter()
        .any(|instruction| matches!(instruction, Instruction::ConditionRegisterClear { d: 6 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    #[test]
    fn recognizes_only_the_complete_linkage_first_variadic_prefix() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 2,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: -1,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "format".into(),
            },
        ];

        assert!(is_linkage_first_frame_prefix(&instructions));
        assert!(has_entry_argument_setup(&instructions, 6));
    }

    #[test]
    fn rejects_a_variadic_call_after_an_earlier_call() {
        let variadic_callees = HashSet::from(["format".into()]);
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -64,
            },
            Instruction::BranchAndLink {
                target: "prepare".into(),
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 2,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: -1,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "format".into(),
            },
        ];

        assert_eq!(
            entry_variadic_call(&instructions, &variadic_callees),
            None
        );
    }

    #[test]
    fn schedules_only_a_complete_later_variadic_argument_packet() {
        let variadic_callees = HashSet::from(["format".into()]);
        let instructions = vec![
            Instruction::AddImmediate {
                d: 3,
                a: 31,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 28,
            },
            Instruction::AddImmediate {
                d: 5,
                a: 0,
                immediate: 5,
            },
            Instruction::AddImmediate {
                d: 6,
                a: 0,
                immediate: -1,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "format".into(),
            },
        ];

        assert!(is_later_variadic_argument_packet(
            &instructions,
            &variadic_callees
        ));
    }

    #[test]
    fn recognizes_a_three_constant_variadic_argument_packet() {
        let variadic_callees = HashSet::from(["panic".into()]);
        let instructions = vec![
            Instruction::load_immediate(3, 0),
            Instruction::load_immediate(4, 2902),
            Instruction::load_immediate(5, 0),
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "panic".into(),
            },
        ];

        assert!(is_variadic_constant_triplet(
            &instructions,
            &variadic_callees
        ));
    }

    #[test]
    fn recognizes_a_saved_copy_of_incoming_r4() {
        assert!(is_saved_incoming_r4_copy(&Instruction::move_register(29, 4)));
        assert!(is_saved_incoming_r4_copy(&Instruction::AddImmediate {
            d: 29,
            a: 4,
            immediate: 0,
        }));
        assert!(!is_saved_incoming_r4_copy(&Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 40,
        }));
    }

    #[test]
    fn fills_a_guarded_variadic_address_latency_slot_with_cr_clear() {
        let variadic_callees = HashSet::from(["report".into()]);
        let mut output = mwcc_machine_code::MachineFunction::new("guarded");
        output.instructions = vec![
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 5,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink {
                target: "report".into(),
            },
        ];
        output.relocations = vec![
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("@message".into()),
            },
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("@message".into()),
            },
        ];

        assert_eq!(
            guarded_variadic_address_clear_move(&output, &variadic_callees),
            Some((3, 2))
        );
    }
}
