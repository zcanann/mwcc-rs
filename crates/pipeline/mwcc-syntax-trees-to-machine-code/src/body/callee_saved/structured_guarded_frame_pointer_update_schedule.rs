//! Retain a frame-held pointer across an inlined guarded member update.
//!
//! Selection treats each use of an address-taken pointer as an independent
//! frame reload.  In a call-free false arm such as a bounded append, MWCC keeps
//! that pointer live from the guard through the cursor and length updates.  The
//! schedule below recognizes the complete guarded transaction before removing
//! any reload, so neither a call nor a write to the authoritative frame slot can
//! be crossed accidentally.

#[allow(unused_imports)]
use super::*;

mod recognize;
use recognize::{direct_call_result_zero_test, guarded_frame_pointer_update};

impl Generator {
    pub(crate) fn schedule_structured_guarded_frame_pointer_updates(&mut self) {
        let mut scheduled = false;
        while let Some(plan) = guarded_frame_pointer_update(&self.output.instructions) {
            scheduled = true;
            let pointer = plan.old_cursor;
            let position = plan.initial_pointer;

            let Instruction::LoadWord { d, .. } = &mut self.output.instructions[plan.start] else {
                unreachable!("validated frame-pointer load changed form")
            };
            *d = pointer;
            let Instruction::LoadWord { d, a, .. } =
                &mut self.output.instructions[plan.start + 1]
            else {
                unreachable!("validated cursor load changed form")
            };
            *d = position;
            *a = pointer;
            let Instruction::CompareLogicalWordImmediate { a, .. } =
                &mut self.output.instructions[plan.start + 2]
            else {
                unreachable!("validated cursor guard changed form")
            };
            *a = position;
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[plan.start + 4]
            else {
                unreachable!("validated overflow result changed form")
            };
            *d = 4;

            let Instruction::AddImmediate { d, a, .. } =
                &mut self.output.instructions[plan.start + 9]
            else {
                unreachable!("validated cursor increment changed form")
            };
            *d = plan.scratch;
            *a = position;
            let Instruction::StoreWord { s, a, .. } =
                &mut self.output.instructions[plan.start + 10]
            else {
                unreachable!("validated cursor store changed form")
            };
            *s = plan.scratch;
            *a = pointer;
            let Instruction::Add { d, a, b } =
                &mut self.output.instructions[plan.start + 12]
            else {
                unreachable!("validated append address changed form")
            };
            *d = position;
            *a = pointer;
            *b = position;
            let Instruction::StoreByte { a, .. } =
                &mut self.output.instructions[plan.start + 13]
            else {
                unreachable!("validated append store changed form")
            };
            *a = position;

            let Instruction::LoadWord { d, a, .. } =
                &mut self.output.instructions[plan.start + 16]
            else {
                unreachable!("validated length load changed form")
            };
            *d = position;
            *a = pointer;
            let Instruction::AddImmediate { d, a, .. } =
                &mut self.output.instructions[plan.start + 17]
            else {
                unreachable!("validated length increment changed form")
            };
            *d = plan.scratch;
            *a = position;
            let Instruction::StoreWord { s, a, .. } =
                &mut self.output.instructions[plan.start + 18]
            else {
                unreachable!("validated length store changed form")
            };
            *s = plan.scratch;
            *a = pointer;
            let Instruction::AddImmediate { d, .. } =
                &mut self.output.instructions[plan.start + 19]
            else {
                unreachable!("validated success result changed form")
            };
            *d = 4;
            let Instruction::CompareWordImmediate { a, .. } =
                &mut self.output.instructions[plan.start + 20]
            else {
                unreachable!("validated result join changed form")
            };
            *a = 4;

            // MWCC issues the derived byte address before publishing the
            // incremented cursor.  Move the address calculation rather than
            // rebuilding the region so resolved labels and relocations remain
            // authoritative.
            crate::move_instruction_before_retargeting(
                self,
                plan.start + 12,
                plan.start + 10,
            );
            for relative in [15usize, 14, 12, 8, 7, 6] {
                crate::remove_instruction_retargeting_to_next(self, plan.start + relative);
            }
            crate::move_instruction_before_retargeting(self, plan.start + 13, plan.start + 9);
        }
        if scheduled {
            self.fold_guarded_frame_call_result_zero_tests();
            self.schedule_guarded_frame_linkage_edges();
        }
    }

    fn fold_guarded_frame_call_result_zero_tests(&mut self) {
        while let Some((copy, saved)) = direct_call_result_zero_test(&self.output.instructions) {
            self.output.instructions[copy] = Instruction::OrRecord {
                a: saved,
                s: Eabi::FIRST_GENERAL_ARGUMENT,
                b: Eabi::FIRST_GENERAL_ARGUMENT,
            };
            crate::remove_instruction_retargeting_to_next(self, copy + 1);
        }
    }

    fn schedule_guarded_frame_linkage_edges(&mut self) {
        if self.behavior.plain_linkage_epilogue_style
            != mwcc_versions::PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !matches!(&self.output.instructions[..8], [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 },
                Instruction::StoreWord { s: 31, a: 1, offset: 28 },
                Instruction::StoreWord { s: 30, a: 1, offset: 24 },
                Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
                Instruction::AddImmediate { d: 3, a: 1, immediate: 8 },
                Instruction::AddImmediate { d: 4, a: 1, immediate: 12 },
            ])
        {
            return;
        }
        crate::move_instruction_before_retargeting(self, 7, 4);

        let end = self.output.instructions.len();
        if matches!(&self.output.instructions[end - 7..], [
            Instruction::LoadWord { d: 0, a: 1, offset: 36 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord { d: 31, a: 1, offset: 28 },
            Instruction::LoadWord { d: 30, a: 1, offset: 24 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
            Instruction::BranchToLinkRegister,
        ]) {
            self.output.instructions[end - 7..].clone_from_slice(&[
                Instruction::Or { a: 3, s: 31, b: 31 },
                Instruction::LoadWord { d: 31, a: 1, offset: 28 },
                Instruction::LoadWord { d: 30, a: 1, offset: 24 },
                Instruction::AddImmediate { d: 1, a: 1, immediate: 32 },
                Instruction::LoadWord { d: 0, a: 1, offset: 4 },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]);
        }
    }
}
