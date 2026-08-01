//! Reconcile canonical frame saves with the allocator's physical result.
//!
//! Structured selection declares logical saved homes before allocation.  A
//! home can subsequently color into a caller-saved register while another
//! temporary crosses a call and colors into a callee-saved register.  Canonical
//! individual-save frames can absorb that change when their existing aligned
//! frame has unused save-slot capacity; specialized and dense frames retain
//! their explicit owners. Linkage-first dense ranges may temporarily outgrow
//! the selected frame: the convention normalizer runs after allocation and
//! owns their final frame size.

use crate::generator::Generator;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationTarget};
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

        // A virtual declared home is only a logical slot request. Once colored,
        // its save must move ahead of the instruction that defines that home;
        // leaving the selection-time store in place would save the callee's new
        // value instead of the caller's register. Physical homes already own
        // canonical placement and need only the capacity check.
        let declared_has_late_virtual_save = declared.iter().enumerate().any(|(slot, home)| {
            if !matches!(Reg::from_field(*home, Class::General), Reg::Virtual(_)) {
                return false;
            }
            let offset = self.frame_size - 4 * (slot as i16 + 1);
            let save = self.output.instructions.iter().position(|instruction| {
                matches!(instruction,
                    Instruction::StoreWord { s, a: 1, offset: candidate }
                        if s == home && *candidate == offset)
            });
            let definition = self.output.instructions.iter().position(|instruction| {
                mwcc_vreg::register_operands(instruction).iter().any(|operand| {
                    operand.role == mwcc_vreg::RegisterRole::Define
                        && operand.class == Class::General
                        && operand.register == *home
                })
            });
            matches!((save, definition), (Some(save), Some(definition)) if save >= definition)
        });
        if required.len() <= declared.len() && !declared_has_late_virtual_save {
            return Ok(());
        }
        if self.frame_size == 0 || declared.is_empty() {
            return Err(Diagnostic::error(
                "allocated callee-saved values need a canonical frame owner",
            ));
        }

        // A linkage-first `stmw`/`lmw` range is self-describing. Repaint it to
        // the allocator's complete dense suffix now; the later convention
        // normalizer grows the frame and moves every save/local displacement.
        // Requiring the OLD frame to have room here prevents that normalizer
        // from ever seeing precisely the range it is designed to size.
        if self.behavior.frame_convention == mwcc_versions::FrameConvention::LinkageFirst
            && self.grow_dense_general_save_range(&declared, &required)?
        {
            return Ok(());
        }

        let local_end = self
            .frame_slots
            .values()
            .map(|slot| i32::from(slot.offset) + i32::try_from(slot.size).unwrap_or(i32::MAX))
            .max()
            .unwrap_or(8);
        let lowest_save =
            i32::from(self.frame_size) - 4 * i32::try_from(required.len()).unwrap_or(i32::MAX);
        if lowest_save < local_end
            && self.behavior.frame_convention
                != mwcc_versions::FrameConvention::LinkageFirst
        {
            return Err(Diagnostic::error(format!(
                "allocation needs {} callee-saved slots but the existing frame has capacity for {} \
                 (declared {declared:?}, required {required:?}, frame size {}, local end {local_end}; \
                 frame growth needed)",
                required.len(),
                declared.len(),
                self.frame_size,
            )));
        }
        if self.grow_dense_general_save_range(&declared, &required)? {
            return Ok(());
        }

        let nonreturning = !self
            .output
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::BranchToLinkRegister));
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
            let [save] = saves.as_slice() else {
                return Err(Diagnostic::error(
                    format!(
                        "allocated callee-saved values need canonical individual save/restore slots \
                         (declared {declared:?}, required {required:?})"
                    ),
                ));
            };
            save_indices.push(*save);
            match restores.as_slice() {
                [restore] => restore_indices.push(*restore),
                [] if nonreturning => {}
                _ => {
                    return Err(Diagnostic::error(format!(
                        "allocated callee-saved values need canonical individual save/restore slots \
                         (declared {declared:?}, required {required:?})"
                    )))
                }
            }
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

        if nonreturning {
            self.callee_saved = required;
            return Ok(());
        }

        let restored_stack_link_reload = self.behavior.saved_gpr_epilogue_style
            == mwcc_versions::SavedGprEpilogueStyle::StackRestoreBeforeLinkRegisterReload;
        let restore_insertion = if restored_stack_link_reload {
            let stack_restore = self
                .output
                .instructions
                .iter()
                .rposition(|instruction| {
                    matches!(instruction,
                        Instruction::AddImmediate { d: 1, a: 1, immediate }
                            if *immediate == self.frame_size)
                })
                .ok_or_else(|| {
                    Diagnostic::error("canonical frame is missing its stack restore")
                })?;
            let has_link_reload = self.output.instructions[stack_restore + 1..]
                .iter()
                .any(|instruction| {
                    matches!(instruction, Instruction::LoadWord { d: 0, a: 1, offset: 4 })
                });
            if !has_link_reload {
                return Err(Diagnostic::error(
                    "canonical frame is missing its restored-stack LR reload",
                ));
            }
            stack_restore
        } else {
            self.output
                .instructions
                .iter()
                .rposition(|instruction| {
                    matches!(
                        instruction,
                        Instruction::LoadWord { d: 0, a: 1, offset }
                            if *offset == self.frame_size + 4
                    )
                })
                .map(|lr_restore| lr_restore + 1)
                .ok_or_else(|| Diagnostic::error("canonical frame is missing its LR restore"))?
        };
        for (slot, register) in required.iter().copied().enumerate() {
            self.insert_frame_instruction(
                restore_insertion + slot,
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

    /// Grow a dense rN..r31 save/restore pair after allocation discovers one
    /// more call-crossing value than selection predicted. Both MWCC dense forms
    /// are supported: inline `stmw`/`lmw` and `_savegpr_N`/`_restgpr_N`.
    fn grow_dense_general_save_range(
        &mut self,
        declared: &[u8],
        required: &[u8],
    ) -> Compilation<bool> {
        let Some(new_first) = dense_suffix_first(required) else {
            return Ok(false);
        };
        let old_first = 32u8
            .checked_sub(u8::try_from(declared.len()).map_err(|_| {
                Diagnostic::error("dense callee-saved range is too large")
            })?)
            .ok_or_else(|| Diagnostic::error("dense callee-saved range is too large"))?;
        if new_first >= old_first {
            return Ok(false);
        }

        let stores: Vec<_> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(
                    instruction,
                    Instruction::StoreMultipleWord { s, a: 1, .. } if *s == old_first
                )
                .then_some(index)
            })
            .collect();
        let loads: Vec<_> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(
                    instruction,
                    Instruction::LoadMultipleWord { d, a: 1, .. } if *d == old_first
                )
                .then_some(index)
            })
            .collect();
        if let ([store], [load]) = (stores.as_slice(), loads.as_slice()) {
            let offset = self.frame_size
                - 4 * i16::try_from(required.len())
                    .map_err(|_| Diagnostic::error("dense callee-saved range is too large"))?;
            let Instruction::StoreMultipleWord { s, offset: at, .. } =
                &mut self.output.instructions[*store]
            else {
                unreachable!("dense save was classified above")
            };
            *s = new_first;
            *at = offset;
            let Instruction::LoadMultipleWord { d, offset: at, .. } =
                &mut self.output.instructions[*load]
            else {
                unreachable!("dense restore was classified above")
            };
            *d = new_first;
            *at = offset;
            self.callee_saved = required.to_vec();
            return Ok(true);
        }

        let old_save = format!("_savegpr_{old_first}");
        let old_restore = format!("_restgpr_{old_first}");
        let new_save = format!("_savegpr_{new_first}");
        let new_restore = format!("_restgpr_{new_first}");
        let save_calls: Vec<_> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(
                    instruction,
                    Instruction::BranchAndLink { target } if target == &old_save
                )
                .then_some(index)
            })
            .collect();
        let restore_calls: Vec<_> = self
            .output
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(index, instruction)| {
                matches!(
                    instruction,
                    Instruction::BranchAndLink { target } if target == &old_restore
                )
                .then_some(index)
            })
            .collect();
        let ([save], [restore]) = (save_calls.as_slice(), restore_calls.as_slice()) else {
            return Ok(false);
        };
        for (index, old, new) in [
            (*save, old_save.as_str(), new_save.as_str()),
            (*restore, old_restore.as_str(), new_restore.as_str()),
        ] {
            let Instruction::BranchAndLink { target } = &mut self.output.instructions[index] else {
                unreachable!("dense helper call was classified above")
            };
            *target = new.to_string();
            for relocation in &mut self.output.relocations {
                if relocation.instruction_index == index
                    && matches!(&relocation.target, RelocationTarget::External(name) if name == old)
                {
                    relocation.target = RelocationTarget::External(new.to_string());
                }
            }
        }
        self.callee_saved = required.to_vec();
        Ok(true)
    }

    fn insert_frame_instruction(&mut self, position: usize, instruction: Instruction) {
        crate::insert_instruction_retargeting(self, position, instruction);
    }

    fn remove_frame_instruction(&mut self, position: usize) {
        crate::remove_instruction_retargeting_to_next(self, position);
    }
}

fn dense_suffix_first(registers: &[u8]) -> Option<u8> {
    let &first = registers.last()?;
    let expected: Vec<_> = (first..=31).rev().collect();
    (registers == expected).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::dense_suffix_first;

    #[test]
    fn dense_suffix_requires_every_register_through_r31() {
        assert_eq!(dense_suffix_first(&[31, 30, 29, 28, 27, 26, 25]), Some(25));
        assert_eq!(dense_suffix_first(&[31, 30, 28]), None);
        assert_eq!(dense_suffix_first(&[]), None);
    }
}
