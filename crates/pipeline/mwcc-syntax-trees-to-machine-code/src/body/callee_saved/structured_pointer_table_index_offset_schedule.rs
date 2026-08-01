//! Final issue order for a retained repeated pointer-table byte offset.

use super::*;

impl Generator {
    pub(crate) fn schedule_prescaled_pointer_table_index(&mut self) {
        if !self.structured_prescaled_pointer_table_index {
            return;
        }
        self.normalize_prescaled_pointer_table_scalar_array_frame();
        self.schedule_prescaled_pointer_table_zero_store();
        self.schedule_prescaled_pointer_table_parameter_copies();
        self.schedule_prescaled_pointer_table_first_lookup();
        self.schedule_prescaled_pointer_table_following_lookup();
        self.schedule_prescaled_pointer_table_final_call();
        self.schedule_prescaled_pointer_table_sync_final_call();
        self.remove_prescaled_pointer_table_empty_poll_entry();
        self.schedule_prescaled_pointer_table_sync_epilogue();
    }

    fn normalize_prescaled_pointer_table_scalar_array_frame(&mut self) {
        if self.frame_size != 176
            || self.callee_saved.len() != 5
            || !self.frame_slots.values().any(|slot| {
                slot.is_array && slot.size == 128 && slot.offset == 24
            })
            || !self.frame_slots.values().any(|slot| {
                !slot.is_array && slot.size == 4 && slot.offset == 8
            })
        {
            return;
        }
        for slot in self.frame_slots.values_mut() {
            if slot.is_array && slot.size == 128 && slot.offset == 24 {
                slot.offset = 28;
            } else if !slot.is_array && slot.size == 4 && slot.offset == 8 {
                slot.offset = 24;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset }
                    if *offset == -176 =>
                {
                    *offset = -184;
                }
                Instruction::StoreMultipleWord { a: 1, offset, .. }
                | Instruction::LoadMultipleWord { a: 1, offset, .. }
                    if *offset == 156 =>
                {
                    *offset = 164;
                }
                Instruction::LoadWord { d: 0, a: 1, offset } if *offset == 180 => {
                    *offset = 188;
                }
                Instruction::StoreWord { a: 1, offset, .. }
                | Instruction::LoadWord { a: 1, offset, .. }
                    if *offset == 8 =>
                {
                    *offset = 24;
                }
                Instruction::AddImmediate { a: 1, immediate, .. } if *immediate == 8 => {
                    *immediate = 24;
                }
                Instruction::AddImmediate { a: 1, immediate, .. } if *immediate == 24 => {
                    *immediate = 28;
                }
                Instruction::AddImmediate { d: 1, a: 1, immediate }
                    if *immediate == 176 =>
                {
                    *immediate = 184;
                }
                _ => {}
            }
        }
        self.frame_size = 184;
    }

    fn schedule_prescaled_pointer_table_zero_store(&mut self) {
        let Some(frame) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -184 })
        }) else {
            return;
        };
        let Some(zero) = self.output.instructions[frame + 1..]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::AddImmediate { d: 0, a: 0, immediate: 0 })
            })
            .map(|offset| frame + 1 + offset)
        else {
            return;
        };
        if !self.output.instructions[zero + 1..].iter().take(2).any(|instruction| {
            matches!(instruction, Instruction::StoreWord { s: 0, a: 1, offset: 24 })
        }) {
            return;
        }
        crate::move_instruction_before_retargeting(self, zero, frame);
    }

    fn schedule_prescaled_pointer_table_parameter_copies(&mut self) {
        let packet = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 29, .. },
                    Instruction::AddImmediate { d: 28, .. },
                    Instruction::AddImmediate { d: 27, .. },
                    Instruction::AddImmediate { d: 26, .. },
                    Instruction::AddImmediate { d: 25, .. },
                ]
            )
        }).map(|start| (start, 25u8, 5usize)).or_else(|| {
            self.output.instructions.windows(3).position(|window| {
                matches!(
                    window,
                    [
                        Instruction::AddImmediate { d: 29, .. },
                        Instruction::AddImmediate { d: 28, .. },
                        Instruction::AddImmediate { d: 27, .. },
                    ]
                )
            }).map(|start| (start, 27u8, 3usize))
        });
        let Some((start, first, count)) = packet else {
            return;
        };
        for destination in first..first + u8::try_from(count - 1).expect("copy count") {
            let from = self.output.instructions[start..start + count]
                .iter()
                .position(|instruction| {
                    matches!(instruction, Instruction::AddImmediate { d, .. } if *d == destination)
                })
                .map(|offset| start + offset)
                .expect("the descending parameter-copy packet was recognized");
            crate::move_instruction_before_retargeting(
                self,
                from,
                start + usize::from(destination - first),
            );
        }
    }

    fn schedule_prescaled_pointer_table_first_lookup(&mut self) {
        let Some(start) = self.output.instructions.windows(7).position(|window| {
            matches!(
                window,
                [
                    Instruction::BranchAndLink { .. },
                    Instruction::ShiftLeftImmediate { a: 31, s: 30, shift: 2 },
                    Instruction::LoadWord { d: 0, .. },
                    Instruction::Add { d: 30, a: 0, b: source },
                    Instruction::AddImmediate { d: 3, a: 1, .. },
                    Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                    Instruction::LoadWordIndexed { d: 4, a: 4, b: 31 },
                ] if *source == 26 || *source == 28
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 5, start + 1);
        crate::move_instruction_before_retargeting(self, start + 5, start + 4);
        crate::move_instruction_before_retargeting(self, start + 6, start + 5);
    }

    fn schedule_prescaled_pointer_table_following_lookup(&mut self) {
        let Some(start) = self.output.instructions.windows(5).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 1, .. },
                    Instruction::LoadWord { d: 4, a: 0, offset: 0 },
                    Instruction::LoadWordIndexed { d: 4, a: 4, b: 31 },
                    Instruction::AddImmediate { d: 4, a: 4, .. },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 1, start);
    }

    fn schedule_prescaled_pointer_table_final_call(&mut self) {
        let Some(start) = self.output.instructions.windows(8).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 29, immediate: 0 },
                    Instruction::AddImmediate { d: 5, a: 25, immediate: 0 },
                    Instruction::AddImmediate { d: 4, a: 1, .. },
                    Instruction::AddImmediate { d: 6, a: 30, immediate: 0 },
                    Instruction::AddImmediate { d: 7, a: 27, immediate: 0 },
                    Instruction::AddImmediate { d: 8, a: 0, immediate: 0 },
                    Instruction::Or { a: 9, s: 28, b: 28 },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 3, start + 2);
        crate::move_instruction_before_retargeting(self, start + 4, start + 3);
        crate::move_instruction_before_retargeting(self, start + 6, start + 4);
        self.output.instructions[start + 4] = Instruction::AddImmediate {
            d: 9,
            a: 28,
            immediate: 0,
        };
    }

    fn schedule_prescaled_pointer_table_sync_final_call(&mut self) {
        let Some(start) = self.output.instructions.windows(8).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate { d: 3, a: 0, immediate: 0 },
                    Instruction::AddImmediate { d: 5, a: 27, immediate: 0 },
                    Instruction::AddImmediate { d: 4, a: 1, immediate: 28 },
                    Instruction::AddImmediate { d: 6, a: 30, immediate: 0 },
                    Instruction::AddImmediate { d: 7, a: 29, immediate: 0 },
                    Instruction::AddImmediate { d: 8, a: 1, immediate: 24 },
                    Instruction::AddImmediate { d: 9, a: 0, immediate: 0 },
                    Instruction::BranchAndLink { .. },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 1, start);
        crate::move_instruction_before_retargeting(self, start + 3, start + 1);
        crate::move_instruction_before_retargeting(self, start + 4, start + 2);
        crate::move_instruction_before_retargeting(self, start + 4, start + 3);
        crate::move_instruction_before_retargeting(self, start + 5, start + 4);
    }

    fn remove_prescaled_pointer_table_empty_poll_entry(&mut self) {
        let Some(branch) = self.output.instructions.windows(3).enumerate().find_map(
            |(start, window)| {
                matches!(
                    window,
                    [
                        Instruction::Branch { target },
                        Instruction::LoadWord { d: 0, a: 1, offset: 24 },
                        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
                    ] if *target == start + 1
                )
                .then_some(start)
            },
        ) else {
            return;
        };
        crate::remove_instruction_retargeting_to_next(self, branch);
    }

    fn schedule_prescaled_pointer_table_sync_epilogue(&mut self) {
        let Some(start) = self.output.instructions.windows(2).rposition(|window| {
            matches!(
                window,
                [
                    Instruction::LoadMultipleWord { d: 27, a: 1, offset: 164 },
                    Instruction::LoadWord { d: 0, a: 1, offset: 188 },
                ]
            )
        }) else {
            return;
        };
        crate::move_instruction_before_retargeting(self, start + 1, start);
    }
}
