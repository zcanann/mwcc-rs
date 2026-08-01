//! Final physical schedule for a compact five-word runtime trampoline frame.
//!
//! The contiguous image copy and the following read/write diamond are selected
//! by separate owners. Build 163 overlaps their independent condition with the
//! pool-address latency, uses linkage-first prologue order, and folds adjacent
//! high-half OR operations in each diamond arm. This pass commits that schedule
//! only after the complete allocated region is present.

use super::*;
use mwcc_machine_code::RelocationTarget;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PhysicalPlan {
    condition: usize,
    shifted_or_pairs: Vec<usize>,
    spr_else_constant: Option<usize>,
    spr_packets: Vec<(usize, u8)>,
}

fn relocation_target(
    output: &mwcc_machine_code::MachineFunction,
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&RelocationTarget> {
    output.relocations.iter().find_map(|relocation| {
        (relocation.instruction_index == instruction_index && relocation.kind == kind)
            .then_some(&relocation.target)
    })
}

fn has_incoming_branch(instructions: &[Instruction], target: usize) -> bool {
    instructions.iter().any(|instruction| match instruction {
        Instruction::Branch { target: branch_target }
        | Instruction::BranchConditionalForward {
            target: branch_target,
            ..
        } => *branch_target == target,
        _ => false,
    })
}

fn spr_packets(instructions: &[Instruction]) -> Vec<(usize, u8)> {
    instructions
        .windows(7)
        .enumerate()
        .filter_map(|(index, window)| {
            match window {
                [
                    Instruction::RotateAndMask { a: 0, s: 4, shift: 0, begin: 20, end: 26 },
                    Instruction::ShiftLeftImmediate { a: upper, s: 0, shift: 6 },
                    Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 27 },
                    Instruction::OrImmediateShifted { a: 4, s: or_upper, immediate: 0x7c80 },
                    Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 16 },
                    Instruction::Or { a: 0, s: 4, b: 0 },
                    Instruction::OrImmediate { a: 0, s: 0, .. },
                ] if upper == or_upper && *upper >= 6 => Some((index, *upper)),
                _ => None,
            }
        })
        .collect()
}

fn spr_else_constant_schedule(instructions: &[Instruction]) -> Option<usize> {
    instructions.windows(10).enumerate().find_map(|(index, window)| {
        match window {
            [
                Instruction::AddImmediateShifted { d: constant, a: 0, .. },
                Instruction::StoreWord { s: stored_constant, a: 1, offset: 8 },
                Instruction::RotateAndMask { a: 0, s: 4, shift: 0, begin: 20, end: 26 },
                Instruction::ShiftLeftImmediate { a: upper, s: 0, shift: 6 },
                Instruction::ClearLeftImmediate { a: 0, s: 4, clear: 27 },
                Instruction::OrImmediateShifted { a: 4, s: or_upper, immediate: 0x7c80 },
                Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 16 },
                Instruction::Or { a: 0, s: 4, b: 0 },
                Instruction::OrImmediate { a: 0, s: 0, .. },
                Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            ] if constant == stored_constant && upper == or_upper && *upper >= 6 => Some(index),
            _ => None,
        }
    })
}

