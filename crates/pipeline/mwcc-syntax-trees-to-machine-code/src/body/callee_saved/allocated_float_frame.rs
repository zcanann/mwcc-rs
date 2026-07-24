//! ABI frame materialization for allocator-selected callee-saved FPRs.

#[allow(unused_imports)]
use super::*;
use super::allocated_float_frame_linkage_first::materialize_linkage_first_frame;

impl Generator {
    /// Expand an already scheduled predecrement non-leaf frame around the FPRs
    /// selected by register allocation. Each Gekko save occupies a 16-byte lane
    /// (`stfd` plus `psq_st`); existing locals and GPR saves retain their low
    /// offsets while the link slot moves to the enlarged frame's top.
    pub(crate) fn materialize_allocated_float_frame(
        &mut self,
        registers: &[u8],
        indexed_restore: bool,
    ) -> Compilation<()> {
        if registers.is_empty() {
            return Ok(());
        }
        let (permutation, lane_bytes) = match self.behavior.frame_convention {
            FrameConvention::Predecrement => (
                materialize_predecrement_frame(
                    &mut self.output.instructions,
                    registers,
                    indexed_restore,
                )
                .map_err(Diagnostic::error)?,
                16,
            ),
            FrameConvention::LinkageFirst => (
                materialize_linkage_first_frame(&mut self.output.instructions, registers)
                    .map_err(Diagnostic::error)?,
                8,
            ),
        };
        crate::remap_instruction_indices(self, &permutation);
        let count = u8::try_from(registers.len())
            .map_err(|_| Diagnostic::error("too many allocator-selected FPR saves"))?;
        self.callee_saved_float = count;
        self.frame_size = self
            .frame_size
            .checked_add(i16::from(count) * lane_bytes)
            .ok_or_else(|| Diagnostic::error("allocated FPR frame is too large"))?;
        Ok(())
    }
}

