//! Fold retained object addresses into integer memory displacements before allocation.
//!
//! A captured inline pointer may own a virtual home even though every use is a
//! load/store base. Keep the object base live instead when the address has one
//! dominating definition and neither the base nor the displacement can change.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_vreg::{Class, Reg, RegisterRole, register_operands};
use std::collections::HashSet;

impl Generator {
    pub(crate) fn fold_retained_member_displacements(&mut self) {
        let Some(base) = self
            .structured_global_base_cache
            .as_ref()
            .map(|cache| cache.register)
        else {
            return;
        };
        if !Reg::is_virtual_field(base) || !self.output.jump_tables.is_empty() {
            return;
        }
        for definition in (0..self.output.instructions.len()).rev() {
            let relocated: HashSet<_> = self
                .output
                .relocations
                .iter()
                .map(|r| r.instruction_index)
                .collect();
            if let Some(replacements) =
                fold_plan(&self.output.instructions, &relocated, base, definition)
            {
                for (index, instruction) in replacements {
                    self.output.instructions[index] = instruction;
                }
                crate::remove_instruction_retargeting_to_next(self, definition);
            }
        }
    }
}

fn memory_displacement(instruction: &mut Instruction) -> Option<(&mut u8, &mut i16)> {
    match instruction {
        Instruction::LoadWord { a, offset, .. }
        | Instruction::LoadByteZero { a, offset, .. }
        | Instruction::LoadHalfwordZero { a, offset, .. }
        | Instruction::LoadHalfwordAlgebraic { a, offset, .. }
        | Instruction::StoreWord { a, offset, .. }
        | Instruction::StoreByte { a, offset, .. }
        | Instruction::StoreHalfword { a, offset, .. } => Some((a, offset)),
        _ => None,
    }
}

