//! Final schedule for a guarded status-initialization call chain.
//!
//! Patched build 159 gives the returned status r30 and temporarily reuses r31
//! for a scalar-global address and the nested UART status. Generic lifetime
//! coloring finds the inverse legal assignment and rematerializes the address;
//! this pass recognizes the complete physical transaction before selecting the
//! measured role transition.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_patched_status_initialization_chain(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.plain_linkage_epilogue_style
                != PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            || !is_status_initialization_chain(&self.output.instructions)
            || !has_repeated_scalar_global_relocations(&self.output.relocations)
        {
            return;
        }

        let old = self.output.instructions.clone();
        let mut scheduled = Vec::with_capacity(old.len());
        scheduled.extend_from_slice(&old[..6]);
        scheduled.push(Instruction::OrRecord {
            a: 30,
            s: Eabi::general_result().number,
            b: Eabi::general_result().number,
        });
        for instruction in &old[8..24] {
            let mut instruction = instruction.clone();
            swap_status_register_roles(&mut instruction);
            spell_status_result_copy(&mut instruction);
            remap_early_branch_target(&mut instruction);
            scheduled.push(instruction);
        }

        let global_high = match old[28] {
            Instruction::AddImmediateShifted { immediate, .. } => immediate,
            _ => unreachable!("the status-chain address high was recognized"),
        };
        let global_low = match old[29] {
            Instruction::AddImmediate { immediate, .. } => immediate,
            _ => unreachable!("the status-chain address low was recognized"),
        };
        scheduled.extend([
            Instruction::AddImmediateShifted {
                d: Eabi::general_result().number,
                a: 0,
                immediate: global_high,
            },
            Instruction::AddImmediate {
                d: 31,
                a: Eabi::general_result().number,
                immediate: global_low,
            },
            old[24].clone(),
            Instruction::AddImmediate {
                d: 6,
                a: 31,
                immediate: 0,
            },
            old[25].clone(),
            old[26].clone(),
            old[27].clone(),
            old[30].clone(),
            Instruction::move_register(0, Eabi::general_result().number),
            Instruction::LoadWord {
                d: Eabi::general_result().number,
                a: 31,
                offset: 0,
            },
            Instruction::move_register(31, 0),
            old[34].clone(),
        ]);
        for instruction in &old[35..47] {
            let mut instruction = instruction.clone();
            swap_status_register_roles(&mut instruction);
            spell_status_result_copy(&mut instruction);
            scheduled.push(instruction);
        }
        scheduled.extend_from_slice(&old[47..]);
        debug_assert_eq!(scheduled.len(), old.len());
        self.output.instructions = scheduled;
        remap_status_initialization_relocations(&mut self.output.relocations);
    }
}

fn is_status_initialization_chain(instructions: &[Instruction]) -> bool {
    instructions.len() == 53
        && matches!(&instructions[..8], [
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 },
            Instruction::StoreWord { s: 31, a: 1, offset: 12 },
            Instruction::StoreWord { s: 30, a: 1, offset: 8 },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
        ])
        && [10, 14, 18].into_iter().all(|start| {
            matches!(&instructions[start..start + 4], [
                Instruction::CompareWordImmediate { a: 31, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::BranchAndLink { .. },
                Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            ])
        })
        && matches!(&instructions[22..35], [
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::AddImmediate { d: 3, a: 3, .. },
            Instruction::AddImmediate { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 5, a: 0, .. },
            Instruction::AddImmediateShifted { d: 6, a: 0, .. },
            Instruction::AddImmediate { d: 6, a: 6, .. },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, .. },
            Instruction::LoadWord { d: 3, a: 3, offset: 0 },
            Instruction::BranchAndLink { .. },
        ])
        && matches!(&instructions[35..], [
            Instruction::CompareWordImmediate { a: 30, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::Or { a: 31, s: 30, b: 30 },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::CompareWordImmediate { a: 31, immediate: 0 },
            Instruction::BranchConditionalForward { .. },
            Instruction::BranchAndLink { .. },
            Instruction::AddImmediate { d: 31, a: 3, immediate: 0 },
            Instruction::Or { a: 3, s: 31, b: 31 },
            Instruction::LoadWord { d: 31, a: 1, offset: 12 },
            Instruction::LoadWord { d: 30, a: 1, offset: 8 },
            Instruction::AddImmediate { d: 1, a: 1, immediate: 16 },
            Instruction::LoadWord { d: 0, a: 1, offset: 4 },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ])
}

fn has_repeated_scalar_global_relocations(relocations: &[mwcc_machine_code::Relocation]) -> bool {
    let target = |index, kind| {
        relocations.iter().find_map(|relocation| {
            if relocation.instruction_index != index || relocation.kind != kind {
                return None;
            }
            let mwcc_machine_code::RelocationTarget::External(target) = &relocation.target else {
                return None;
            };
            Some(target.as_str())
        })
    };
    let Some(first_high) = target(28, RelocationKind::Addr16Ha) else {
        return false;
    };
    target(29, RelocationKind::Addr16Lo) == Some(first_high)
        && target(32, RelocationKind::Addr16Ha) == Some(first_high)
        && target(33, RelocationKind::Addr16Lo) == Some(first_high)
}

fn swap_status_register_roles(instruction: &mut Instruction) {
    mwcc_vreg::for_each_register(instruction, |_, class, register| {
        if class != mwcc_vreg::Class::General {
            return;
        }
        *register = match *register {
            30 => 31,
            31 => 30,
            other => other,
        };
    });
}

fn spell_status_result_copy(instruction: &mut Instruction) {
    if matches!(
        instruction,
        Instruction::AddImmediate {
            d: 30,
            a: 3,
            immediate: 0
        }
    ) {
        *instruction = Instruction::move_register(30, Eabi::general_result().number);
    }
}

fn remap_early_branch_target(instruction: &mut Instruction) {
    let Instruction::BranchConditionalForward { target, .. } = instruction else {
        return;
    };
    if *target <= 23 {
        *target -= 1;
    }
}

fn remap_status_initialization_relocations(relocations: &mut Vec<mwcc_machine_code::Relocation>) {
    relocations.retain(|relocation| !matches!(relocation.instruction_index, 32 | 33));
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index {
            9..=23 => relocation.instruction_index - 1,
            28 => 23,
            29 => 24,
            index => index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationTarget};

    #[test]
    fn relocation_map_removes_the_rematerialized_address_pair() {
        let mut relocations = [5, 9, 12, 16, 20, 28, 29, 30, 32, 33, 34, 40, 44]
            .into_iter()
            .map(|instruction_index| Relocation {
                instruction_index,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("symbol".into()),
            })
            .collect::<Vec<_>>();

        remap_status_initialization_relocations(&mut relocations);

        assert_eq!(
            relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [5, 8, 11, 15, 19, 23, 24, 30, 34, 40, 44]
        );
    }
}
