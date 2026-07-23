//! Argument preparation scheduled into non-leaf prologue latency slots.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Fill a linkage-first prologue that was produced by late frame
    /// normalization rather than emitted in its final convention.
    ///
    /// Structured owners initially use the allocator's predecrement frame and
    /// therefore cannot use the ordinary body-time hoist. Once normalization
    /// has produced `mflr; stw LR; stwu`, the same measured scheduling rule can
    /// safely recognize and fill its two linkage hazards.
    pub(crate) fn hoist_normalized_linkage_first_arg_moves(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !matches!(
                self.output.instructions.as_slice(),
                [
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::StoreWord {
                        s: 0,
                        a: 1,
                        offset: 4
                    },
                    Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -8
                    },
                    ..
                ]
            )
        {
            return;
        }
        self.hoist_leading_arg_moves(Some(1));
    }

    /// Fill the non-leaf prologue's linkage-write latency with leading register-
    /// ALU argument setup and floating-register copies that are ready at
    /// function entry.
    ///
    /// A register move/derivation qualifies; a memory load and anything touching
    /// r0 do not. Mainline places at most two instructions before the LR store.
    /// Build 163 places the first before that store and up to two more between
    /// the store and `stwu`, preserving argument order while filling both
    /// linkage-write hazards.
    pub(crate) fn hoist_leading_arg_moves(&mut self, lr_store_index: Option<usize>) {
        let Some(store) = lr_store_index else { return };
        let linkage_first = self.behavior.frame_convention == FrameConvention::LinkageFirst;
        let frame_writes = if linkage_first { 2 } else { 1 };
        let mut run = 0;

        // A lone `li` is normally handled by the saved-LR-store scheduler. Once
        // a move has already filled that slot it comes along here; linkage-first
        // frames also need this pass because `stwu` separates the store and body.
        let mut saw_move = false;
        let slot_capacity = if linkage_first { 3 } else { 2 };
        while run < slot_capacity {
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
                Instruction::AddImmediateShifted { d, a: 0, .. } => d != 0 && linkage_first,
                ref other if is_hoistable_argument_register_op(other) => {
                    saw_move = true;
                    true
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
            let scheduled = &mut self.output.instructions[store..store + run + 2];
            scheduled.rotate_left(2);
            scheduled[1..=run].rotate_right(1);
        } else {
            remap_predecrement_relocations(&mut self.output.relocations, store, run);
            self.output.instructions[store..=store + run].rotate_left(1);
        }
    }
}

fn is_argument_register_op(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::FloatMove { .. }
            | Instruction::Add { .. }
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

fn is_hoistable_argument_register_op(instruction: &Instruction) -> bool {
    is_argument_register_op(instruction)
        && mwcc_vreg::register_operands(instruction)
            .iter()
            .all(|operand| {
                operand.class != mwcc_vreg::Class::General || operand.register != 0
            })
}

fn remap_linkage_first_relocations(
    relocations: &mut [mwcc_machine_code::Relocation],
    store: usize,
    run: usize,
) {
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index.checked_sub(store) {
            Some(0) => store + 1,
            Some(1) => store + run + 1,
            Some(2) => store,
            Some(offset) if offset <= run + 1 => store + offset - 1,
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    fn relocations(count: usize) -> Vec<Relocation> {
        (0..count)
            .map(|instruction_index| Relocation {
                instruction_index,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("symbol".to_string()),
            })
            .collect()
    }

    #[test]
    fn linkage_first_relocation_permutation_tracks_three_setup_slots() {
        let mut relocations = relocations(6);
        remap_linkage_first_relocations(&mut relocations, 1, 3);
        let indices: Vec<usize> = relocations
            .iter()
            .map(|relocation| relocation.instruction_index)
            .collect();
        assert_eq!(indices, [0, 2, 5, 1, 3, 4]);
    }

    #[test]
    fn floating_scratch_zero_is_safe_for_argument_hoisting() {
        assert!(is_hoistable_argument_register_op(&Instruction::FloatMove {
            d: 0,
            b: 1,
        }));
        assert!(!is_hoistable_argument_register_op(&Instruction::Or {
            a: 3,
            s: 0,
            b: 0,
        }));
    }
}
