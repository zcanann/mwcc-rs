//! Linkage-first layout for a non-leaf frame with one escaped scalar local.
//!
//! Structured lowering initially emits the safe predecrement layout shared by
//! newer compilers. Once the complete physical frame is known, legacy builds
//! move linkage into the caller area, place the lone scalar in the low local
//! word, and issue its escaped address before the initializing store.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn normalize_linkage_first_addressable_scalar_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.non_leaf
            || !self.callee_saved.is_empty()
            || self.callee_saved_float != 0
            || self.frame_size != 16
            || !predecrement_entry(&self.output.instructions)
        {
            return;
        }

        let mut addressable_slots = self.frame_slots.values_mut().filter(|slot| {
            !slot.is_array && slot.parameter_register.is_none() && slot.size == 4
        });
        let Some(slot) = addressable_slots.next() else {
            return;
        };
        if addressable_slots.next().is_some() || slot.offset != 12 {
            return;
        }
        slot.offset = 8;

        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWord { a: 1, offset, .. }
                | Instruction::StoreByte { a: 1, offset, .. }
                | Instruction::StoreHalfword { a: 1, offset, .. }
                | Instruction::LoadWord { a: 1, offset, .. }
                | Instruction::LoadByteZero { a: 1, offset, .. }
                | Instruction::LoadHalfwordZero { a: 1, offset, .. }
                | Instruction::LoadHalfwordAlgebraic { a: 1, offset, .. }
                    if *offset == 12 =>
                {
                    *offset = 8;
                }
                Instruction::AddImmediate {
                    a: 1, immediate, ..
                } if *immediate == 12 => {
                    *immediate = 8;
                }
                _ => {}
            }
        }

        crate::move_instruction_before_retargeting(self, 1, 0);
        crate::move_instruction_before_retargeting(self, 2, 1);
        let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[1] else {
            unreachable!("the linkage store was recognized before rotation")
        };
        *offset = 4;

        if let Some((store, address)) = initializer_address_pair(&self.output.instructions, 8) {
            crate::move_instruction_before_retargeting(self, address, store);
        }
    }
}

fn predecrement_entry(instructions: &[Instruction]) -> bool {
    matches!(instructions.get(0..3), Some([
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord { s: 0, a: 1, offset: 20 },
    ]))
}

fn initializer_address_pair(instructions: &[Instruction], slot: i16) -> Option<(usize, usize)> {
    let first_call = instructions
        .iter()
        .position(|instruction| {
            matches!(instruction,
                Instruction::BranchAndLink { .. }
                    | Instruction::BranchToLinkRegisterAndLink
                    | Instruction::BranchToCountRegisterAndLink)
        })
        .unwrap_or(instructions.len());
    instructions[..first_call]
        .windows(2)
        .enumerate()
        .find_map(|(index, window)| {
            matches!(&window[0],
                Instruction::StoreWord { a: 1, offset, .. }
                    | Instruction::StoreByte { a: 1, offset, .. }
                    | Instruction::StoreHalfword { a: 1, offset, .. }
                    if *offset == slot)
                .then_some(())?;
            matches!(window[1],
                Instruction::AddImmediate { d, a: 1, immediate }
                    if d > 2 && immediate == slot)
                .then_some((index, index + 1))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_the_safe_entry_and_initializer_address_packet() {
        let instructions = vec![
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 20 },
            Instruction::load_immediate(0, 4),
            Instruction::StoreWord { s: 0, a: 1, offset: 12 },
            Instruction::AddImmediate { d: 5, a: 1, immediate: 12 },
            Instruction::BranchAndLink { target: "consume".into() },
        ];

        assert!(predecrement_entry(&instructions));
        assert_eq!(initializer_address_pair(&instructions, 12), Some((4, 5)));
    }

    #[test]
    fn rejects_an_address_that_does_not_escape_to_the_first_call() {
        let instructions = vec![
            Instruction::StoreWord { s: 0, a: 1, offset: 8 },
            Instruction::BranchAndLink { target: "consume".into() },
            Instruction::AddImmediate { d: 5, a: 1, immediate: 8 },
        ];

        assert_eq!(initializer_address_pair(&instructions, 8), None);
    }
}
