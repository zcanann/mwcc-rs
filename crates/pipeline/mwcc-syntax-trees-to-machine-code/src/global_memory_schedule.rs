//! Alias-aware scheduling for direct small-data memory operations.
//!
//! The generic instruction scheduler treats every memory access as a barrier
//! because instructions alone do not prove aliasing. Text relocations retain
//! that missing identity: accesses to two different direct global symbols
//! cannot alias, so MWCC may issue a later load before an earlier store to hide
//! the load latency.

use mwcc_machine_code::{Instruction, Relocation, RelocationKind, RelocationTarget};
use mwcc_vreg::{register_operands, Class, RegisterRole};
use std::collections::HashSet;

/// Hoist adjacent direct-global loads ahead of independent direct-global
/// stores. Returns the old-index to new-index permutation.
pub(crate) fn hoist_independent_sda_loads(
    instructions: &mut [Instruction],
    relocations: &[Relocation],
) -> Vec<usize> {
    let mut permutation = (0..instructions.len()).collect::<Vec<_>>();
    let control_entries = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target < instructions.len() =>
            {
                Some(*target)
            }
            _ => None,
        })
        .collect::<HashSet<_>>();

    let mut store = 0;
    while store + 1 < instructions.len() {
        let load = store + 1;
        if control_entries.contains(&store) || control_entries.contains(&load) {
            store += 1;
            continue;
        }
        let Some(store_target) = external_sda_target(relocations, store) else {
            store += 1;
            continue;
        };
        let Some(load_target) = external_sda_target(relocations, load) else {
            store += 1;
            continue;
        };
        let Some(load_destination) = direct_load_destination(&instructions[load]) else {
            store += 1;
            continue;
        };
        let independent_registers = register_operands(&instructions[store])
            .iter()
            .all(|operand| {
                operand.class != load_destination.0 || operand.register != load_destination.1
            });
        if direct_store(&instructions[store])
            && store_target != load_target
            && independent_registers
        {
            instructions.swap(store, load);
            permutation[store] = load;
            permutation[load] = store;
            store += 2;
        } else {
            store += 1;
        }
    }
    permutation
}

/// Hoist an address high half over one independent store so the store fills
/// the address pair's result-latency slot.
///
/// The natural statement stream for `object->state = value; call(callback)` is
/// `store; lis callback@ha; addi callback@l`. Legacy MWCC issues the independent
/// `lis` first and leaves the store between the dependent address halves. The
/// relocation pair proves this is one address, while register and control-entry
/// checks make crossing the store safe.
pub(crate) fn hoist_address_highs_over_stores(
    instructions: &mut [Instruction],
    relocations: &[Relocation],
) -> Vec<usize> {
    let mut permutation = (0..instructions.len()).collect::<Vec<_>>();
    let control_entries = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target < instructions.len() =>
            {
                Some(*target)
            }
            _ => None,
        })
        .collect::<HashSet<_>>();

    let mut store = 0;
    while store + 3 < instructions.len() {
        let high = store + 1;
        let low = store + 2;
        if control_entries.contains(&store)
            || control_entries.contains(&high)
            || control_entries.contains(&low)
            || !direct_store(&instructions[store])
            || !matches!(
                instructions[low + 1],
                Instruction::BranchAndLink { .. }
            )
        {
            store += 1;
            continue;
        }
        let Instruction::AddImmediateShifted {
            d: high_register,
            a: 0,
            ..
        } = instructions[high]
        else {
            store += 1;
            continue;
        };
        let Instruction::AddImmediate {
            a: low_base, ..
        } = instructions[low]
        else {
            store += 1;
            continue;
        };
        if low_base != high_register
            || register_operands(&instructions[store]).iter().any(|operand| {
                operand.class == Class::General && operand.register == high_register
            })
            || external_address_target(relocations, high, RelocationKind::Addr16Ha)
                != external_address_target(relocations, low, RelocationKind::Addr16Lo)
            || external_address_target(relocations, high, RelocationKind::Addr16Ha).is_none()
        {
            store += 1;
            continue;
        }

        instructions.swap(store, high);
        permutation[store] = high;
        permutation[high] = store;
        store += 3;
    }
    permutation
}

