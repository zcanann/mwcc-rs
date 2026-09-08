//! Separate a global update's temporary from a pending narrow-store negation.
//!
//! r0 reuse hides the independence of these two expression results. Recover
//! the update value's own virtual home before moving the negation; memory
//! operations keep their source order and the allocator owns register reuse.

use crate::generator::Generator;
use mwcc_machine_code::{Instruction, Relocation, RelocationKind, RelocationTarget};
use mwcc_versions::{NegatedUpdateScheduleStyle, Optimization};
use mwcc_vreg::{Class, Reg};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Placement {
    BeforeMultiply,
    BeforeSubtract,
    AfterSubtract,
}

impl Generator {
    pub(crate) fn schedule_negated_global_updates(&mut self) {
        if !self.behavior.scheduler_enabled
            || !matches!(
                self.behavior.optimization,
                Optimization::O2 | Optimization::O3 | Optimization::O4
            )
            || !self.output.jump_tables.is_empty()
            || self.output.instructions.iter().any(|i| {
                matches!(
                    i,
                    Instruction::VerbatimWord(_) | Instruction::BranchToCountRegister
                )
            })
        {
            return;
        }
        let original_liveness = mwcc_vreg::analyze(&self.output.instructions);
        let mut permutation: Vec<_> = (0..self.output.instructions.len()).collect();
        for start in 0..self.output.instructions.len().saturating_sub(5) {
            let Some(placement) = plan(
                &self.output.instructions,
                &self.output.relocations,
                start,
                self.behavior.negated_update_schedule_style,
            ) else {
                continue;
            };
            let Instruction::MultiplyImmediate {
                d: original_product,
                ..
            } = self.output.instructions[start]
            else {
                unreachable!()
            };
            let product_owned =
                dead_after_subtraction(&original_liveness, original_product, start + 2);
            let result = self.fresh_virtual_general();
            // A load and the subtraction result are distinct values, even when
            // allocation eventually gives them the same physical home.
            let loaded = if placement != Placement::AfterSubtract && product_owned {
                self.fresh_virtual_general()
            } else {
                result
            };
            let product = if product_owned && !Reg::is_virtual_field(original_product) {
                let register = self.fresh_virtual_general();
                if let Instruction::MultiplyImmediate { d, .. } =
                    &mut self.output.instructions[start]
                {
                    *d = register;
                }
                register
            } else {
                original_product
            };
            if placement != Placement::AfterSubtract {
                if let Instruction::LoadWord { d, .. } = &mut self.output.instructions[start + 1] {
                    *d = loaded;
                }
            }
            if let Instruction::SubtractFrom { d, a, b } = &mut self.output.instructions[start + 2]
            {
                *d = result;
                *a = product;
                if placement != Placement::AfterSubtract {
                    *b = loaded;
                }
            }
            if let Instruction::StoreWord { s, .. } = &mut self.output.instructions[start + 3] {
                *s = result;
            }
            if product_owned {
                let mut group = vec![Reg::from_field(result, Class::General)
                    .virtual_register()
                    .unwrap()];
                if placement != Placement::AfterSubtract {
                    group.push(
                        Reg::from_field(loaded, Class::General)
                            .virtual_register()
                            .unwrap(),
                    );
                }
                group.push(
                    Reg::from_field(product, Class::General)
                        .virtual_register()
                        .unwrap(),
                );
                self.consumer_allocation_groups.push(group);
            }
            let destination = start
                + match placement {
                    Placement::BeforeMultiply => 0,
                    Placement::BeforeSubtract => 2,
                    Placement::AfterSubtract => 3,
                };
            let negation = start + 4;
            // Move the block entry with the complete reordered expression;
            // instruction-owned relocations still follow their own operation.
            for instruction in &mut self.output.instructions {
                match instruction {
                    Instruction::Branch { target }
                    | Instruction::BranchConditionalForward { target, .. }
                        if *target == destination =>
                    {
                        *target = negation
                    }
                    _ => {}
                }
            }
            self.output.instructions[destination..=negation].rotate_right(1);
            for old in destination..negation {
                permutation[old] = old + 1;
            }
            permutation[negation] = destination;
        }
        crate::remap_instruction_indices(self, &permutation);
    }
}

