//! Overlap a constant division's high half with its direct-global dividend load.
//!
//! Structured bodies already own their instruction order. This small schedule
//! still runs before allocation so the constant and retained dividend receive
//! homes in the same definition order as MWCC.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, Relocation, RelocationKind, RelocationTarget};
use mwcc_versions::Optimization;
use mwcc_vreg::{register_operands, Class};
use std::collections::HashSet;

impl Generator {
    pub(crate) fn schedule_constant_division_loads(&mut self) {
        if !self.behavior.scheduler_enabled
            || !matches!(
                self.behavior.optimization,
                Optimization::O2 | Optimization::O3 | Optimization::O4
            )
            || !self.output.jump_tables.is_empty()
            || self.output.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::VerbatimWord { .. } | Instruction::BranchToCountRegister
                )
            })
        {
            return;
        }
        let permutation = hoist_constant_pairs_over_addresses(
            &mut self.output.instructions,
            &self.output.relocations,
            &self.volatile_globals,
        );
        crate::remap_instruction_indices(self, &permutation);
        let permutation = hoist_constant_highs(
            &mut self.output.instructions,
            &self.output.relocations,
            &self.volatile_globals,
        );
        crate::remap_instruction_indices(self, &permutation);
    }
}

/// A retained output address is independent of the first dividend's constant.
/// Form the complete constant first so its short-lived high half can share the
/// address's eventual home. Physical register selection still belongs to the
/// allocator; this pass changes only dependency-safe instruction order.
fn hoist_constant_pairs_over_addresses(
    instructions: &mut [Instruction],
    relocations: &[Relocation],
    volatile: &HashSet<String>,
) -> Vec<usize> {
    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    let entries: HashSet<_> = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    for start in 0..instructions.len().saturating_sub(5) {
        if (start + 1..=start + 5).any(|i| entries.contains(&i)) {
            continue;
        }
        let [Instruction::AddImmediateShifted { d: base, a: 0, .. }, Instruction::AddImmediate {
            d: address,
            a: base_input,
            ..
        }, Instruction::LoadWord { d: dividend, .. }, Instruction::AddImmediateShifted { d: high, a: 0, .. }, Instruction::AddImmediate {
            d: constant,
            a: high_input,
            ..
        }, multiply] = &instructions[start..start + 6]
        else {
            continue;
        };
        if base != address
            || base != base_input
            || high != high_input
            || !mwcc_vreg::Reg::is_virtual_field(*base)
            || !mwcc_vreg::Reg::is_virtual_field(*high)
            || base == high
            || base == constant
            || constant == dividend
            || register_operands(&instructions[start + 2])
                .iter()
                .any(|operand| {
                    operand.class == Class::General
                        && (operand.register == *high || operand.register == *constant)
                })
            || !matches!(multiply,
                Instruction::MultiplyHighWord { a, b, .. } | Instruction::MultiplyHighWordUnsigned { a, b, .. }
                if a == constant && b == dividend)
        {
            continue;
        }
        let external = |index, kind| {
            relocations.iter().find_map(|r| {
                if r.instruction_index != index || r.kind != kind {
                    return None;
                }
                match &r.target {
                    RelocationTarget::External(name) => Some(name),
                    _ => None,
                }
            })
        };
        let Some(global) = external(start, RelocationKind::Addr16Ha) else {
            continue;
        };
        if external(start + 1, RelocationKind::Addr16Lo) != Some(global)
            || !external(start + 2, RelocationKind::EmbSda21)
                .is_some_and(|name| !volatile.contains(name))
            || relocations
                .iter()
                .any(|r| (start + 3..=start + 5).contains(&r.instruction_index))
        {
            continue;
        }
        // Branch entries execute the whole packet; metadata follows its owner.
        for instruction in instructions.iter_mut() {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == start =>
                {
                    *target = start + 3
                }
                _ => {}
            }
        }
        instructions[start..start + 5].rotate_right(2);
        permutation[start..start + 5].rotate_left(2);
    }
    permutation
}

