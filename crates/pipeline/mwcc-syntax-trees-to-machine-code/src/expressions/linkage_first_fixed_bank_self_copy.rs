//! Linkage-first split-base scheduling for a volatile fixed-bank self-copy.
//!
//! Selection materializes the complete bank address before loading and storing
//! one volatile slot. Build 163 instead keeps the adjusted high page live for
//! the load, derives a second full-base value, and stores through that value.
//! The split value remains virtual here so ordinary liveness and allocation
//! prove that its preferred physical home is available.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    start: usize,
    high_base: u8,
    scratch: u8,
    high_adjusted: i16,
    low: i16,
    element_offset: i16,
}

fn recognize_at(instructions: &[Instruction], start: usize) -> Option<Region> {
    let [Instruction::AddImmediateShifted {
        d: high_base,
        a: 0,
        immediate: high_adjusted,
    }, Instruction::AddImmediate {
        d: completed_base,
        a: base_source,
        immediate: low,
    }, Instruction::LoadWord {
        d: scratch,
        a: load_base,
        offset: element_offset,
    }, Instruction::StoreWord {
        s: stored,
        a: store_base,
        offset: stored_offset,
    }] = instructions.get(start..start + 4)?
    else {
        return None;
    };
    if high_base != completed_base
        || high_base != base_source
        || high_base != load_base
        || high_base != store_base
        || scratch != stored
        || element_offset != stored_offset
        || *low == 0
        || low.checked_add(*element_offset).is_none()
    {
        return None;
    }
    Some(Region {
        start,
        high_base: *high_base,
        scratch: *scratch,
        high_adjusted: *high_adjusted,
        low: *low,
        element_offset: *element_offset,
    })
}

fn callback_argument_register(
    instructions: &[Instruction],
    relocations: &[mwcc_machine_code::Relocation],
    after: usize,
) -> Option<u8> {
    let call = instructions[after..]
        .iter()
        .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        .map(|relative| after + relative)?;
    let relocation = relocations
        .iter()
        .filter(|relocation| {
            (after..call).contains(&relocation.instruction_index)
                && relocation.kind == RelocationKind::Addr16Lo
        })
        .last()?;
    match instructions.get(relocation.instruction_index)? {
        Instruction::AddImmediate { d, .. } => Some(*d),
        _ => None,
    }
}

fn has_interior_branch_target(instructions: &[Instruction], start: usize) -> bool {
    let interior = start + 1..start + 4;
    instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target }
                if interior.contains(target)
        )
    })
}

fn rewrite(instructions: &mut [Instruction], region: Region, full_base: u8) {
    let low_element = region
        .low
        .checked_add(region.element_offset)
        .expect("the recognized fixed-bank displacement fits");
    instructions[region.start..region.start + 4].clone_from_slice(&[
        Instruction::load_immediate_shifted(region.high_base, region.high_adjusted),
        Instruction::LoadWord {
            d: region.scratch,
            a: region.high_base,
            offset: low_element,
        },
        Instruction::AddImmediate {
            d: full_base,
            a: region.high_base,
            immediate: region.low,
        },
        Instruction::StoreWord {
            s: region.scratch,
            a: full_base,
            offset: region.element_offset,
        },
    ]);
}

impl Generator {
    pub(crate) fn split_linkage_first_fixed_bank_self_copies(&mut self) {
        if self.behavior.frame_convention != mwcc_versions::FrameConvention::LinkageFirst
            || self.behavior.fixed_address_poll_address_style
                != mwcc_versions::FixedAddressPollAddressStyle::MaterializedBankPage
        {
            return;
        }

        let mut start = 0;
        while start + 4 <= self.output.instructions.len() {
            let Some(region) = recognize_at(&self.output.instructions, start) else {
                start += 1;
                continue;
            };
            let range = start..start + 4;
            if has_interior_branch_target(&self.output.instructions, start)
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
                start += 4;
                continue;
            }
            let Some(callback) = callback_argument_register(
                &self.output.instructions,
                &self.output.relocations,
                start + 4,
            ) else {
                start += 4;
                continue;
            };
            // r4 is MWCC's first disposable argument lane. When the callback
            // itself occupies r4, the independent bank base takes r5 instead.
            let preferred = if callback == 4 { 5 } else { 4 };
            let full_base = self.fresh_virtual_general_preferring(preferred);
            rewrite(&mut self.output.instructions, region, full_base);
            start += 4;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    fn self_copy() -> Vec<Instruction> {
        vec![
            Instruction::load_immediate_shifted(32, -13312),
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 0x6000,
            },
            Instruction::LoadWord {
                d: 0,
                a: 32,
                offset: 4,
            },
            Instruction::StoreWord {
                s: 0,
                a: 32,
                offset: 4,
            },
        ]
    }

    #[test]
    fn recognizes_a_materialized_fixed_bank_self_copy() {
        assert_eq!(
            recognize_at(&self_copy(), 0),
            Some(Region {
                start: 0,
                high_base: 32,
                scratch: 0,
                high_adjusted: -13312,
                low: 0x6000,
                element_offset: 4,
            })
        );
    }

    #[test]
    fn splits_the_load_and_store_bases_without_physical_register_assumptions() {
        let mut instructions = self_copy();
        rewrite(
            &mut instructions,
            recognize_at(&self_copy(), 0).expect("self-copy region"),
            33,
        );
        assert_eq!(
            instructions,
            vec![
                Instruction::load_immediate_shifted(32, -13312),
                Instruction::LoadWord {
                    d: 0,
                    a: 32,
                    offset: 0x6004,
                },
                Instruction::AddImmediate {
                    d: 33,
                    a: 32,
                    immediate: 0x6000,
                },
                Instruction::StoreWord {
                    s: 0,
                    a: 33,
                    offset: 4,
                },
            ]
        );
    }

    #[test]
    fn finds_the_relocated_callback_argument_before_the_call() {
        let mut instructions = self_copy();
        instructions.extend([
            Instruction::load_immediate_shifted(34, 0),
            Instruction::AddImmediate {
                d: 4,
                a: 34,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "start".to_string(),
            },
        ]);
        let relocations = vec![
            Relocation {
                instruction_index: 4,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("callback".to_string()),
            },
            Relocation {
                instruction_index: 5,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("callback".to_string()),
            },
        ];

        assert_eq!(
            callback_argument_register(&instructions, &relocations, 4),
            Some(4)
        );
    }
}
