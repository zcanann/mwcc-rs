//! ABI frame materialization for allocator-selected callee-saved FPRs.

#[allow(unused_imports)]
use super::*;
use super::allocated_float_frame_linkage_first::materialize_linkage_first_frame;

impl Generator {
    /// Expand an already scheduled non-leaf frame around the FPRs selected by
    /// register allocation. GameCube builds preserve only the scalar double
    /// lane; Wii additionally preserves the paired-single half.
    pub(crate) fn materialize_allocated_float_frame(
        &mut self,
        registers: &[u8],
        paired_single_frame: bool,
    ) -> Compilation<()> {
        let registers = required_float_save_range(self.callee_saved_float, registers)
            .map_err(Diagnostic::error)?;
        if registers.is_empty() {
            return Ok(());
        }
        let saved_gpr_count = self.callee_saved.len();
        let (permutation, frame_growth) = match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                materialize_predecrement_frame(
                    &mut self.output.instructions,
                    &registers,
                    saved_gpr_count,
                    paired_single_frame,
                )
                .map_err(Diagnostic::error)?
            }
            FrameConvention::LinkageFirst => {
                let permutation =
                    materialize_linkage_first_frame(&mut self.output.instructions, &registers)
                        .map_err(Diagnostic::error)?;
                let frame_growth = i16::try_from(registers.len())
                    .ok()
                    .and_then(|count| count.checked_mul(8))
                    .ok_or_else(|| Diagnostic::error("allocated FPR frame is too large"))?;
                (permutation, frame_growth)
            }
        };
        crate::remap_instruction_indices(self, &permutation);
        let count = u8::try_from(registers.len())
            .map_err(|_| Diagnostic::error("too many allocator-selected FPR saves"))?;
        self.callee_saved_float = count;
        self.frame_size = self
            .frame_size
            .checked_add(frame_growth)
            .ok_or_else(|| Diagnostic::error("allocated FPR frame is too large"))?;
        Ok(())
    }
}

/// Resolve the ABI save range from selection's declared saved-float homes and
/// allocation's physical subset. Structured owners know how many source-level
/// values survive calls before coloring; allocator reuse may report only the
/// registers occupied by virtual intervals, so that subset cannot define the
/// frame on its own.
fn required_float_save_range(
    declared_count: u8,
    allocated: &[u8],
) -> Result<Vec<u8>, String> {
    if allocated.is_empty() || declared_count == 0 {
        return Ok(allocated.to_vec());
    }
    let declared: Vec<u8> = (0..declared_count)
        .map(|index| 31u8.saturating_sub(index))
        .collect();
    if allocated.iter().any(|register| !declared.contains(register)) {
        return Err(format!(
            "allocator-selected FPRs {allocated:?} lie outside declared save range {declared:?}"
        ));
    }
    Ok(declared)
}

