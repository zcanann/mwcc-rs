//! Recover local indirect CFG edges from selected jump-table relocations.
//!
//! Table addresses can be hoisted or copied before dispatch. Propagate their
//! identities through register definitions, including the indexed target load,
//! and attach the corresponding table entries to each consuming bctr.

use crate::{analyze_with_indirect_successors, register_operands, Class, Liveness, RegisterRole};
use mwcc_machine_code::{Instruction, JumpTable, Relocation, RelocationTarget};
use std::collections::{HashMap, HashSet};

pub fn analyze_with_jump_tables(
    instructions: &[Instruction],
    relocations: &[Relocation],
    tables: &[JumpTable],
) -> Liveness {
    let successors = jump_table_successors(instructions, relocations, tables);
    analyze_with_indirect_successors(instructions, &successors)
}

/// Include both indirect CFG edges and the function's ABI result uses.
pub fn analyze_with_jump_tables_and_return_registers(
    instructions: &[Instruction],
    relocations: &[Relocation],
    tables: &[JumpTable],
    return_registers: &[(Class, u8)],
) -> Liveness {
    let successors = jump_table_successors(instructions, relocations, tables);
    crate::analyze_with_return_registers(instructions, &successors, return_registers)
}

/// Combine recovered indirect CFG edges with explicit ABI input uses.
pub fn analyze_with_jump_tables_and_abi_uses(
    instructions: &[Instruction],
    relocations: &[Relocation],
    tables: &[JumpTable],
    return_registers: &[(Class, u8)],
    instruction_uses: &HashMap<usize, Vec<(Class, u8)>>,
) -> Liveness {
    let successors = jump_table_successors(instructions, relocations, tables);
    crate::analyze_with_abi_uses(instructions, &successors, return_registers, instruction_uses)
}

fn jump_table_successors(
    instructions: &[Instruction],
    relocations: &[Relocation],
    tables: &[JumpTable],
) -> HashMap<usize, Vec<usize>> {
    let mut references = HashMap::<usize, HashSet<usize>>::new();
    for relocation in relocations {
        let table = match relocation.target {
            RelocationTarget::JumpTable => 0,
            RelocationTarget::JumpTableAt(index) => index,
            _ => continue,
        };
        if table < tables.len() {
            references
                .entry(relocation.instruction_index)
                .or_default()
                .insert(table);
        }
    }
    let mut registers = HashMap::<mwcc_machine_code::RegisterField, HashSet<usize>>::new();
    let mut count_register = HashSet::new();
    let mut successors = HashMap::new();
    for (index, instruction) in instructions.iter().enumerate() {
        let operands = register_operands(instruction);
        let mut origins = references.get(&index).cloned().unwrap_or_default();
        for operand in &operands {
            if operand.class == Class::General && operand.role == RegisterRole::Use {
                if let Some(tables) = registers.get(&operand.register) {
                    origins.extend(tables);
                }
            }
        }
        for operand in &operands {
            if operand.class == Class::General && operand.role == RegisterRole::Define {
                registers.insert(operand.register, origins.clone());
            }
        }
        match instruction {
            Instruction::MoveToCountRegister { s } => {
                count_register = registers.get(s).cloned().unwrap_or_default();
            }
            Instruction::BranchToCountRegister if !count_register.is_empty() => {
                let mut targets: Vec<_> = count_register
                    .iter()
                    .flat_map(|table| tables[*table].entries.iter())
                    .map(|offset| *offset as usize / 4)
                    .collect();
                targets.sort_unstable();
                targets.dedup();
                successors.insert(index, targets);
            }
            Instruction::BranchAndLink { .. }
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::BranchToLinkRegisterAndLink => {
                for register in [0].into_iter().chain(3..=12) {
                    registers.remove(&register);
                }
                count_register.clear();
            }
            _ => {}
        }
    }
    successors
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::RelocationKind;

    #[test]
    fn arm_inputs_are_live_across_the_indirect_dispatch() {
        let code = [
            Instruction::load_immediate(32, 7),
            Instruction::load_immediate_shifted(33, 0),
            Instruction::AddImmediate {
                d: 33,
                a: 33,
                immediate: 0,
            },
            Instruction::LoadWordIndexed { d: 0, a: 33, b: 4 },
            Instruction::MoveToCountRegister { s: 0 },
            Instruction::BranchToCountRegister,
            Instruction::Add { d: 3, a: 32, b: 3 },
            Instruction::BranchToLinkRegister,
            Instruction::Add { d: 3, a: 32, b: 5 },
            Instruction::BranchToLinkRegister,
        ];
        let relocations = [Relocation {
            instruction_index: 1,
            kind: RelocationKind::Addr16Ha,
            target: RelocationTarget::JumpTable,
        }];
        let tables = [JumpTable {
            entries: vec![24, 32],
            anonymous_offset: 0,
        }];
        let live = analyze_with_jump_tables(&code, &relocations, &tables);
        for register in [3, 5] {
            let pinned = live
                .pinned
                .iter()
                .find(|p| p.class == Class::General && p.register == register)
                .unwrap();
            assert!(pinned.live_slots.as_ref().unwrap().contains(&10));
        }
        assert!(live
            .intervals
            .iter()
            .find(|p| p.vreg == crate::VirtualRegister::new(0, Class::General))
            .unwrap()
            .live_slots
            .as_ref()
            .unwrap()
            .contains(&11));
    }

    #[test]
    fn hoisted_table_addresses_keep_distinct_dispatch_targets() {
        let code = [
            Instruction::load_immediate_shifted(32, 0),
            Instruction::load_immediate_shifted(33, 0),
            Instruction::move_register(34, 32),
            Instruction::LoadWordIndexed { d: 0, a: 34, b: 3 },
            Instruction::MoveToCountRegister { s: 0 },
            Instruction::BranchToCountRegister,
            Instruction::LoadWordIndexed { d: 0, a: 33, b: 4 },
            Instruction::MoveToCountRegister { s: 0 },
            Instruction::BranchToCountRegister,
            Instruction::MoveToCountRegister { s: 5 },
            Instruction::BranchToCountRegister,
            Instruction::BranchToLinkRegister,
        ];
        let relocations = [
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::JumpTable,
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::JumpTableAt(1),
            },
        ];
        let tables = [
            JumpTable {
                entries: vec![24, 24],
                anonymous_offset: 0,
            },
            JumpTable {
                entries: vec![44],
                anonymous_offset: 0,
            },
        ];
        assert_eq!(
            jump_table_successors(&code, &relocations, &tables),
            HashMap::from([(5, vec![6]), (8, vec![11])])
        );
    }
}