fn physical_plan(output: &mwcc_machine_code::MachineFunction) -> Option<PhysicalPlan> {
    let instructions = &output.instructions;
    if !matches!(
        instructions.get(0..16),
        Some([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                ..
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate { d: 7, a: 6, .. },
            Instruction::LoadWord {
                d: 6,
                a: 7,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 6,
                a: 1,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 6,
                a: 7,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 12,
            },
            Instruction::StoreWord {
                s: 6,
                a: 1,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 24,
            },
            Instruction::CompareWordImmediate { immediate: 0, .. },
        ])
    ) {
        return None;
    }
    let RelocationTarget::AnonymousRodataAt(image) =
        relocation_target(output, 2, RelocationKind::Addr16Ha)?
    else {
        return None;
    };
    if !matches!(
        relocation_target(output, 4, RelocationKind::Addr16Lo),
        Some(RelocationTarget::AnonymousRodataAt(low_image)) if low_image == image
    )
        || !matches!(
            instructions.get(16),
            Some(Instruction::BranchConditionalForward { .. })
        )
    {
        return None;
    }

    let shifted_or_pairs: Vec<usize> = instructions
        .windows(2)
        .enumerate()
        .filter_map(|(index, pair)| match pair {
            [
                Instruction::OrImmediateShifted {
                    a,
                    s,
                    immediate: first,
                },
                Instruction::OrImmediateShifted {
                    a: next_a,
                    s: next_s,
                    immediate: second,
                },
            ] if a == next_a
                && a == next_s
                && a == s
                && *first != 0
                && *second != 0
                && !has_incoming_branch(instructions, index + 1)
                && output
                    .relocations
                    .iter()
                    .all(|relocation| relocation.instruction_index != index + 1) =>
            {
                Some(index)
            }
            _ => None,
        })
        .collect();
    let spr_else_constant = spr_else_constant_schedule(instructions);
    let spr_packets = spr_packets(instructions);
    match shifted_or_pairs.as_slice() {
        [first_pair, second_pair]
            if *first_pair > 16
                && *second_pair > first_pair + 2
                && spr_else_constant.is_none()
                && spr_packets.is_empty() => {}
        [] if spr_else_constant.is_some_and(|constant| {
            matches!(spr_packets.as_slice(), [(first, _), (second, _)]
                if *first > 16 && *second == constant + 2)
        }) => {}
        _ => return None,
    }

    let epilogue = instructions.len().checked_sub(4)?;
    if !matches!(
        instructions.get(epilogue..),
        Some([
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ])
    ) {
        return None;
    }

    Some(PhysicalPlan {
        condition: 15,
        shifted_or_pairs,
        spr_else_constant,
        spr_packets,
    })
}

impl Generator {
    pub(crate) fn finalize_linkage_first_instruction_array_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.frame_size != 32
            || !self.non_leaf
            || !self.callee_saved.is_empty()
            || self.callee_saved_float != 0
            || self.frame_slots.len() != 1
            || !self.frame_slots.values().any(|slot| {
                slot.offset == 8
                    && slot.size == 20
                    && slot.is_array
                    && matches!(slot.value_type, Type::Int | Type::UnsignedInt)
            })
        {
            return;
        }
        let Some(plan) = physical_plan(&self.output) else {
            return;
        };

        for first in plan.shifted_or_pairs.into_iter().rev() {
            let second_immediate = match self.output.instructions[first + 1] {
                Instruction::OrImmediateShifted { immediate, .. } => immediate,
                _ => unreachable!("the instruction-array plan owns an OR pair"),
            };
            let Instruction::OrImmediateShifted { immediate, .. } =
                &mut self.output.instructions[first]
            else {
                unreachable!("the instruction-array plan owns an OR pair")
            };
            *immediate |= second_immediate;
            crate::remove_instruction_retargeting_to_next(self, first + 1);
        }

        for (start, upper) in &plan.spr_packets {
            if *upper == 6 {
                continue;
            }
            let Instruction::ShiftLeftImmediate { a, .. } =
                &mut self.output.instructions[start + 1]
            else {
                unreachable!("the instruction-array plan owns its SPR upper shift")
            };
            *a = 6;
            let Instruction::OrImmediateShifted { s, .. } =
                &mut self.output.instructions[start + 3]
            else {
                unreachable!("the instruction-array plan owns its SPR upper merge")
            };
            *s = 6;
        }

        if let Some(constant) = plan.spr_else_constant {
            // Generic arm emission completes the leading constant store before
            // starting the second SPR word. Build 163 instead issues the three
            // source-dependent mask operations, fills their latency with the
            // independent `lis`, then commits that constant between `oris` and
            // the remaining low-field operations.
            let Instruction::AddImmediateShifted { d, .. } =
                &mut self.output.instructions[constant]
            else {
                unreachable!("the instruction-array plan owns its else constant")
            };
            *d = 7;
            let Instruction::StoreWord { s, .. } =
                &mut self.output.instructions[constant + 1]
            else {
                unreachable!("the instruction-array plan owns its else constant store")
            };
            *s = 7;
            crate::move_instruction_before_retargeting(self, constant + 2, constant);
            crate::move_instruction_before_retargeting(self, constant + 3, constant + 1);
            crate::move_instruction_before_retargeting(self, constant + 4, constant + 2);
            crate::move_instruction_before_retargeting(self, constant + 5, constant + 4);
            let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[16]
            else {
                unreachable!("the instruction-array plan owns its diamond branch")
            };
            *target = constant;
        }

