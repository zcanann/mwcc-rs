//! Argument preparation scheduled into non-leaf prologue latency slots.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fill the non-leaf prologue's linkage-write latency with leading register-
    /// ALU argument setup that is ready at function entry.
    ///
    /// A register move/derivation qualifies; a memory load and anything touching
    /// r0 do not. Mainline places at most two instructions before the LR store.
    /// Build 163 places the first before that store and the second between the
    /// store and `stwu`, preserving argument order while filling both writes.
    pub(crate) fn hoist_leading_arg_moves(&mut self, lr_store_index: Option<usize>) {
        let Some(store) = lr_store_index else { return };
        let linkage_first = self.behavior.frame_convention == FrameConvention::LinkageFirst;
        let frame_writes = if linkage_first { 2 } else { 1 };
        let mut run = 0;

        // A lone `li` is normally handled by the saved-LR-store scheduler. Once
        // a move has already filled that slot it comes along here; linkage-first
        // frames also need this pass because `stwu` separates the store and body.
        let mut saw_move = false;
        while run < 2 {
            let Some(instruction) = self.output.instructions.get(store + frame_writes + run) else {
                break;
            };
            let hoistable = match *instruction {
                Instruction::Or { a, s, b } => {
                    let movable = a != 0 && s != 0 && b != 0;
                    saw_move |= movable;
                    movable
                }
                Instruction::AddImmediate { d, a, .. } => {
                    d != 0 && (a != 0 || saw_move || linkage_first)
                }
                ref other if is_argument_alu(other) => {
                    let movable = mwcc_vreg::register_operands(other)
                        .iter()
                        .all(|operand| operand.register != 0);
                    saw_move |= movable;
                    movable
                }
                _ => false,
            };
            if !hoistable {
                break;
            }
            run += 1;
        }
        if run == 0 {
            return;
        }

        if linkage_first {
            remap_linkage_first_relocations(&mut self.output.relocations, store, run);
            self.output.instructions[store..store + 3].rotate_left(2);
            if run == 2 {
                self.output.instructions.swap(store + 2, store + 3);
            }
        } else {
            remap_predecrement_relocations(&mut self.output.relocations, store, run);
            self.output.instructions[store..=store + run].rotate_left(1);
        }
    }
}

fn is_argument_alu(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::Add { .. }
            | Instruction::MultiplyLow { .. }
            | Instruction::SubtractFrom { .. }
            | Instruction::And { .. }
            | Instruction::Xor { .. }
            | Instruction::ShiftLeftWord { .. }
            | Instruction::ShiftRightWord { .. }
            | Instruction::ShiftRightAlgebraicWord { .. }
            | Instruction::Negate { .. }
            | Instruction::ShiftLeftImmediate { .. }
            | Instruction::ShiftRightAlgebraicImmediate { .. }
            | Instruction::ShiftRightLogicalImmediate { .. }
            | Instruction::ClearLeftImmediate { .. }
            | Instruction::AndContiguousMask { .. }
            | Instruction::RotateAndMask { .. }
            | Instruction::OrImmediate { .. }
            | Instruction::ExtendSignByte { .. }
            | Instruction::ExtendSignHalfword { .. }
    )
}

fn remap_linkage_first_relocations(
    relocations: &mut [mwcc_machine_code::Relocation],
    store: usize,
    run: usize,
) {
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index.checked_sub(store) {
            Some(0) => store + 1,
            Some(1) => store + if run == 2 { 3 } else { 2 },
            Some(2) => store,
            Some(3) if run == 2 => store + 2,
            _ => relocation.instruction_index,
        };
    }
}

fn remap_predecrement_relocations(
    relocations: &mut [mwcc_machine_code::Relocation],
    store: usize,
    run: usize,
) {
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index.checked_sub(store) {
            Some(0) => store + run,
            Some(offset) if offset <= run => store + offset - 1,
            _ => relocation.instruction_index,
        };
    }
}
