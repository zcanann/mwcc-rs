//! Entry and exit issue order for structured frames backed by array pools.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A pooled frame saves a dense physical GPR range with `stmw`, then copies
    /// retained incoming values into their homes in incoming-register order.
    /// Home planning is lifetime-ranked and therefore stores parameters in the
    /// opposite order; keep that concern separate from entry scheduling.
    pub(super) fn emit_structured_array_pool_parameter_copies(
        &mut self,
        saved_parameter_homes: &[(String, u8, u8)],
    ) {
        for (_, home, incoming) in saved_parameter_homes.iter().rev() {
            self.output
                .instructions
                .push(Instruction::move_register(*home, *incoming));
        }
    }

    /// Pooled dense frames use the ordinary MWCC teardown issue order even
    /// though non-pooled dense frames restore the stack pointer first.
    pub(super) fn schedule_structured_array_pool_epilogue(&mut self) {
        let end = self.output.instructions.len();
        if end < 5 {
            return;
        }
        if matches!(
            &self.output.instructions[end - 5..],
            [
                Instruction::LoadMultipleWord { a: 1, .. },
                Instruction::LoadWord { d: 0, a: 1, .. },
                Instruction::AddImmediate { d: 1, a: 1, .. },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::BranchToLinkRegister,
            ]
        ) {
            self.output.instructions.swap(end - 3, end - 2);
        }
    }

    /// Physical allocation uses entry-move order to break otherwise-equal home
    /// preferences, so changing the virtual stream to obtain MWCC's issue order
    /// can also swap the homes themselves. Reorder the already-allocated
    /// parameter-copy run instead: incoming r3, r4, ... order is then purely a
    /// schedule decision and cannot perturb allocation.
    pub(crate) fn schedule_allocated_structured_array_pool_parameter_copies(&mut self) {
        if !self.structured_array_pool_emitted
            || self.output.anonymous_rodata.len() < 2
            || !self
                .output
                .anonymous_rodata
                .iter()
                .any(|blob| blob.static_slot_prefix_bump.is_some())
        {
            return;
        }

        let Some(store_index) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreMultipleWord { s: 14, .. })
        }) else {
            return;
        };
        let start = store_index + 1;
        let end = self.output.instructions[start..]
            .iter()
            .position(|instruction| {
                matches!(
                    instruction,
                    Instruction::LoadWord { .. }
                        | Instruction::LoadWordWithUpdate { .. }
                        | Instruction::StoreWord { .. }
                        | Instruction::StoreWordWithUpdate { .. }
                        | Instruction::MoveToCountRegister { .. }
                        | Instruction::BranchAndLink { .. }
                )
            })
            .map_or(self.output.instructions.len(), |offset| start + offset);
        let mut parameter_copies: Vec<usize> = (start..end)
            .filter(|&index| {
                matches!(
                    &self.output.instructions[index],
                    Instruction::Or { a, s, b }
                        if a != s && s == b && (14..=31).contains(a) && (3..=10).contains(s)
                )
            })
            .collect();
        if parameter_copies.len() < 2 {
            return;
        }
        parameter_copies.sort_by_key(|&index| {
            let Instruction::Or { s, .. } = &self.output.instructions[index] else {
                unreachable!("the parameter-copy list was filtered as register moves")
            };
            *s
        });

        let mut order = parameter_copies.clone();
        order.extend((start..end).filter(|index| !parameter_copies.contains(index)));
        let old = self.output.instructions[start..end].to_vec();
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_index, &old_index) in (start..end).zip(&order) {
            self.output.instructions[new_index] = old[old_index - start].clone();
            permutation[old_index] = new_index;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }

    /// The compact pooled-copy form has three independent entry packets:
    /// tail-loop setup, the dense save, and the read-only image base. MWCC
    /// issues them in that order after allocation. Keep the physical-register
    /// signature narrow: the wider pooled forms use different packet orders.
    pub(crate) fn schedule_allocated_compact_structured_array_pool_entry(&mut self) {
        if !self.structured_array_pool_emitted
            || self.output.instructions.len() < 10
            || !matches!(
                &self.output.instructions[..4],
                [
                    Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::AddImmediateShifted { d: 5, a: 0, .. },
                    Instruction::StoreWord { s: 0, a: 1, .. },
                ]
            )
        {
            return;
        }

        let old = self.output.instructions[4..10].to_vec();
        let find = |predicate: fn(&Instruction) -> bool| old.iter().position(predicate);
        let Some(order) = [
            find(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 32,
                    }
                )
            }),
            find(|instruction| matches!(instruction, Instruction::AddImmediate { d: 6, a: 1, .. })),
            find(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreMultipleWord { s: 21, a: 1, .. }
                )
            }),
            find(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 21,
                        a: 5,
                        immediate: 0,
                    }
                )
            }),
            find(|instruction| matches!(instruction, Instruction::Or { a: 31, s: 4, b: 4 })),
            find(|instruction| {
                matches!(
                    instruction,
                    Instruction::AddImmediate {
                        d: 5,
                        a: 21,
                        immediate: 92,
                    }
                )
            }),
        ]
        .into_iter()
        .collect::<Option<Vec<_>>>()
        else {
            return;
        };
        if order.iter().copied().collect::<std::collections::HashSet<_>>().len() != 6 {
            return;
        }

        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, &old_relative) in order.iter().enumerate() {
            self.output.instructions[4 + new_relative] = old[old_relative].clone();
            permutation[4 + old_relative] = 4 + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}