fn fold_plan(
    instructions: &[Instruction],
    relocated: &HashSet<usize>,
    base: u8,
    definition: usize,
) -> Option<Vec<(usize, Instruction)>> {
    let (alias, displacement) = match instructions.get(definition)? {
        Instruction::AddImmediate { d, a, immediate } if *a == base => (*d, *immediate),
        Instruction::Or { a, s, b } if *s == base && *b == base => (*a, 0),
        _ => return None,
    };
    if !Reg::is_virtual_field(alias) || alias == base || relocated.contains(&definition) {
        return None;
    }
    let mut replacements = Vec::new();
    for (index, instruction) in instructions.iter().enumerate() {
        // No opaque flow or back edges: lexical intervals can then prove that
        // no branch bypasses the sole address definition on its way to a use.
        match instruction {
            Instruction::VerbatimWord(_) | Instruction::BranchToCountRegister => return None,
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. }
                if *target <= index =>
            {
                return None;
            }
            _ => {}
        }
        if index == definition {
            continue;
        }
        let operands = register_operands(instruction);
        let alias_operands: Vec<_> = operands
            .iter()
            .filter(|o| o.class == Class::General && o.register == alias)
            .collect();
        if alias_operands.is_empty() {
            continue;
        }
        if index < definition
            || alias_operands.len() != 1
            || alias_operands[0].role != RegisterRole::Use
            || relocated.contains(&index)
        {
            return None;
        }
        let mut replacement = instruction.clone();
        let (address, offset) = memory_displacement(&mut replacement)?;
        if *address != alias {
            return None;
        }
        *offset = offset.checked_add(displacement)?;
        *address = base;
        replacements.push((index, replacement));
    }
    let last_use = replacements.last()?.0;
    for (index, instruction) in instructions.iter().enumerate() {
        if index > definition && index <= last_use {
            let writes_base = register_operands(instruction).iter().any(|operand| {
                operand.class == Class::General
                    && operand.register == base
                    && operand.role == RegisterRole::Define
            });
            // The allocator description treats stwu as a pinned stack update.
            // Account explicitly for its architectural base definition here.
            let updates_base = matches!(instruction,
                Instruction::StoreWordWithUpdate { a, .. } if *a == base);
            if writes_base || updates_base {
                return None;
            }
        }
        if let Instruction::Branch { target }
        | Instruction::BranchConditionalForward { target, .. } = instruction
        {
            if index < definition && *target > definition && *target <= last_use {
                return None;
            }
        }
    }
    Some(replacements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address() -> Instruction {
        Instruction::AddImmediate {
            d: 33,
            a: 32,
            immediate: 6,
        }
    }

    #[test]
    fn preserves_memory_widths_and_signed_offsets_through_both_arms() {
        let instructions = vec![
            address(),
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 4,
            },
            Instruction::StoreWord {
                s: 3,
                a: 33,
                offset: 4,
            },
            Instruction::Branch { target: 6 },
            Instruction::LoadHalfwordAlgebraic {
                d: 3,
                a: 33,
                offset: -2,
            },
            Instruction::StoreByte {
                s: 3,
                a: 33,
                offset: 1,
            },
            Instruction::BranchToLinkRegister,
        ];
        assert_eq!(
            fold_plan(&instructions, &HashSet::new(), 32, 0),
            Some(vec![
                (
                    2,
                    Instruction::StoreWord {
                        s: 3,
                        a: 32,
                        offset: 10
                    }
                ),
                (
                    4,
                    Instruction::LoadHalfwordAlgebraic {
                        d: 3,
                        a: 32,
                        offset: 4
                    }
                ),
                (
                    5,
                    Instruction::StoreByte {
                        s: 3,
                        a: 32,
                        offset: 7
                    }
                ),
            ])
        );
    }

    #[test]
    fn rejects_escapes_redefinitions_update_forms_and_displacement_overflow() {
        let store = Instruction::StoreWord {
            s: 3,
            a: 33,
            offset: 0,
        };
        for middle in [
            Instruction::move_register(3, 33),
            Instruction::AddImmediate {
                d: 33,
                a: 32,
                immediate: 8,
            },
            Instruction::AddImmediate {
                d: 32,
                a: 32,
                immediate: 8,
            },
            Instruction::StoreWordWithUpdate {
                s: 3,
                a: 32,
                offset: 8,
            },
            Instruction::LoadWordWithUpdate {
                d: 3,
                a: 33,
                offset: 8,
            },
            Instruction::StoreWord {
                s: 33,
                a: 33,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 3,
                a: 33,
                offset: i16::MAX,
            },
        ] {
            assert!(
                fold_plan(&[address(), middle, store.clone()], &HashSet::new(), 32, 0).is_none()
            );
        }
        for relocated in [HashSet::from([0]), HashSet::from([1])] {
            assert!(fold_plan(&[address(), store.clone()], &relocated, 32, 0).is_none());
        }
    }

    #[test]
    fn rejects_a_bypassed_definition_and_back_edges() {
        let store = Instruction::StoreWord {
            s: 3,
            a: 33,
            offset: 0,
        };
        let instructions = vec![
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 3,
            },
            address(),
            store.clone(),
            store.clone(),
        ];
        assert!(fold_plan(&instructions, &HashSet::new(), 32, 1).is_none());
        assert!(
            fold_plan(
                &[address(), store, Instruction::Branch { target: 0 }],
                &HashSet::new(),
                32,
                0
            )
            .is_none()
        );
    }

    #[test]
    fn folds_zero_bias_without_conflating_integer_and_float_registers() {
        let instructions = vec![
            Instruction::move_register(33, 32),
            Instruction::FloatMove { d: 33, b: 3 },
            Instruction::LoadByteZero {
                d: 3,
                a: 33,
                offset: -1,
            },
        ];
        assert_eq!(
            fold_plan(&instructions, &HashSet::new(), 32, 0),
            Some(vec![(
                2,
                Instruction::LoadByteZero {
                    d: 3,
                    a: 32,
                    offset: -1
                }
            ),])
        );
    }
}
