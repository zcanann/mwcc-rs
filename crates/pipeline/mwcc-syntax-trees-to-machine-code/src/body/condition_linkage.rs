//! Scheduling of a non-leaf conditional's CR work around the saved-link-register
//! sequence. Shared by the single-arm and if/else body paths.

use super::*;
use mwcc_versions::FoldedFloatCompareLinkageStyle;

impl Generator {
    /// Insert the saved-LR store around a condition emitted immediately after
    /// `mflr r0`. `condition_start` is the first emitted condition instruction.
    pub(crate) fn schedule_condition_linkage(&mut self, condition_start: usize) {
        let linkage_position = condition_start
            .checked_sub(1)
            .expect("condition linkage requires a preceding mflr");
        debug_assert!(matches!(
            self.output.instructions.get(linkage_position),
            Some(Instruction::MoveFromLinkRegister { d: 0 })
        ));
        let folded_float_compare = matches!(
            self.output.instructions.get(condition_start),
            Some(Instruction::FloatCompareOrdered { .. })
        ) && matches!(
            self.output.instructions.get(condition_start + 1),
            Some(Instruction::ConditionRegisterOr { .. })
        );

        let store_position = if folded_float_compare
            && self.behavior.folded_float_compare_linkage_style
                == FoldedFloatCompareLinkageStyle::CompareFirst
        {
            // Build 163 starts the compare before touching LR, then uses mflr and
            // its store to separate fcmpo from the dependent cror.
            self.output
                .instructions
                .swap(linkage_position, condition_start);
            condition_start + 1
        } else {
            let first_writes_r0 = self
                .output
                .instructions
                .get(condition_start)
                .is_some_and(condition_instruction_writes_r0);
            let float_load_first = matches!(
                self.output.instructions.get(condition_start),
                Some(
                    Instruction::LoadFloatSingle { .. }
                        | Instruction::LoadFloatSingleIndexed { .. }
                        | Instruction::LoadFloatDouble { .. }
                        | Instruction::LoadFloatDoubleIndexed { .. }
                )
            );
            if first_writes_r0 || (self.behavior.lr_save_precedes_float_const && float_load_first) {
                condition_start
            } else {
                condition_start + 1
            }
        };

        self.output.instructions.insert(
            store_position,
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
        );
        // The insert shifts condition instructions at/after it down by one, so
        // their relocations (for example, a global's SDA21 load) move with them.
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index >= store_position {
                relocation.instruction_index += 1;
            }
        }
    }
}

/// Whether the first condition instruction destroys the saved return address in
/// r0 before it can be written to the linkage area.
fn condition_instruction_writes_r0(instruction: &Instruction) -> bool {
    match instruction {
        // Compares and float/CR operations write cr0 or an FPR, not a GPR.
        Instruction::CompareWord { .. }
        | Instruction::CompareWordImmediate { .. }
        | Instruction::CompareLogicalWord { .. }
        | Instruction::CompareLogicalWordImmediate { .. }
        | Instruction::FloatCompareOrdered { .. }
        | Instruction::FloatCompareUnordered { .. }
        | Instruction::LoadFloatSingle { .. }
        | Instruction::LoadFloatSingleIndexed { .. }
        | Instruction::LoadFloatDouble { .. }
        | Instruction::LoadFloatDoubleIndexed { .. }
        | Instruction::ConditionRegisterOr { .. } => false,
        // A narrow extension into a non-r0 GPR leaves the saved LR intact.
        Instruction::ExtendSignByte { a, .. }
        | Instruction::ExtendSignByteRecord { a, .. }
        | Instruction::ExtendSignHalfword { a, .. }
        | Instruction::ExtendSignHalfwordRecord { a, .. }
        | Instruction::ClearLeftImmediate { a, .. }
        | Instruction::ClearLeftImmediateRecord { a, .. } => *a == 0,
        // Other supported first instructions write a GPR.
        _ => true,
    }
}
