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
        let permutation = hoist_constant_highs(
            &mut self.output.instructions,
            &self.output.relocations,
            &self.volatile_globals,
        );
        crate::remap_instruction_indices(self, &permutation);
    }
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