fn materialize_predecrement_frame(
    instructions: &mut Vec<Instruction>,
    registers: &[u8],
    indexed_restore: bool,
) -> Result<Vec<usize>, &'static str> {
    let expected: Vec<u8> = (0..registers.len())
        .map(|index| 31u8.saturating_sub(index as u8))
        .collect();
    if registers != expected {
        return Err("allocator-selected FPR saves are not a contiguous f31-down range");
    }
    let Some((frame_push, old_size)) =
        instructions
            .iter()
            .enumerate()
            .find_map(|(index, instruction)| match instruction {
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset } if *offset < 0 => {
                    Some((index, -*offset))
                }
                _ => None,
            })
    else {
        return Err("allocator-selected FPR saves require a predecrement frame");
    };
    let lane_bytes = i16::try_from(registers.len())
        .ok()
        .and_then(|count| count.checked_mul(16))
        .ok_or("allocated FPR frame is too large")?;
    let new_size = old_size
        .checked_add(lane_bytes)
        .ok_or("allocated FPR frame is too large")?;
    let link_offset = old_size
        .checked_add(4)
        .ok_or("allocated FPR link slot is out of range")?;
    let new_link_offset = new_size
        .checked_add(4)
        .ok_or("allocated FPR link slot is out of range")?;

    let link_store = instructions
        .iter()
        .position(|instruction| {
            matches!(instruction, Instruction::StoreWord { s: 0, a: 1, offset } if *offset == link_offset)
        })
        .ok_or("allocated FPR frame has no saved-link store")?;
    let last_call = instructions
        .iter()
        .rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .ok_or("allocator-selected FPR saves require a non-leaf body")?;
    let restore_at = instructions
        .iter()
        .enumerate()
        .skip(last_call + 1)
        .find_map(|(index, instruction)| match instruction {
            Instruction::LoadWord { d, a: 1, .. } if *d >= 14 => Some(index),
            Instruction::LoadWord { d: 0, a: 1, offset } if *offset == link_offset => Some(index),
            _ => None,
        })
        .ok_or("allocated FPR frame has no epilogue restore point")?;

    if instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::StoreFloatDouble { s, a: 1, .. }
                | Instruction::PairedSingleQuantizedStore { s, a: 1, .. }
                if registers.contains(s)
        )
    }) {
        return Err("allocator-selected FPR frame already contains FPR saves");
    }

    if let Instruction::StoreWordWithUpdate { offset, .. } = &mut instructions[frame_push] {
        *offset = -new_size;
    }
    for instruction in instructions.iter_mut() {
        match instruction {
            Instruction::StoreWord { s: 0, a: 1, offset }
            | Instruction::LoadWord { d: 0, a: 1, offset }
                if *offset == link_offset =>
            {
                *offset = new_link_offset;
            }
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate,
            } if *immediate == old_size => {
                *immediate = new_size;
            }
            _ => {}
        }
    }

    let mut saves = Vec::with_capacity(registers.len() * 2);
    let mut restores =
        Vec::with_capacity(registers.len() * usize::from(indexed_restore) + registers.len() * 2);
    for (index, register) in registers.iter().copied().enumerate() {
        let double_offset = new_size - 16 * (index as i16 + 1);
        let paired_offset = double_offset + 8;
        saves.extend([
            Instruction::StoreFloatDouble {
                s: register,
                a: 1,
                offset: double_offset,
            },
            Instruction::PairedSingleQuantizedStore {
                s: register,
                a: 1,
                offset: paired_offset,
                w: 0,
                i: 0,
            },
        ]);
        if indexed_restore {
            restores.extend([
                Instruction::load_immediate(0, paired_offset),
                Instruction::PairedSingleQuantizedLoadIndexed {
                    d: register,
                    a: 1,
                    b: 0,
                    w: 0,
                    i: 0,
                },
                Instruction::LoadFloatDouble {
                    d: register,
                    a: 1,
                    offset: double_offset,
                },
            ]);
        } else {
            restores.extend([
                Instruction::PairedSingleQuantizedLoad {
                    d: register,
                    a: 1,
                    offset: paired_offset,
                    w: 0,
                    i: 0,
                },
                Instruction::LoadFloatDouble {
                    d: register,
                    a: 1,
                    offset: double_offset,
                },
            ]);
        }
    }

    let old = std::mem::take(instructions);
    let old_len = old.len();
    let save_at = link_store + 1;
    let mut permutation = vec![0usize; old_len];
    let mut rebuilt = Vec::with_capacity(old_len + saves.len() + restores.len());
    for (index, instruction) in old.into_iter().enumerate() {
        if index == save_at {
            rebuilt.append(&mut saves);
        }
        if index == restore_at {
            rebuilt.append(&mut restores);
        }
        permutation[index] = rebuilt.len();
        rebuilt.push(instruction);
    }
    *instructions = rebuilt;
    Ok(permutation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_a_wii_predecrement_frame_for_two_saved_fprs() {
        let mut instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::BranchAndLink {
                target: "call".into(),
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ];
        let permutation =
            materialize_predecrement_frame(&mut instructions, &[31, 30], true).unwrap();

        assert_eq!(permutation[3], 7);
        assert!(matches!(
            instructions[0],
            Instruction::StoreWordWithUpdate { offset: -48, .. }
        ));
        assert!(matches!(
            instructions[3],
            Instruction::StoreFloatDouble {
                s: 31,
                offset: 32,
                ..
            }
        ));
        assert!(matches!(
            instructions[5],
            Instruction::StoreFloatDouble {
                s: 30,
                offset: 16,
                ..
            }
        ));
        assert!(matches!(
            instructions[8],
            Instruction::AddImmediate {
                d: 0,
                immediate: 40,
                ..
            }
        ));
        assert!(matches!(
            instructions.last(),
            Some(Instruction::BranchToLinkRegister)
        ));
    }

}