fn materialize_predecrement_frame(
    instructions: &mut Vec<Instruction>,
    registers: &[u8],
    saved_gpr_count: usize,
    paired_single_frame: bool,
) -> Result<(Vec<usize>, i16), &'static str> {
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
    let payload_bytes = i16::try_from(registers.len())
        .ok()
        .and_then(|count| count.checked_mul(if paired_single_frame { 16 } else { 8 }))
        .ok_or("allocated FPR frame is too large")?;
    let unaligned_size = old_size
        .checked_add(payload_bytes)
        .ok_or("allocated FPR frame is too large")?;
    let new_size = if paired_single_frame {
        unaligned_size
    } else {
        unaligned_size
            .checked_add(15)
            .map(|size| size & !15)
            .ok_or("allocated FPR frame is too large")?
    };
    let frame_growth = new_size - old_size;
    let compact_padding = frame_growth - payload_bytes;
    let link_offset = old_size
        .checked_add(4)
        .ok_or("allocated FPR link slot is out of range")?;
    let new_link_offset = new_size
        .checked_add(4)
        .ok_or("allocated FPR link slot is out of range")?;
    let saved_gpr_offsets: Vec<i16> = (0..saved_gpr_count)
        .map(|slot| old_size - 4 * (slot as i16 + 1))
        .collect();

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
            Instruction::StoreWord { a: 1, offset, .. }
                if !paired_single_frame && saved_gpr_offsets.contains(offset) =>
            {
                *offset = offset
                    .checked_add(compact_padding)
                    .ok_or("allocated GPR save offset is out of range")?;
            }
            Instruction::LoadWord { a: 1, offset, .. }
                if !paired_single_frame && saved_gpr_offsets.contains(offset) =>
            {
                *offset = offset
                    .checked_add(compact_padding)
                    .ok_or("allocated GPR restore offset is out of range")?;
            }
            _ => {}
        }
    }

    let mut saves = Vec::with_capacity(
        registers.len() * if paired_single_frame { 2 } else { 1 },
    );
    let mut restores = Vec::with_capacity(
        registers.len() * if paired_single_frame { 3 } else { 1 },
    );
    for (index, register) in registers.iter().copied().enumerate() {
        let lane_bytes = if paired_single_frame { 16 } else { 8 };
        let double_offset = new_size - lane_bytes * (index as i16 + 1);
        saves.push(Instruction::StoreFloatDouble {
            s: register,
            a: 1,
            offset: double_offset,
        });
        if paired_single_frame {
            let paired_offset = double_offset + 8;
            saves.push(Instruction::PairedSingleQuantizedStore {
                s: register,
                a: 1,
                offset: paired_offset,
                w: 0,
                i: 0,
            });
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
            restores.push(Instruction::LoadFloatDouble {
                d: register,
                a: 1,
                offset: double_offset,
            });
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
    Ok((permutation, frame_growth))
}

#[cfg(test)]
mod declared_range_tests {
    use super::required_float_save_range;

    #[test]
    fn expands_an_allocator_subset_to_the_declared_contiguous_range() {
        assert_eq!(
            required_float_save_range(7, &[31, 30, 27]).expect("declared range"),
            vec![31, 30, 29, 28, 27, 26, 25]
        );
    }

    #[test]
    fn rejects_an_allocation_outside_the_declared_range() {
        assert!(required_float_save_range(2, &[31, 29]).is_err());
    }
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
        let (permutation, frame_growth) =
            materialize_predecrement_frame(&mut instructions, &[31, 30], 1, true).unwrap();

        assert_eq!(frame_growth, 32);
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

    #[test]
    fn expands_a_gamecube_predecrement_frame_with_compact_fpr_lanes() {
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
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 12,
            },
            Instruction::move_register(31, 3),
            Instruction::FloatMove { d: 31, b: 1 },
            Instruction::BranchAndLink {
                target: "call".into(),
            },
            Instruction::StoreFloatSingle {
                s: 31,
                a: 31,
                offset: 0,
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
        let (_, frame_growth) =
            materialize_predecrement_frame(&mut instructions, &[31], 1, false).unwrap();

        assert_eq!(frame_growth, 16);
        assert!(matches!(
            instructions.as_slice(),
            [
                Instruction::StoreWordWithUpdate { offset: -32, .. },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, offset: 36, .. },
                Instruction::StoreFloatDouble {
                    s: 31,
                    offset: 24,
                    ..
                },
                Instruction::StoreWord {
                    s: 31,
                    offset: 20,
                    ..
                },
                Instruction::Or { a: 31, s: 3, b: 3 },
                Instruction::FloatMove { d: 31, b: 1 },
                Instruction::BranchAndLink { .. },
                Instruction::StoreFloatSingle { .. },
                Instruction::LoadFloatDouble {
                    d: 31,
                    offset: 24,
                    ..
                },
                Instruction::LoadWord {
                    d: 31,
                    offset: 20,
                    ..
                },
                Instruction::LoadWord { d: 0, offset: 36, .. },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate { immediate: 32, .. },
                Instruction::BranchToLinkRegister,
            ]
        ));
    }

}