/// Start a non-power-of-two global-array scale before its independent address
/// pair. Structured control-flow bodies reach the physical stream already
/// scheduled, so their `lis; mulli; addi; add` selection does not pass through
/// the generic list scheduler. MWCC issues the longer integer multiply first:
/// `mulli; lis; addi; add`.
///
/// The complete four-instruction dataflow and matching relocation pair prove
/// both the independence of the first two instructions and their ownership by
/// one address computation. Control-flow entry points remain immovable.
pub(crate) fn hoist_integer_scales_over_address_highs(
    instructions: &mut [Instruction],
    relocations: &[Relocation],
) -> Vec<usize> {
    let mut permutation = (0..instructions.len()).collect::<Vec<_>>();
    let control_entries = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::BranchConditionalForward { target, .. }
            | Instruction::Branch { target }
                if *target < instructions.len() =>
            {
                Some(*target)
            }
            _ => None,
        })
        .collect::<HashSet<_>>();

    let mut high = 0;
    while high + 3 < instructions.len() {
        let scale = high + 1;
        let low = high + 2;
        let sum = high + 3;
        if [high, scale, low, sum]
            .iter()
            .any(|index| control_entries.contains(index))
        {
            high += 1;
            continue;
        }
        let Instruction::AddImmediateShifted {
            d: high_register,
            a: 0,
            ..
        } = instructions[high]
        else {
            high += 1;
            continue;
        };
        let Instruction::MultiplyImmediate {
            d: scaled,
            a: scale_source,
            ..
        } = instructions[scale]
        else {
            high += 1;
            continue;
        };
        let Instruction::AddImmediate {
            d: base,
            a: low_base,
            ..
        } = instructions[low]
        else {
            high += 1;
            continue;
        };
        let Instruction::Add {
            a: sum_left,
            b: sum_right,
            ..
        } = instructions[sum]
        else {
            high += 1;
            continue;
        };
        let sum_consumes_both =
            (sum_left == base && sum_right == scaled)
                || (sum_left == scaled && sum_right == base);
        if low_base != high_register
            || !sum_consumes_both
            || [scaled, scale_source].contains(&high_register)
            || external_address_target(relocations, high, RelocationKind::Addr16Ha)
                != external_address_target(relocations, low, RelocationKind::Addr16Lo)
            || external_address_target(relocations, high, RelocationKind::Addr16Ha).is_none()
        {
            high += 1;
            continue;
        }

        instructions.swap(high, scale);
        permutation[high] = scale;
        permutation[scale] = high;
        high += 4;
    }
    permutation
}

fn external_address_target(
    relocations: &[Relocation],
    index: usize,
    kind: RelocationKind,
) -> Option<(&str, i32)> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != index || relocation.kind != kind {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(name) => Some((name.as_str(), 0)),
            RelocationTarget::ExternalWithAddend(name, addend) => {
                Some((name.as_str(), *addend))
            }
            _ => None,
        }
    })
}

fn external_sda_target(relocations: &[Relocation], index: usize) -> Option<&str> {
    relocations.iter().find_map(|relocation| {
        if relocation.instruction_index != index || relocation.kind != RelocationKind::EmbSda21 {
            return None;
        }
        match &relocation.target {
            RelocationTarget::External(name) | RelocationTarget::ExternalWithAddend(name, _) => {
                Some(name.as_str())
            }
            _ => None,
        }
    })
}

fn direct_store(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::StoreWord { .. }
            | Instruction::StoreByte { .. }
            | Instruction::StoreHalfword { .. }
            | Instruction::StoreFloatSingle { .. }
            | Instruction::StoreFloatDouble { .. }
    )
}

