//! Build-163 schedules selected after translation-unit assembly.
//!
//! An earlier assembly function is an optimizer barrier: later parameter-only
//! call bodies stop filling linkage latency slots, complete their saved-home
//! copies in source order, and materialize indirect-call addresses before the
//! remaining register arguments.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_asm_barrier_entry(
        &mut self,
        saved: &[u8],
    ) -> bool {
        if !self.preceded_by_asm
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_callee_saved_frame_layout
                != LegacyCalleeSavedFrameLayout::CompactValueHomes
            || saved.is_empty()
        {
            return false;
        }
        let Some(stack_update) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, .. })
        }) else {
            return false;
        };
        let Some(first_call) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::BranchAndLink { .. })
        }) else {
            return false;
        };
        if stack_update >= first_call
            || self.output.instructions[stack_update + 1..first_call]
                .iter()
                .any(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
                    )
                })
        {
            return false;
        }

        let mut copies = self.output.instructions[stack_update + 1..first_call]
            .iter()
            .filter_map(asm_barrier_entry_copy)
            .filter(|(destination, _)| saved.contains(destination))
            .collect::<Vec<_>>();
        if copies.len() != saved.len()
            || saved.iter().any(|register| {
                !self.output.instructions[stack_update + 1..first_call]
                    .iter()
                    .any(|instruction| {
                        matches!(instruction,
                            Instruction::StoreWord { s, a: 1, .. } if s == register)
                    })
            })
        {
            return false;
        }

        let mut insertion = stack_update + 1;
        for register in saved {
            let Some(store) = self.output.instructions[insertion..first_call]
                .iter()
                .position(|instruction| {
                    matches!(instruction,
                        Instruction::StoreWord { s, a: 1, .. } if s == register)
                })
                .map(|offset| insertion + offset)
            else {
                return false;
            };
            if store != insertion {
                crate::move_instruction_before_retargeting(self, store, insertion);
            }
            insertion += 1;
        }

        copies.sort_unstable_by_key(|(_, source)| *source);
        for (destination, source) in copies {
            let Some(copy) = self.output.instructions[insertion..first_call]
                .iter()
                .position(|instruction| {
                    asm_barrier_entry_copy(instruction) == Some((destination, source))
                })
                .map(|offset| insertion + offset)
            else {
                return false;
            };
            if copy != insertion {
                crate::move_instruction_before_retargeting(self, copy, insertion);
            }
            self.output.instructions[insertion] =
                Instruction::move_register(destination, source);
            insertion += 1;
        }
        self.normalize_asm_barrier_stored_constant(insertion);
        true
    }

    fn normalize_asm_barrier_stored_constant(&mut self, start: usize) {
        if !matches!(
            self.output.instructions.get(start..start + 3),
            Some([
                Instruction::AddImmediateShifted { d: high, a: 0, .. },
                Instruction::AddImmediate { d: 0, a: low_base, .. },
                Instruction::StoreWord { s: 0, a, .. },
            ]) if high == low_base && *a != 1
        ) || self.output.relocations.iter().any(|relocation| {
            (start..start + 2).contains(&relocation.instruction_index)
        }) {
            return;
        }
        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[start]
        else {
            unreachable!("the asm-barrier constant high was matched above")
        };
        *d = 3;
        let Instruction::AddImmediate { a, .. } =
            &mut self.output.instructions[start + 1]
        else {
            unreachable!("the asm-barrier constant low was matched above")
        };
        *a = 3;
    }

    pub(crate) fn schedule_linkage_first_asm_barrier_indirect_call(&mut self) -> bool {
        if !self.preceded_by_asm
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
        {
            return false;
        }
        let Some(tail) = relocated_indirect_tail(
            &self.output.instructions,
            &self.output.relocations,
        ) else {
            return false;
        };

        if tail.low != tail.start + 1 {
            crate::move_instruction_before_retargeting(self, tail.low, tail.start + 1);
        }
        let argument = saved_copy_position(
            &self.output.instructions,
            tail.start + 2..=tail.call,
            3,
        )
        .expect("the recognized asm-barrier tail retains its argument copy");
        if argument != tail.start + 2 {
            crate::move_instruction_before_retargeting(self, argument, tail.start + 2);
        }
        let pointer = saved_copy_position(
            &self.output.instructions,
            tail.start + 3..=tail.call,
            12,
        )
        .expect("the recognized asm-barrier tail retains its pointer copy");
        if pointer != tail.start + 3 {
            crate::move_instruction_before_retargeting(self, pointer, tail.start + 3);
        }

        let Instruction::AddImmediateShifted { d, .. } =
            &mut self.output.instructions[tail.start]
        else {
            unreachable!("the recognized relocated high remains first")
        };
        *d = 3;
        let Instruction::AddImmediate { a, .. } =
            &mut self.output.instructions[tail.start + 1]
        else {
            unreachable!("the recognized relocated low remains second")
        };
        *a = 3;
        true
    }
}

