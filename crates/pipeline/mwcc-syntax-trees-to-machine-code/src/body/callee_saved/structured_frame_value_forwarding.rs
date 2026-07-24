//! Straight-line forwarding for published frame-scalar values.
//!
//! An address-taken scalar must be written to its stack slot even when its
//! assigned value is still in a register: a later `&value` call observes that
//! memory.  The publication does not kill the register value, though.  MWCC
//! therefore omits an immediately following reload into the same register.
//! Keep this deliberately narrow; forwarding across another instruction needs
//! alias and clobber analysis owned by a future frame-value data-flow pass.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn forward_adjacent_frame_scalar_values(
        &mut self,
        safe_offsets: &std::collections::HashSet<i16>,
    ) {
        while let Some(index) = adjacent_forwarded_load(&self.output.instructions, safe_offsets) {
            self.output.instructions.remove(index);
            let old_len = self.output.instructions.len() + 1;
            let permutation: Vec<usize> = (0..old_len)
                .map(|old| {
                    if old < index {
                        old
                    } else if old == index {
                        index.saturating_sub(1)
                    } else {
                        old - 1
                    }
                })
                .collect();
            crate::remap_instruction_indices(self, &permutation);
        }
    }
}

fn adjacent_forwarded_load(
    instructions: &[Instruction],
    safe_offsets: &std::collections::HashSet<i16>,
) -> Option<usize> {
    instructions
        .windows(2)
        .enumerate()
        .find(|(store, window)| {
            let load = store + 1;
            let is_branch_target = instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::BranchConditionalForward { target, .. }
                        | Instruction::Branch { target }
                        if *target == load
                )
            });
            !is_branch_target
                && matches!(
                    (&window[0], &window[1]),
                    (
                        Instruction::StoreWord {
                            s,
                            a: 1,
                            offset: store_offset,
                        },
                        Instruction::LoadWord {
                            d,
                            a: 1,
                            offset: load_offset,
                        },
                    ) if s == d
                        && store_offset == load_offset
                        && safe_offsets.contains(store_offset)
                )
        })
        .map(|(store, _)| store + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_same_register_reload_of_published_scalar() {
        let instructions = vec![
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 8,
            },
        ];
        assert_eq!(
            adjacent_forwarded_load(&instructions, &std::collections::HashSet::from([8])),
            Some(1)
        );
    }

    #[test]
    fn preserves_different_register_and_unapproved_slot_loads() {
        let different_register = vec![
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 4,
                a: 1,
                offset: 8,
            },
        ];
        assert_eq!(
            adjacent_forwarded_load(&different_register, &std::collections::HashSet::from([8])),
            None
        );

        let same_register = vec![
            Instruction::StoreWord {
                s: 3,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 8,
            },
        ];
        assert_eq!(
            adjacent_forwarded_load(&same_register, &std::collections::HashSet::new()),
            None
        );
    }
}
