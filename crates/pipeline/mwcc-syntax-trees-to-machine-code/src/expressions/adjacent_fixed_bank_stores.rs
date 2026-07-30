//! Build-163 reuse across adjacent constant-slot stores to one fixed register bank.
//!
//! Selection initially preserves each declared address as `lis; addi`. When two
//! slots are written consecutively, MWCC instead retains the high page and folds
//! the bank low half into both store displacements.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    start: usize,
    first_base: u8,
    second_base: u8,
    low: i16,
    first_offset: i16,
    second_offset: i16,
}

fn store_operands(instruction: &Instruction) -> Option<(u8, u8, i16)> {
    match instruction {
        Instruction::StoreByte { s, a, offset }
        | Instruction::StoreHalfword { s, a, offset }
        | Instruction::StoreWord { s, a, offset } => Some((*s, *a, *offset)),
        _ => None,
    }
}

fn recognize(instructions: &[Instruction]) -> Option<Region> {
    instructions
        .windows(7)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::AddImmediateShifted {
                d: first_base,
                a: 0,
                immediate: first_high,
            }, Instruction::AddImmediate {
                d: completed_first,
                a: first_source,
                immediate: first_low,
            }, Instruction::AddImmediate {
                d: value,
                a: 0,
                ..
            }, first_store, Instruction::AddImmediateShifted {
                d: second_base,
                a: 0,
                immediate: second_high,
            }, Instruction::AddImmediate {
                d: completed_second,
                a: second_source,
                immediate: second_low,
            }, second_store] = window
            else {
                return None;
            };
            let Some((first_value, first_store_base, first_offset)) =
                store_operands(first_store)
            else {
                return None;
            };
            let Some((_, second_store_base, second_offset)) = store_operands(second_store) else {
                return None;
            };
            if first_base != completed_first
                || first_base != first_source
                || *first_base != first_store_base
                || second_base != completed_second
                || second_base != second_source
                || *second_base != second_store_base
                || *value != first_value
                || first_high != second_high
                || first_low != second_low
                || first_low.checked_add(first_offset).is_none()
                || first_low.checked_add(second_offset).is_none()
            {
                return None;
            }
            Some(Region {
                start,
                first_base: *first_base,
                second_base: *second_base,
                low: *first_low,
                first_offset,
                second_offset,
            })
        })
}

fn retarget_store(instruction: &mut Instruction, base: u8, offset: i16) {
    match instruction {
        Instruction::StoreByte { a, offset: at, .. }
        | Instruction::StoreHalfword { a, offset: at, .. }
        | Instruction::StoreWord { a, offset: at, .. } => {
            *a = base;
            *at = offset;
        }
        _ => unreachable!("an adjacent fixed-bank region contains stores"),
    }
}

impl Generator {
    pub(crate) fn fuse_adjacent_materialized_fixed_bank_stores(&mut self) {
        if self.behavior.fixed_address_poll_address_style
            != mwcc_versions::FixedAddressPollAddressStyle::MaterializedBankPage
        {
            return;
        }
        let Some(region) = recognize(&self.output.instructions) else {
            return;
        };
        let range = region.start..region.start + 7;
        if self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::BranchConditionalForward { target, .. }
                    | Instruction::Branch { target }
                    if range.contains(target)
            )
        }) || self
            .output
            .relocations
            .iter()
            .any(|relocation| range.contains(&relocation.instruction_index))
            || self
                .output
                .data_section_displacements
                .iter()
                .any(|displacement| range.contains(&displacement.instruction_index))
        {
            return;
        }

        let first_offset = region
            .low
            .checked_add(region.first_offset)
            .expect("the recognized first fixed-bank displacement fits");
        let second_offset = region
            .low
            .checked_add(region.second_offset)
            .expect("the recognized second fixed-bank displacement fits");
        retarget_store(
            &mut self.output.instructions[region.start + 3],
            region.first_base,
            first_offset,
        );
        retarget_store(
            &mut self.output.instructions[region.start + 6],
            region.first_base,
            second_offset,
        );
        crate::remove_instruction_retargeting_to_next(self, region.start + 5);
        crate::remove_instruction_retargeting_to_next(self, region.start + 4);
        crate::remove_instruction_retargeting_to_next(self, region.start + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adjacent_stores(second_high: i16) -> Vec<Instruction> {
        vec![
            Instruction::load_immediate_shifted(32, -13312),
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 0x6000,
            },
            Instruction::load_immediate(0, 42),
            Instruction::StoreWord {
                s: 0,
                a: 32,
                offset: 0,
            },
            Instruction::load_immediate_shifted(33, second_high),
            Instruction::AddImmediate {
                d: 33,
                a: 33,
                immediate: 0x6000,
            },
            Instruction::StoreWord {
                s: 31,
                a: 33,
                offset: 4,
            },
        ]
    }

    #[test]
    fn recognizes_adjacent_slots_in_one_materialized_bank() {
        assert_eq!(
            recognize(&adjacent_stores(-13312)),
            Some(Region {
                start: 0,
                first_base: 32,
                second_base: 33,
                low: 0x6000,
                first_offset: 0,
                second_offset: 4,
            })
        );
    }

    #[test]
    fn rejects_adjacent_stores_from_different_pages() {
        assert_eq!(recognize(&adjacent_stores(-13311)), None);
    }
}
