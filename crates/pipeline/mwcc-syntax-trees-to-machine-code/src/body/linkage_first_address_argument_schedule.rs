//! Completed address chains for linkage-first call arguments.
//!
//! Build 163 keeps an absolute global address's `lis`/`addi` pair adjacent
//! before issuing a following integer argument. Later generations use the
//! integer materialization as an address-latency filler.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_address_constant_arguments(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        let Some(start) = (0..self.output.instructions.len().saturating_sub(3)).find(|&start| {
            let [Instruction::AddImmediateShifted {
                d: address, a: 0, ..
            }, Instruction::AddImmediate {
                d: constant, a: 0, ..
            }, Instruction::AddImmediate {
                d: completed,
                a: base,
                ..
            }, Instruction::BranchAndLink { .. }] = &self.output.instructions[start..start + 4]
            else {
                return false;
            };
            if address != completed
                || address != base
                || address == constant
                || !(3..=10).contains(address)
                || !(3..=10).contains(constant)
            {
                return false;
            }
            let Some(high_target) = external_relocation_target(
                &self.output.relocations,
                start,
                RelocationKind::Addr16Ha,
            ) else {
                return false;
            };
            let Some(low_target) = external_relocation_target(
                &self.output.relocations,
                start + 2,
                RelocationKind::Addr16Lo,
            ) else {
                return false;
            };
            high_target == low_target
                && !self.call_return_types.contains_key(high_target)
                && (self.global_array_sizes.contains_key(high_target)
                    || self.addressable_globals.contains_key(high_target))
        }) else {
            return;
        };

        self.output.instructions.swap(start + 1, start + 2);
        swap_relocation_indices(&mut self.output.relocations, start + 1, start + 2);
    }
}

fn external_relocation_target(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != instruction_index || relocation.kind != kind {
            return None;
        }
        let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
            return None;
        };
        Some(target.as_str())
    })
}

fn swap_relocation_indices(
    relocations: &mut [mwcc_machine_code::Relocation],
    first: usize,
    second: usize,
) {
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index {
            index if index == first => second,
            index if index == second => first,
            index => index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn swaps_every_relocation_attached_to_two_scheduled_instructions() {
        let mut relocations = vec![
            Relocation {
                instruction_index: 4,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("buffer".into()),
            },
            Relocation {
                instruction_index: 3,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External("other".into()),
            },
        ];

        swap_relocation_indices(&mut relocations, 3, 4);

        assert_eq!(relocations[0].instruction_index, 3);
        assert_eq!(relocations[1].instruction_index, 4);
    }
}