        let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[3] else {
            unreachable!("the instruction-array plan owns its link store")
        };
        *offset = 4;

        crate::move_instruction_before_retargeting(self, 1, 0);
        crate::move_instruction_before_retargeting(self, 3, 1);
        crate::move_instruction_before_retargeting(self, plan.condition, 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation};

    fn candidate() -> MachineFunction {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted {
                d: 6,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 7,
                a: 6,
                immediate: 0,
            },
        ];
        for pair in 0..2i16 {
            instructions.extend([
                Instruction::LoadWord {
                    d: 6,
                    a: 7,
                    offset: pair * 8,
                },
                Instruction::LoadWord {
                    d: 0,
                    a: 7,
                    offset: pair * 8 + 4,
                },
                Instruction::StoreWord {
                    s: 6,
                    a: 1,
                    offset: 8 + pair * 8,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 12 + pair * 8,
                },
            ]);
        }
        instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 7,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 24,
            },
            Instruction::CompareWordImmediate { a: 5, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 23,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 21,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 0xf000,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::Branch { target: 27 },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 4,
                shift: 21,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 0xe000,
            },
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 3,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 8,
            },
            Instruction::BranchAndLink {
                target: "access".into(),
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        MachineFunction {
            instructions,
            relocations: vec![
                Relocation {
                    instruction_index: 2,
                    kind: RelocationKind::Addr16Ha,
                    target: RelocationTarget::AnonymousRodataAt(0),
                },
                Relocation {
                    instruction_index: 4,
                    kind: RelocationKind::Addr16Lo,
                    target: RelocationTarget::AnonymousRodataAt(0),
                },
            ],
            ..Default::default()
        }
    }

    fn spr_word(low_bits: u16) -> Vec<Instruction> {
        vec![
            Instruction::RotateAndMask {
                a: 0,
                s: 4,
                shift: 0,
                begin: 20,
                end: 26,
            },
            Instruction::ShiftLeftImmediate {
                a: 6,
                s: 0,
                shift: 6,
            },
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 4,
                clear: 27,
            },
            Instruction::OrImmediateShifted {
                a: 4,
                s: 6,
                immediate: 0x7c80,
            },
            Instruction::ShiftLeftImmediate {
                a: 0,
                s: 0,
                shift: 16,
            },
            Instruction::Or {
                a: 0,
                s: 4,
                b: 0,
            },
            Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: low_bits,
            },
        ]
    }

    fn spr_candidate() -> MachineFunction {
        let mut output = candidate();
        let mut body = spr_word(0x2a6);
        body.extend([
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: -28541,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::Branch { target: 38 },
            Instruction::AddImmediateShifted {
                d: 7,
                a: 0,
                immediate: -32637,
            },
            Instruction::StoreWord {
                s: 7,
                a: 1,
                offset: 8,
            },
        ]);
        body.extend(spr_word(0x3a6));
        body.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 12,
        });
        output.instructions.splice(17..26, body);
        output.instructions[16] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 28,
        };
        output
    }

    #[test]
    fn recognizes_the_complete_pool_copy_and_diamond() {
        assert_eq!(
            physical_plan(&candidate()),
            Some(PhysicalPlan {
                condition: 15,
                shifted_or_pairs: vec![18, 23],
                spr_else_constant: None,
                spr_packets: vec![],
            })
        );
    }

    #[test]
    fn rejects_a_branch_into_the_second_shifted_or() {
        let mut output = candidate();
        output.instructions[16] = Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 19,
        };

        assert_eq!(physical_plan(&output), None);
    }

    #[test]
    fn recognizes_the_spr_packet_with_a_leading_else_constant() {
        assert_eq!(
            physical_plan(&spr_candidate()),
            Some(PhysicalPlan {
                condition: 15,
                shifted_or_pairs: vec![],
                spr_else_constant: Some(28),
                spr_packets: vec![(17, 6), (30, 6)],
            })
        );
    }
}
