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
}