fn direct_load_destination(instruction: &Instruction) -> Option<(Class, u8)> {
    if !matches!(
        instruction,
        Instruction::LoadWord { .. }
            | Instruction::LoadByteZero { .. }
            | Instruction::LoadHalfwordZero { .. }
            | Instruction::LoadHalfwordAlgebraic { .. }
            | Instruction::LoadFloatSingle { .. }
            | Instruction::LoadFloatDouble { .. }
    ) {
        return None;
    }
    register_operands(instruction)
        .into_iter()
        .find(|operand| operand.role == RegisterRole::Define)
        .map(|operand| (operand.class, operand.register))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relocation(index: usize, target: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn later_distinct_global_load_issues_before_store() {
        let mut instructions = vec![
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
        ];

        let permutation = hoist_independent_sda_loads(
            &mut instructions,
            &[relocation(0, "state"), relocation(1, "argument")],
        );

        assert!(matches!(
            instructions[0],
            Instruction::LoadWord { d: 3, .. }
        ));
        assert_eq!(permutation, [1, 0]);
    }

    #[test]
    fn possible_alias_and_register_dependency_preserve_order() {
        let mut possible_alias = vec![
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
        ];
        assert_eq!(
            hoist_independent_sda_loads(
                &mut possible_alias,
                &[relocation(0, "word"), relocation(1, "word")],
            ),
            [0, 1],
        );

        let mut dependent = vec![
            Instruction::StoreWord {
                s: 3,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
        ];
        assert_eq!(
            hoist_independent_sda_loads(
                &mut dependent,
                &[relocation(0, "state"), relocation(1, "argument")],
            ),
            [0, 1],
        );
    }

    #[test]
    fn control_entry_preserves_memory_order() {
        let mut instructions = vec![
            Instruction::Branch { target: 1 },
            Instruction::StoreWord {
                s: 0,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 0,
                offset: 0,
            },
        ];

        assert_eq!(
            hoist_independent_sda_loads(
                &mut instructions,
                &[relocation(1, "state"), relocation(2, "argument")],
            ),
            [0, 1, 2],
        );
    }

    fn address_relocation(index: usize, kind: RelocationKind, target: &str) -> Relocation {
        Relocation {
            instruction_index: index,
            kind,
            target: RelocationTarget::External(target.into()),
        }
    }

    #[test]
    fn store_fills_a_following_address_pairs_latency_slot() {
        let mut instructions = vec![
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 12,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "wait".into(),
            },
        ];
        let relocations = [
            address_relocation(1, RelocationKind::Addr16Ha, "callback"),
            address_relocation(2, RelocationKind::Addr16Lo, "callback"),
        ];

        let permutation =
            hoist_address_highs_over_stores(&mut instructions, &relocations);

        assert!(matches!(
            instructions[0],
            Instruction::AddImmediateShifted { d: 3, .. }
        ));
        assert!(matches!(
            instructions[1],
            Instruction::StoreWord { s: 0, a: 4, .. }
        ));
        assert_eq!(permutation, [1, 0, 2, 3]);
    }

    #[test]
    fn dependent_store_and_control_entry_preserve_address_order() {
        let relocations = [
            address_relocation(1, RelocationKind::Addr16Ha, "callback"),
            address_relocation(2, RelocationKind::Addr16Lo, "callback"),
        ];
        let mut dependent = vec![
            Instruction::StoreWord {
                s: 3,
                a: 4,
                offset: 12,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "wait".into(),
            },
        ];
        assert_eq!(
            hoist_address_highs_over_stores(&mut dependent, &relocations),
            [0, 1, 2, 3],
        );

        let mut control_entry = vec![
            Instruction::Branch { target: 1 },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 12,
            },
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: 0,
            },
            Instruction::BranchAndLink {
                target: "wait".into(),
            },
        ];
        let relocations = [
            address_relocation(2, RelocationKind::Addr16Ha, "callback"),
            address_relocation(3, RelocationKind::Addr16Lo, "callback"),
        ];
        assert_eq!(
            hoist_address_highs_over_stores(&mut control_entry, &relocations),
            [0, 1, 2, 3, 4],
        );
    }

    #[test]
    fn integer_scale_starts_before_an_independent_global_address_pair() {
        let mut instructions = vec![
            Instruction::AddImmediateShifted {
                d: 3,
                a: 0,
                immediate: 0,
            },
            Instruction::MultiplyImmediate {
                d: 4,
                a: 28,
                immediate: 80,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 3,
                immediate: 0,
            },
            Instruction::Add {
                d: 29,
                a: 0,
                b: 4,
            },
        ];
        let relocations = [
            address_relocation(0, RelocationKind::Addr16Ha, "records"),
            address_relocation(2, RelocationKind::Addr16Lo, "records"),
        ];

        let permutation =
            hoist_integer_scales_over_address_highs(&mut instructions, &relocations);

        assert!(matches!(instructions[0], Instruction::MultiplyImmediate { .. }));
        assert!(matches!(instructions[1], Instruction::AddImmediateShifted { .. }));
        assert_eq!(permutation, [1, 0, 2, 3]);
    }
}
