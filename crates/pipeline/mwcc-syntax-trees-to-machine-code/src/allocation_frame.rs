//! Reconcile canonical frame saves with the allocator's physical result.
//!
//! Structured selection declares logical saved homes before allocation.  A
//! home can subsequently color into a caller-saved register while another
//! temporary crosses a call and colors into a callee-saved register.  Canonical
//! individual-save frames can absorb that change when their existing aligned
//! frame has unused save-slot capacity; specialized and dense frames retain
//! their explicit owners.

use crate::generator::Generator;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_vreg::{Allocation, Class, Reg};

impl Generator {
    pub(crate) fn reconcile_allocated_general_frame(
        &mut self,
        allocation: &Allocation,
        allocated_callee_saved: &[u8],
    ) -> Compilation<()> {
        let declared = self.callee_saved.clone();
        let mut required = allocated_callee_saved.to_vec();
        for home in &declared {
            let physical = match Reg::from_field(*home, Class::General) {
                Reg::Physical(register) => Some(register),
                Reg::Virtual(register) => allocation.physical(register),
            };
            if let Some(register) = physical {
                if self.constraints.general_callee_saved.contains(&register) {
                    required.push(register);
                }
            }
        }
        required.sort_unstable_by(|left, right| right.cmp(left));
        required.dedup();

        if required.len() <= declared.len() {
            return Ok(());
        }
        if self.frame_size == 0 || declared.is_empty() {
            return Err(Diagnostic::error(
                "allocated callee-saved values need a canonical frame owner",
            ));
        }

        let local_end = self
            .frame_slots
            .values()
            .map(|slot| i32::from(slot.offset) + i32::from(slot.size))
            .max()
            .unwrap_or(8);
        let lowest_save =
            i32::from(self.frame_size) - 4 * i32::try_from(required.len()).unwrap_or(i32::MAX);
        if lowest_save < local_end {
            return Err(Diagnostic::error(format!(
                "allocation needs {} callee-saved slots but the existing frame has capacity for {} (frame growth needed)",
                required.len(),
                declared.len()
            )));
        }

        let mut save_indices = Vec::with_capacity(declared.len());
        let mut restore_indices = Vec::with_capacity(declared.len());
        for (slot, home) in declared.iter().copied().enumerate() {
            let offset = self.frame_size - 4 * (slot as i16 + 1);
            let saves: Vec<_> = self
                .output
                .instructions
                .iter()
                .enumerate()
                .filter_map(|(index, instruction)| {
                    matches!(
                        instruction,
                        Instruction::StoreWord { s, a: 1, offset: candidate }
                            if *s == home && *candidate == offset
                    )
                    .then_some(index)
                })
                .collect();
            let restores: Vec<_> = self
                .output
                .instructions
                .iter()
                .enumerate()
                .filter_map(|(index, instruction)| {
                    matches!(
                        instruction,
                        Instruction::LoadWord { d, a: 1, offset: candidate }
                            if *d == home && *candidate == offset
                    )
                    .then_some(index)
                })
                .collect();
            let ([save], [restore]) = (saves.as_slice(), restores.as_slice()) else {
                return Err(Diagnostic::error(
                    "allocated callee-saved values need canonical individual save/restore slots",
                ));
            };
            save_indices.push(*save);
            restore_indices.push(*restore);
        }

        // Logical-home saves may be interleaved with the moves that define
        // those homes.  Repainting such a slot with a different physical
        // register could save it after it was already overwritten.  Remove the
        // logical slots and rebuild one canonical physical range around the LR
        // linkage instead.
        let mut removed = save_indices;
        removed.extend(restore_indices);
        removed.sort_unstable();
        for index in removed.into_iter().rev() {
            self.remove_frame_instruction(index);
        }

        let lr_store = self
            .output
            .instructions
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreWord { s: 0, a: 1, offset }
                        if *offset == self.frame_size + 4
                )
            })
            .ok_or_else(|| Diagnostic::error("canonical frame is missing its LR save"))?;
        for (slot, register) in required.iter().copied().enumerate() {
            self.insert_frame_instruction(
                lr_store + 1 + slot,
                Instruction::StoreWord {
                    s: register,
                    a: 1,
                    offset: self.frame_size - 4 * (slot as i16 + 1),
                },
            );
        }

        let lr_restore = self
            .output
            .instructions
            .iter()
            .rposition(|instruction| {
                matches!(
                    instruction,
                    Instruction::LoadWord { d: 0, a: 1, offset }
                        if *offset == self.frame_size + 4
                )
            })
            .ok_or_else(|| Diagnostic::error("canonical frame is missing its LR restore"))?;
        for (slot, register) in required.iter().copied().enumerate() {
            self.insert_frame_instruction(
                lr_restore + 1 + slot,
                Instruction::LoadWord {
                    d: register,
                    a: 1,
                    offset: self.frame_size - 4 * (slot as i16 + 1),
                },
            );
        }
        self.callee_saved = required;
        Ok(())
    }

    fn insert_frame_instruction(&mut self, position: usize, instruction: Instruction) {
        self.output.instructions.insert(position, instruction);
        self.labels.inserted(position, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= position {
                relocation.instruction_index += 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target >= position =>
                {
                    *target += 1;
                }
                _ => {}
            }
        }
    }

    fn remove_frame_instruction(&mut self, position: usize) {
        self.output.instructions.remove(position);
        self.labels.removed_retargeting_to_next(position, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != position);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > position {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target } => {
                    if *target > position {
                        *target -= 1;
                    }
                }
                _ => {}
            }
        }
    }
}
