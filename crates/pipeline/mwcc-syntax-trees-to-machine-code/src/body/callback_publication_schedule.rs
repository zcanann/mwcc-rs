//! Scheduling a callback publication immediately before invoking it.
//!
//! When a callback address is stored globally and the same callback is then
//! called with one loaded argument, MWCC borrows r4 for the address chain. That
//! frees r3 for the argument load and overlaps the callback high/low latency.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_callback_publication_call(&mut self) {
        let Some(start) = (0..self.output.instructions.len().saturating_sub(4)).find(|&start| {
            let [Instruction::AddImmediateShifted { d: high, a: 0, .. }, Instruction::AddImmediate {
                d: published,
                a: low_base,
                ..
            }, Instruction::LoadWord {
                d: argument, a: 0, ..
            }, Instruction::StoreWord {
                s: stored, a: 0, ..
            }, Instruction::BranchAndLink { target }] = &self.output.instructions[start..start + 5]
            else {
                return false;
            };
            if *high != Eabi::FIRST_GENERAL_ARGUMENT
                || *low_base != *high
                || *argument != *high
                || published != stored
                || *published == Eabi::FIRST_GENERAL_ARGUMENT + 1
                || self
                    .call_parameter_types
                    .get(target)
                    .is_none_or(|parameters| {
                        parameters.len() != 1 || matches!(parameters[0], Type::Float | Type::Double)
                    })
            {
                return false;
            }
            let Some(high_target) =
                relocation_target(&self.output.relocations, start, RelocationKind::Addr16Ha)
            else {
                return false;
            };
            let Some(low_target) = relocation_target(
                &self.output.relocations,
                start + 1,
                RelocationKind::Addr16Lo,
            ) else {
                return false;
            };
            high_target == low_target
                && high_target == target
                && has_relocation(
                    &self.output.relocations,
                    start + 2,
                    RelocationKind::EmbSda21,
                )
                && has_relocation(
                    &self.output.relocations,
                    start + 3,
                    RelocationKind::EmbSda21,
                )
        }) else {
            return;
        };

        let borrowed = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        let Instruction::AddImmediateShifted { d, .. } = &mut self.output.instructions[start]
        else {
            unreachable!()
        };
        *d = borrowed;
        let Instruction::AddImmediate { a, .. } = &mut self.output.instructions[start + 1] else {
            unreachable!()
        };
        *a = borrowed;
        self.output.instructions.swap(start + 1, start + 2);
        swap_relocation_indices(&mut self.output.relocations, start + 1, start + 2);
    }
}

fn relocation_target(
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

fn has_relocation(
    relocations: &[mwcc_machine_code::Relocation],
    instruction_index: usize,
    kind: RelocationKind,
) -> bool {
    relocations.iter().any(|relocation| {
        relocation.instruction_index == instruction_index && relocation.kind == kind
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
    fn finds_only_the_requested_external_relocation_kind() {
        let relocations = [
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("callback".into()),
            },
            Relocation {
                instruction_index: 2,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("other".into()),
            },
        ];

        assert_eq!(
            relocation_target(&relocations, 2, RelocationKind::Addr16Ha),
            Some("callback")
        );
    }
}
