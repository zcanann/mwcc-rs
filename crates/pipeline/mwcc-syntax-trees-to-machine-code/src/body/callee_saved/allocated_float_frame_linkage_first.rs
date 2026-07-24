//! Build 163 linkage-first materialization for allocator-selected FPRs.

#[allow(unused_imports)]
use super::*;

/// Add compact double-only FPR lanes after the GPR/linkage frame has been
/// normalized. Existing locals and GPR homes stay at their low offsets; the
/// FPRs occupy the new high lanes below the caller linkage area.
pub(super) fn materialize_linkage_first_frame(
    instructions: &mut Vec<Instruction>,
    registers: &[u8],
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
        return Err("allocator-selected FPR saves require a linkage-first frame");
    };
    if !instructions[..frame_push].iter().any(|instruction| {
        matches!(instruction, Instruction::StoreWord { s: 0, a: 1, offset: 4 })
    }) {
        return Err("allocator-selected FPR saves require a normalized linkage-first frame");
    }
    let lane_bytes = i16::try_from(registers.len())
        .ok()
        .and_then(|count| count.checked_mul(8))
        .ok_or("allocated FPR frame is too large")?;
    let new_size = old_size
        .checked_add(lane_bytes)
        .ok_or("allocated FPR frame is too large")?;
    let old_link_load = old_size
        .checked_add(4)
        .ok_or("allocated FPR link slot is out of range")?;
    let new_link_load = new_size
        .checked_add(4)
        .ok_or("allocated FPR link slot is out of range")?;
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
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate,
            } if *immediate == old_size => Some(index),
            _ => None,
        })
        .ok_or("allocator-selected FPR frame has no epilogue restore point")?;

    if instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::StoreFloatDouble { s, a: 1, .. } if registers.contains(s)
        )
    }) {
        return Err("allocator-selected FPR frame already contains FPR saves");
    }
    if let Instruction::StoreWordWithUpdate { offset, .. } = &mut instructions[frame_push] {
        *offset = -new_size;
    }
    for instruction in instructions.iter_mut() {
        match instruction {
            Instruction::LoadWord { d: 0, a: 1, offset } if *offset == old_link_load => {
                *offset = new_link_load;
            }
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate,
            } if *immediate == old_size => *immediate = new_size,
            _ => {}
        }
    }

    let mut saves = Vec::with_capacity(registers.len());
    let mut restores = Vec::with_capacity(registers.len());
    for (index, register) in registers.iter().copied().enumerate() {
        let offset = new_size - 8 * (index as i16 + 1);
        saves.push(Instruction::StoreFloatDouble {
            s: register,
            a: 1,
            offset,
        });
        restores.push(Instruction::LoadFloatDouble {
            d: register,
            a: 1,
            offset,
        });
    }

    let old = std::mem::take(instructions);
    let old_len = old.len();
    let save_at = frame_push + 1;
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
    fn expands_a_linkage_first_frame_for_two_saved_fprs() {
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
                offset: -40,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 36,
            },
            Instruction::BranchAndLink {
                target: "call".into(),
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 40,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ];
        let permutation =
            materialize_linkage_first_frame(&mut instructions, &[31, 30]).unwrap();

        assert_eq!(permutation[3], 5);
        assert!(matches!(
            instructions[2],
            Instruction::StoreWordWithUpdate { offset: -56, .. }
        ));
        assert!(matches!(
            instructions[3],
            Instruction::StoreFloatDouble {
                s: 31,
                offset: 48,
                ..
            }
        ));
        assert!(matches!(
            instructions[4],
            Instruction::StoreFloatDouble {
                s: 30,
                offset: 40,
                ..
            }
        ));
        assert!(matches!(
            instructions[8],
            Instruction::LoadFloatDouble {
                d: 31,
                offset: 48,
                ..
            }
        ));
        assert!(matches!(
            instructions[10],
            Instruction::LoadWord {
                d: 31,
                offset: 36,
                ..
            }
        ));
    }
}
