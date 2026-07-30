//! Linkage-first entry scheduling for a late inlined switch transaction.
//!
//! Whole-file IPA can place a multi-call switch after an existing fixed-bank
//! store. Build 163 retains one optimizer frame lane, shares the switch's BSS
//! anchor with the caller, and folds the hardware-bank page low half back into
//! the store displacement.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_inlined_switch_entry(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_inline_expansion_frame_bytes != 8
            || self.frame_size != 24
            || self.data_section_anchor.is_none()
        {
            return;
        }
        let Some((bank_low, store_offset)) = inlined_switch_entry(&self.output.instructions) else {
            return;
        };
        let Some(folded_offset) = bank_low.checked_add(store_offset) else {
            return;
        };

        let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[9] else {
            unreachable!("the inlined-switch entry store was matched")
        };
        *offset = folded_offset;
        crate::remove_instruction_retargeting_to_next(self, 8);

        // mflr; lis bank; stw LR; li zero; lis anchor; stwu; stw r31;
        // addi anchor; stw fixed; li state; lwz state; stw state.
        crate::move_instruction_before_retargeting(self, 7, 1);
        crate::move_instruction_before_retargeting(self, 6, 4);
        crate::move_instruction_before_retargeting(self, 10, 9);

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[4] else {
            unreachable!("the inlined-switch anchor high half was matched")
        };
        *d = 4;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[7] else {
            unreachable!("the inlined-switch anchor low half was matched")
        };
        *a = 4;
        let Instruction::LoadWord { d, .. } = &mut self.output.instructions[10] else {
            unreachable!("the inlined-switch state load was matched")
        };
        *d = 3;
        let Instruction::StoreWord { a, .. } = &mut self.output.instructions[11] else {
            unreachable!("the inlined-switch state store was matched")
        };
        *a = 3;
    }
}

fn inlined_switch_entry(instructions: &[Instruction]) -> Option<(i16, i16)> {
    match instructions {
        [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 0,
                immediate: 0,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediateShifted {
                d: anchor_high,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 31,
                a,
                immediate: 0,
            },
            Instruction::AddImmediateShifted { d: bank, a: 0, .. },
            Instruction::AddImmediate {
                d: full_bank,
                a: bank_source,
                immediate: bank_low,
            },
            Instruction::StoreWord {
                s: 0,
                a: store_base,
                offset: store_offset,
            },
            Instruction::LoadWord { d: state, a: 0, .. },
            Instruction::AddImmediate { d: 0, a: 0, .. },
            Instruction::StoreWord {
                s: 0,
                a: state_base,
                ..
            },
            ..,
        ] if anchor_high == a
            && bank == full_bank
            && bank == bank_source
            && bank == store_base
            && state == state_base =>
        {
            Some((*bank_low, *store_offset))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_anchor_after_a_materialized_fixed_bank_store() {
        let instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::load_immediate_shifted(5, 0),
            Instruction::AddImmediate {
                d: 31,
                a: 5,
                immediate: 0,
            },
            Instruction::load_immediate_shifted(3, -13312),
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0x6000,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: 4,
            },
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::load_immediate(0, 3),
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 12,
            },
        ];

        assert_eq!(inlined_switch_entry(&instructions), Some((0x6000, 4)));
    }
}