fn asm_barrier_entry_copy(instruction: &Instruction) -> Option<(u8, u8)> {
    match *instruction {
        Instruction::Or { a, s, b } if s == b && s < 14 => Some((a, s)),
        Instruction::AddImmediate {
            d,
            a,
            immediate: 0,
        } if a < 14 => Some((d, a)),
        _ => None,
    }
}

fn saved_copy_position(
    instructions: &[Instruction],
    range: std::ops::RangeInclusive<usize>,
    destination: u8,
) -> Option<usize> {
    range.into_iter().find(|index| {
        matches!(instructions[*index],
            Instruction::Or { a, s, b }
                if a == destination && s == b && (14..=31).contains(&s))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RelocatedIndirectTail {
    start: usize,
    low: usize,
    call: usize,
}

fn relocated_indirect_tail(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
) -> Option<RelocatedIndirectTail> {
    let call = instructions
        .iter()
        .rposition(|instruction| matches!(instruction, Instruction::BranchToLinkRegisterAndLink))?;
    let start = relocations
        .iter()
        .filter(|relocation| {
            relocation.kind == RelocationKind::Addr16Ha
                && relocation.instruction_index < call
        })
        .map(|relocation| relocation.instruction_index)
        .max()?;
    if call != start + 5 {
        return None;
    }
    let high = relocations.iter().find(|relocation| {
        relocation.instruction_index == start && relocation.kind == RelocationKind::Addr16Ha
    })?;
    let high_target = external_target(high)?;
    let low = relocations.iter().find(|relocation| {
        relocation.kind == RelocationKind::Addr16Lo
            && (start + 1..call).contains(&relocation.instruction_index)
            && external_target(relocation) == Some(high_target)
    })?;
    let region = &instructions[start..=call];
    let complete = matches!(instructions[start], Instruction::AddImmediateShifted { a: 0, .. })
        && matches!(instructions[low.instruction_index], Instruction::AddImmediate { d: 4, .. })
        && saved_copy_position(instructions, start..=call, 3).is_some()
        && saved_copy_position(instructions, start..=call, 12).is_some()
        && region
            .iter()
            .any(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { s: 12 }));
    complete.then_some(RelocatedIndirectTail {
        start,
        low: low.instruction_index,
        call,
    })
}

fn external_target(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(target) => Some(target),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relocation(
        instruction_index: usize,
        kind: RelocationKind,
        target: &str,
    ) -> mwcc_machine_code::Relocation {
        mwcc_machine_code::Relocation {
            instruction_index,
            kind,
            target: mwcc_machine_code::RelocationTarget::External(target.to_owned()),
        }
    }

    #[test]
    fn recognizes_a_relocated_indirect_tail_behind_an_asm_barrier() {
        let instructions = vec![
            Instruction::load_immediate_shifted(4, 0),
            Instruction::move_register(12, 31),
            Instruction::move_register(3, 30),
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: 0,
            },
            Instruction::MoveToLinkRegister { s: 12 },
            Instruction::BranchToLinkRegisterAndLink,
        ];
        let relocations = vec![
            relocation(0, RelocationKind::Addr16Ha, "argument"),
            relocation(3, RelocationKind::Addr16Lo, "argument"),
        ];

        assert_eq!(
            relocated_indirect_tail(&instructions, &relocations),
            Some(RelocatedIndirectTail {
                start: 0,
                low: 3,
                call: 5,
            })
        );
    }
}