/// A physical temporary may become virtual only if its value has no later
/// consumer. CFG occupancy includes uses reached through branches and calls.
fn dead_after_subtraction(
    liveness: &mwcc_vreg::Liveness,
    register: u8,
    subtraction: usize,
) -> bool {
    let slots = match Reg::from_field(register, Class::General) {
        Reg::Virtual(register) => liveness
            .intervals
            .iter()
            .find(|i| i.vreg == register)
            .and_then(|i| i.live_slots.as_ref()),
        Reg::Physical(register) => liveness
            .pinned
            .iter()
            .find(|p| p.class == Class::General && p.register == register)
            .and_then(|p| p.live_slots.as_ref()),
    };
    slots.is_some_and(|slots| slots.binary_search(&(2 * subtraction + 1)).is_err())
}

fn sda_target(relocations: &[Relocation], index: usize) -> Option<&str> {
    relocations.iter().find_map(|r| {
        if r.instruction_index != index || r.kind != RelocationKind::EmbSda21 {
            return None;
        }
        match &r.target {
            RelocationTarget::External(name) => Some(name.as_str()),
            _ => None,
        }
    })
}

fn plan(
    instructions: &[Instruction],
    relocations: &[Relocation],
    start: usize,
    style: NegatedUpdateScheduleStyle,
) -> Option<Placement> {
    let Instruction::MultiplyImmediate {
        d: product,
        a: delta,
        ..
    } = *instructions.get(start)?
    else {
        return None;
    };
    if product == 0 || delta == 0 || product == delta {
        return None;
    }
    let Instruction::LoadWord {
        d: 0,
        a: load_base,
        offset: load_offset,
    } = *instructions.get(start + 1)?
    else {
        return None;
    };
    if !matches!(instructions.get(start + 2), Some(Instruction::SubtractFrom { d: 0, a, b: 0 }) if *a == product)
    {
        return None;
    }
    let Instruction::StoreWord {
        s: 0,
        a: store_base,
        offset: store_offset,
    } = *instructions.get(start + 3)?
    else {
        return None;
    };
    if (load_base, load_offset) != (store_base, store_offset)
        || load_base == product
        || load_base == delta
    {
        return None;
    }
    let name = sda_target(relocations, start + 1)?;
    if sda_target(relocations, start + 3)? != name {
        return None;
    }
    if !matches!(instructions.get(start + 4), Some(Instruction::Negate { d: 0, a }) if *a == delta)
    {
        return None;
    }
    let store = match instructions.get(start + 5)? {
        Instruction::ExtendSignByte { a: 0, s: 0 }
        | Instruction::ExtendSignHalfword { a: 0, s: 0 } => start + 6,
        _ => start + 5,
    };
    if !matches!(instructions.get(store), Some(Instruction::StoreHalfword { s: 0, a, .. } | Instruction::StoreByte { s: 0, a, .. }) if *a != 0)
    {
        return None;
    }
    let entries: HashSet<_> = instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => Some(*target),
            _ => None,
        })
        .collect();
    if (start + 1..=store).any(|i| entries.contains(&i))
        || relocations.iter().any(|r| {
            (start..=store).contains(&r.instruction_index)
                && r.instruction_index != start + 1
                && r.instruction_index != start + 3
        })
    {
        return None;
    }
    if start > 0 && matches!(instructions[start - 1], Instruction::StoreWord { .. }) {
        Some(Placement::BeforeMultiply)
    } else {
        Some(match style {
            NegatedUpdateScheduleStyle::AfterSubtract => Placement::AfterSubtract,
            NegatedUpdateScheduleStyle::BeforeSubtract => Placement::BeforeSubtract,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence() -> Vec<Instruction> {
        vec![
            Instruction::MultiplyImmediate {
                d: 5,
                a: 4,
                immediate: 160,
            },
            Instruction::LoadWord {
                d: 0,
                a: 13,
                offset: 0,
            },
            Instruction::SubtractFrom { d: 0, a: 5, b: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 13,
                offset: 0,
            },
            Instruction::Negate { d: 0, a: 4 },
            Instruction::ExtendSignHalfword { a: 0, s: 0 },
            Instruction::StoreHalfword {
                s: 0,
                a: 3,
                offset: 0,
            },
        ]
    }
    fn relocations() -> Vec<Relocation> {
        [1, 3]
            .into_iter()
            .map(|instruction_index| Relocation {
                instruction_index,
                kind: RelocationKind::EmbSda21,
                target: RelocationTarget::External("input".into()),
            })
            .collect()
    }

    #[test]
    fn physical_temporaries_must_be_dead_on_all_paths_before_migration() {
        let instructions = sequence();
        assert!(dead_after_subtraction(
            &mwcc_vreg::analyze(&instructions),
            5,
            2
        ));
        for consumer in [
            Instruction::Or { a: 3, s: 5, b: 5 },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ] {
            let mut instructions = sequence();
            instructions.push(consumer);
            assert!(!dead_after_subtraction(
                &mwcc_vreg::analyze(&instructions),
                5,
                2
            ));
        }
    }

    #[test]
    fn version_policy_places_negation_around_the_subtraction() {
        let instructions = sequence();
        assert_eq!(
            plan(
                &instructions,
                &relocations(),
                0,
                NegatedUpdateScheduleStyle::AfterSubtract
            ),
            Some(Placement::AfterSubtract)
        );
        assert_eq!(
            plan(
                &instructions,
                &relocations(),
                0,
                NegatedUpdateScheduleStyle::BeforeSubtract
            ),
            Some(Placement::BeforeSubtract)
        );
        let mut instructions = instructions;
        instructions.insert(
            0,
            Instruction::StoreWord {
                s: 6,
                a: 3,
                offset: 0,
            },
        );
        let mut relocations = relocations();
        for relocation in &mut relocations {
            relocation.instruction_index += 1;
        }
        assert_eq!(
            plan(
                &instructions,
                &relocations,
                1,
                NegatedUpdateScheduleStyle::AfterSubtract
            ),
            Some(Placement::BeforeMultiply)
        );
    }

    #[test]
    fn interior_entries_cannot_skip_part_of_the_renamed_value() {
        for entry in 1..7 {
            let mut instructions = sequence();
            instructions.push(Instruction::Branch { target: entry });
            assert_eq!(
                plan(
                    &instructions,
                    &relocations(),
                    0,
                    NegatedUpdateScheduleStyle::AfterSubtract
                ),
                None
            );
        }
        let mut instructions = sequence();
        instructions.push(Instruction::Branch { target: 0 });
        assert!(plan(
            &instructions,
            &relocations(),
            0,
            NegatedUpdateScheduleStyle::AfterSubtract
        )
        .is_some());
    }

    #[test]
    fn only_a_single_unmodified_delta_and_matching_global_update_qualify() {
        let mut instructions = sequence();
        instructions[0] = Instruction::MultiplyImmediate {
            d: 4,
            a: 4,
            immediate: 160,
        };
        assert_eq!(
            plan(
                &instructions,
                &relocations(),
                0,
                NegatedUpdateScheduleStyle::AfterSubtract
            ),
            None
        );
        let mut different_globals = relocations();
        different_globals[1].target = RelocationTarget::External("other".into());
        assert_eq!(
            plan(
                &sequence(),
                &different_globals,
                0,
                NegatedUpdateScheduleStyle::AfterSubtract
            ),
            None
        );
        let mut instructions = sequence();
        instructions[3] = Instruction::StoreWord {
            s: 0,
            a: 13,
            offset: 4,
        };
        assert_eq!(
            plan(
                &instructions,
                &relocations(),
                0,
                NegatedUpdateScheduleStyle::AfterSubtract
            ),
            None
        );
    }
}
