//! Build-163 reuse across adjacent volatile accesses to one fixed register bank.
//!
//! A constant store, a volatile self-load/store, and a following independent
//! clear form one scheduling region. The optimizer keeps the bank high page
//! live, derives a second full base from it, and reuses the high-page register
//! as the self-load destination while the clear occupies r0.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    start: usize,
    high_base: u8,
    full_base: u8,
    scratch: u8,
    high_adjusted: i16,
    low: i16,
    value: i16,
    element_offset: i16,
}

fn recognize(instructions: &[Instruction]) -> Option<Region> {
    instructions
        .windows(9)
        .enumerate()
        .find_map(|(start, window)| {
            let [Instruction::AddImmediateShifted {
                d: high_base,
                a: 0,
                immediate: high_adjusted,
            }, Instruction::AddImmediate {
                d: completed_high,
                a: high_source,
                immediate: low,
            }, Instruction::AddImmediate {
                d: scratch,
                a: 0,
                immediate: value,
            }, Instruction::StoreWord {
                s: first_value,
                a: first_base,
                offset: 0,
            }, Instruction::AddImmediateShifted {
                d: full_base,
                a: 0,
                immediate: repeated_high,
            }, Instruction::AddImmediate {
                d: completed_full,
                a: full_source,
                immediate: repeated_low,
            }, Instruction::LoadWord {
                d: loaded,
                a: load_base,
                offset: element_offset,
            }, Instruction::StoreWord {
                s: stored,
                a: store_base,
                offset: stored_offset,
            }, Instruction::AddImmediate {
                d: cleared,
                a: 0,
                immediate: 0,
            }] = window
            else {
                return None;
            };
            if high_base != completed_high
                || high_base != high_source
                || high_base != first_base
                || full_base != completed_full
                || full_base != full_source
                || high_base == full_base
                || scratch != first_value
                || scratch != loaded
                || scratch != stored
                || scratch != cleared
                || full_base != load_base
                || full_base != store_base
                || high_adjusted != repeated_high
                || low != repeated_low
                || element_offset != stored_offset
                || *value == 0
                || *low == 0
                || low.checked_add(*element_offset).is_none()
            {
                return None;
            }
            Some(Region {
                start,
                high_base: *high_base,
                full_base: *full_base,
                scratch: *scratch,
                high_adjusted: *high_adjusted,
                low: *low,
                value: *value,
                element_offset: *element_offset,
            })
        })
}

fn has_branch_target_in(instructions: &[Instruction], region: std::ops::Range<usize>) -> bool {
    instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target }
                if region.contains(target)
        )
    })
}

impl Generator {
    pub(crate) fn fuse_linkage_first_fixed_bank_region(&mut self) {
        if self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst
            || self.behavior.fixed_address_poll_address_style
                != mwcc_versions::FixedAddressPollAddressStyle::MaterializedBankPage
        {
            return;
        }
        let Some(region) = recognize(&self.output.instructions) else {
            return;
        };
        let range = region.start..region.start + 9;
        if has_branch_target_in(&self.output.instructions, range.clone())
            || self
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

        let low_element = region
            .low
            .checked_add(region.element_offset)
            .expect("the recognized fixed-bank displacement fits");
        self.output.instructions[region.start..region.start + 7].clone_from_slice(&[
            Instruction::load_immediate_shifted(region.high_base, region.high_adjusted),
            Instruction::load_immediate(region.scratch, region.value),
            Instruction::StoreWord {
                s: region.scratch,
                a: region.high_base,
                offset: region.low,
            },
            Instruction::AddImmediate {
                d: region.full_base,
                a: region.high_base,
                immediate: region.low,
            },
            Instruction::load_immediate(region.scratch, 0),
            Instruction::LoadWord {
                d: region.high_base,
                a: region.high_base,
                offset: low_element,
            },
            Instruction::StoreWord {
                s: region.high_base,
                a: region.full_base,
                offset: region.element_offset,
            },
        ]);
        crate::remove_instruction_retargeting_to_next(self, region.start + 8);
        crate::remove_instruction_retargeting_to_next(self, region.start + 7);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bank_region(repeated_high: i16) -> Vec<Instruction> {
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
            Instruction::load_immediate_shifted(33, repeated_high),
            Instruction::AddImmediate {
                d: 33,
                a: 33,
                immediate: 0x6000,
            },
            Instruction::LoadWord {
                d: 0,
                a: 33,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 0,
                a: 33,
                offset: 4,
            },
            Instruction::load_immediate(0, 0),
        ]
    }

    #[test]
    fn recognizes_a_complete_linkage_first_bank_region() {
        assert_eq!(
            recognize(&bank_region(-13312)),
            Some(Region {
                start: 0,
                high_base: 32,
                full_base: 33,
                scratch: 0,
                high_adjusted: -13312,
                low: 0x6000,
                value: 42,
                element_offset: 4,
            })
        );
    }

    #[test]
    fn rejects_a_bank_region_with_a_mismatched_repeated_page() {
        assert_eq!(recognize(&bank_region(-13311)), None);
    }
}