fn hoist_constant_highs(
    instructions: &mut [Instruction],
    relocations: &[Relocation],
    volatile: &HashSet<String>,
) -> Vec<usize> {
    let mut permutation: Vec<_> = (0..instructions.len()).collect();
    let entries: HashSet<_> = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    let relocated: HashSet<_> = relocations.iter().map(|r| r.instruction_index).collect();
    for load in 0..instructions.len().saturating_sub(3) {
        let (high, low, multiply) = (load + 1, load + 2, load + 3);
        if [high, low, multiply]
            .iter()
            .any(|i| entries.contains(i) || relocated.contains(i))
        {
            continue;
        }
        let Some(name) = relocations.iter().find_map(|r| {
            if r.instruction_index != load || r.kind != RelocationKind::EmbSda21 {
                return None;
            }
            match &r.target {
                RelocationTarget::External(name) => Some(name),
                _ => None,
            }
        }) else {
            continue;
        };
        if volatile.contains(name) {
            continue;
        }
        let Instruction::LoadWord { d: dividend, .. } = instructions[load] else {
            continue;
        };
        let Instruction::AddImmediateShifted {
            d: constant_high,
            a: 0,
            ..
        } = instructions[high]
        else {
            continue;
        };
        let Instruction::AddImmediate {
            d: constant,
            a: high_input,
            ..
        } = instructions[low]
        else {
            continue;
        };
        if high_input != constant_high
            || constant == dividend
            || register_operands(&instructions[load])
                .iter()
                .any(|operand| operand.class == Class::General && operand.register == constant_high)
            || !matches!(instructions[multiply],
                Instruction::MultiplyHighWord { a, b, .. } | Instruction::MultiplyHighWordUnsigned { a, b, .. }
                    if a == constant && b == dividend)
        {
            continue;
        }
        // An incoming edge to the old load must execute the whole reordered
        // pair. Relocations, in contrast, continue to follow the load itself.
        for instruction in instructions.iter_mut() {
            match instruction {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. }
                    if *target == load =>
                {
                    *target = high
                }
                _ => {}
            }
        }
        instructions.swap(load, high);
        permutation.swap(load, high);
    }
    permutation
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence() -> Vec<Instruction> {
        vec![
            Instruction::LoadWord {
                d: 5,
                a: 13,
                offset: 0,
            },
            Instruction::AddImmediateShifted {
                d: 4,
                a: 0,
                immediate: 0x6666,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 4,
                immediate: 0x6667,
            },
            Instruction::MultiplyHighWord { d: 0, a: 0, b: 5 },
        ]
    }
    fn relocation(index: usize) -> Relocation {
        Relocation {
            instruction_index: index,
            kind: RelocationKind::EmbSda21,
            target: RelocationTarget::External("input".into()),
        }
    }

    fn address_sequence() -> Vec<Instruction> {
        vec![
            Instruction::AddImmediateShifted {
                d: 32,
                a: 0,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 0,
            },
            Instruction::LoadWord {
                d: 33,
                a: 0,
                offset: 0,
            },
            Instruction::AddImmediateShifted {
                d: 34,
                a: 0,
                immediate: 0x6666,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 34,
                immediate: 0x6667,
            },
            Instruction::MultiplyHighWord { d: 0, a: 0, b: 33 },
        ]
    }

    fn address_relocations() -> Vec<Relocation> {
        vec![
            Relocation {
                instruction_index: 0,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("packet".into()),
            },
            Relocation {
                instruction_index: 1,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::External("packet".into()),
            },
            relocation(2),
        ]
    }

    #[test]
    fn complete_constants_lead_addresses_and_relocations_follow_their_owners() {
        let mut output = mwcc_machine_code::MachineFunction::default();
        output.instructions = address_sequence();
        output.instructions.push(Instruction::Branch { target: 0 });
        output.relocations = address_relocations();
        let permutation = hoist_constant_pairs_over_addresses(
            &mut output.instructions,
            &output.relocations,
            &HashSet::new(),
        );
        assert_eq!(permutation, [2, 3, 4, 0, 1, 5, 6]);
        crate::remap_machine_function_indices(&mut output, &permutation);
        assert!(matches!(
            output.instructions[0],
            Instruction::AddImmediateShifted {
                immediate: 0x6666,
                ..
            }
        ));
        assert!(matches!(
            output.instructions[6],
            Instruction::Branch { target: 0 }
        ));
        assert_eq!(
            output
                .relocations
                .iter()
                .map(|r| r.instruction_index)
                .collect::<Vec<_>>(),
            [2, 3, 4]
        );
        assert!(matches!(
            output.instructions[output.relocations[2].instruction_index],
            Instruction::LoadWord { .. }
        ));
    }

    #[test]
    fn address_packet_internal_entries_and_symbol_mismatches_prevent_hoisting() {
        for entry in 1..=5 {
            let mut instructions = address_sequence();
            instructions.push(Instruction::Branch { target: entry });
            assert_eq!(
                hoist_constant_pairs_over_addresses(
                    &mut instructions,
                    &address_relocations(),
                    &HashSet::new()
                ),
                (0..7).collect::<Vec<_>>()
            );
        }
        let mut relocations = address_relocations();
        relocations[1].target = RelocationTarget::External("other".into());
        assert_eq!(
            hoist_constant_pairs_over_addresses(
                &mut address_sequence(),
                &relocations,
                &HashSet::new()
            ),
            (0..6).collect::<Vec<_>>()
        );
        relocations = address_relocations();
        relocations.push(relocation(3));
        assert_eq!(
            hoist_constant_pairs_over_addresses(
                &mut address_sequence(),
                &relocations,
                &HashSet::new()
            ),
            (0..6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn constant_address_and_dividend_dependencies_remain_ordered() {
        for (index, instruction) in [
            (
                2,
                Instruction::LoadWord {
                    d: 34,
                    a: 0,
                    offset: 0,
                },
            ),
            (
                2,
                Instruction::LoadWord {
                    d: 33,
                    a: 34,
                    offset: 0,
                },
            ),
            (
                4,
                Instruction::AddImmediate {
                    d: 32,
                    a: 34,
                    immediate: 7,
                },
            ),
        ] {
            let mut instructions = address_sequence();
            instructions[index] = instruction;
            assert_eq!(
                hoist_constant_pairs_over_addresses(
                    &mut instructions,
                    &address_relocations(),
                    &HashSet::new()
                ),
                (0..6).collect::<Vec<_>>()
            );
        }
        assert_eq!(
            hoist_constant_pairs_over_addresses(
                &mut address_sequence(),
                &address_relocations(),
                &["input".into()].into_iter().collect()
            ),
            (0..6).collect::<Vec<_>>()
        );
    }

    #[test]
    fn load_owns_its_relocation_but_entry_edges_execute_the_high_half() {
        let mut instructions = sequence();
        instructions.insert(0, Instruction::Branch { target: 1 });
        let permutation =
            hoist_constant_highs(&mut instructions, &[relocation(1)], &HashSet::new());
        assert_eq!(permutation, [0, 2, 1, 3, 4]);
        assert!(matches!(
            instructions[1],
            Instruction::AddImmediateShifted { .. }
        ));
        let Instruction::Branch { target } = instructions[0] else {
            panic!()
        };
        assert_eq!(permutation[target], 1);
        assert_eq!(permutation[1], 2); // The dividend relocation follows lwz.
    }

    #[test]
    fn internal_entries_and_relocated_constants_are_not_reordered() {
        for entry in 1..=3 {
            let mut instructions = sequence();
            instructions.push(Instruction::Branch { target: entry });
            assert_eq!(
                hoist_constant_highs(&mut instructions, &[relocation(0)], &HashSet::new()),
                [0, 1, 2, 3, 4]
            );
        }
        let mut instructions = sequence();
        assert_eq!(
            hoist_constant_highs(
                &mut instructions,
                &[relocation(0), relocation(1)],
                &HashSet::new()
            ),
            [0, 1, 2, 3]
        );
    }

    #[test]
    fn volatile_inputs_address_dependencies_and_unrelated_products_are_excluded() {
        let mut instructions = sequence();
        assert_eq!(
            hoist_constant_highs(
                &mut instructions,
                &[relocation(0)],
                &["input".into()].into_iter().collect()
            ),
            [0, 1, 2, 3]
        );
        for replacement in [
            Instruction::LoadWord {
                d: 4,
                a: 13,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 5,
                a: 4,
                offset: 0,
            },
        ] {
            let mut instructions = sequence();
            instructions[0] = replacement;
            assert_eq!(
                hoist_constant_highs(&mut instructions, &[relocation(0)], &HashSet::new()),
                [0, 1, 2, 3]
            );
        }
        let mut instructions = sequence();
        instructions[3] = Instruction::MultiplyHighWord { d: 0, a: 0, b: 6 };
        assert_eq!(
            hoist_constant_highs(&mut instructions, &[relocation(0)], &HashSet::new()),
            [0, 1, 2, 3]
        );
    }
}
