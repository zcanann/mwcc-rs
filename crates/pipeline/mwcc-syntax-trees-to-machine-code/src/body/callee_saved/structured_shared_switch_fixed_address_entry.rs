//! Fixed-address read/modify/write entry scheduling for shared-switch bodies.
//!
//! A large dispatcher exposes three independent entry lifetimes before its
//! first context call: the fixed-address bank, its halfword load, and the
//! narrowed value. Build 163 assigns those lifetimes r6/r5/r4 and uses their
//! ready operations to fill the linkage-save and saved-parameter gaps. Keep
//! that measured schedule behind the semantic shared-switch owner rather than
//! teaching the generic frame emitter about one whole-body allocation choice.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_shared_switch_fixed_address_entry(&mut self) {
        let Some(entry) = fixed_address_entry(&self.output.instructions) else {
            return;
        };
        if self
            .output
            .relocations
            .iter()
            .any(|relocation| relocation.instruction_index == entry.high)
        {
            return;
        }

        let bank = self.fresh_virtual_general_preferring(6);
        let loaded = self.fresh_virtual_general_preferring(5);
        let narrowed = self.fresh_virtual_general_preferring(4);

        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[entry.high]
        else {
            unreachable!("the fixed-address high half was recognized")
        };
        *d = bank;
        let Instruction::LoadHalfwordZero { d, a, .. } = &mut self.output.instructions[entry.load]
        else {
            unreachable!("the fixed-address load was recognized")
        };
        *d = loaded;
        *a = bank;
        let Instruction::ClearLeftImmediate { a, s, .. } =
            &mut self.output.instructions[entry.narrow]
        else {
            unreachable!("the fixed-address narrowing operation was recognized")
        };
        *a = narrowed;
        *s = loaded;
        let Instruction::And { s, .. } = &mut self.output.instructions[entry.merge] else {
            unreachable!("the fixed-address mask merge was recognized")
        };
        *s = narrowed;
        let Instruction::StoreHalfword { a, .. } = &mut self.output.instructions[entry.store]
        else {
            unreachable!("the fixed-address store was recognized")
        };
        *a = bank;

        // stwu; mflr; lis bank; stw LR; li mask; addi context;
        // stw saved; mr saved; lhz; clrlwi; and; ori; sth; bl
        self.move_instruction_before(entry.high, 2);
        self.move_instruction_before(entry.mask, 4);
        self.move_instruction_before(entry.context_argument, 5);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FixedAddressEntry {
    high: usize,
    mask: usize,
    load: usize,
    narrow: usize,
    merge: usize,
    store: usize,
    context_argument: usize,
}

fn fixed_address_entry(instructions: &[Instruction]) -> Option<FixedAddressEntry> {
    let [Instruction::StoreWordWithUpdate {
        s: 1,
        a: 1,
        offset: frame_update,
    }, Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: link_offset,
    }, Instruction::StoreWord { s: saved, a: 1, .. }, saved_copy, Instruction::AddImmediateShifted {
        d: bank,
        a: 0,
        immediate: bank_high,
    }, Instruction::AddImmediate { d: 0, a: 0, .. }, Instruction::LoadHalfwordZero {
        d: loaded,
        a: load_bank,
        offset: load_offset,
    }, Instruction::ClearLeftImmediate {
        a: narrowed,
        s: narrow_source,
        clear: 16,
    }, Instruction::And {
        a: 0,
        s: merge_source,
        b: 0,
    }, Instruction::OrImmediate { a: 0, s: 0, .. }, Instruction::StoreHalfword {
        s: 0,
        a: store_bank,
        offset: store_offset,
    }, Instruction::AddImmediate {
        d: 3,
        a: 1,
        immediate: context_offset,
    }, Instruction::BranchAndLink { .. }, ..] = instructions
    else {
        return None;
    };
    let saved_source = match saved_copy {
        Instruction::Or { a, s: 4, b: 4 }
        | Instruction::AddImmediate {
            d: a,
            a: 4,
            immediate: 0,
        } => *a,
        _ => return None,
    };
    (*frame_update < 0
        && i32::from(*link_offset) == -i32::from(*frame_update) + 4
        && *saved == saved_source
        && *bank_high != 0
        && *bank == *load_bank
        && *bank == *store_bank
        && *loaded == *narrow_source
        && *narrowed == *merge_source
        && *load_offset == *store_offset
        && *context_offset > 0
        && [*bank, *loaded, *narrowed]
            .into_iter()
            .all(|register| register >= mwcc_vreg::VIRTUAL_BASE)
        && *bank != *loaded
        && *bank != *narrowed
        && *loaded != *narrowed)
        .then_some(FixedAddressEntry {
            high: 5,
            mask: 6,
            load: 7,
            narrow: 8,
            merge: 9,
            store: 11,
            context_argument: 12,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_saved_dispatcher_fixed_address_entry() {
        let saved = mwcc_vreg::VIRTUAL_BASE;
        let bank = saved + 1;
        let loaded = saved + 2;
        let narrowed = saved + 3;
        let instructions = vec![
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -736,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 740,
            },
            Instruction::StoreWord {
                s: saved,
                a: 1,
                offset: 732,
            },
            Instruction::move_register(saved, 4),
            Instruction::load_immediate_shifted(bank, -13312),
            Instruction::load_immediate(0, -41),
            Instruction::LoadHalfwordZero {
                d: loaded,
                a: bank,
                offset: 20490,
            },
            Instruction::ClearLeftImmediate {
                a: narrowed,
                s: loaded,
                clear: 16,
            },
            Instruction::And {
                a: 0,
                s: narrowed,
                b: 0,
            },
            Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: 128,
            },
            Instruction::StoreHalfword {
                s: 0,
                a: bank,
                offset: 20490,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 1,
                immediate: 8,
            },
            Instruction::BranchAndLink {
                target: "clear_context".into(),
            },
        ];

        assert_eq!(
            fixed_address_entry(&instructions),
            Some(FixedAddressEntry {
                high: 5,
                mask: 6,
                load: 7,
                narrow: 8,
                merge: 9,
                store: 11,
                context_argument: 12,
            })
        );
    }
}
