//! Entry and exit issue order for structured frames backed by array pools.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Keep the first table-address high half distinct from its indexed base.
    /// MWCC assigns the short-lived high half to r4 while the original address
    /// lifetime retains its saved-register home; destructive selection
    /// otherwise coalesces both values into that saved register.
    pub(crate) fn separate_structured_array_pool_initial_table_address(&mut self) {
        if !self.structured_array_pool_emitted {
            return;
        }
        let Some(start) = self.output.instructions.windows(8).position(|window| {
            matches!(
                window,
                [
                    Instruction::MultiplyImmediate { d: scaled, .. },
                    Instruction::AddImmediate { d: 3, a: 1, .. },
                    Instruction::AddImmediateShifted {
                        d: table_high,
                        a: 0,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 0,
                        a: table_low,
                        ..
                    },
                    Instruction::Add {
                        d: table_add,
                        a: 0,
                        b: table_scaled,
                    },
                    Instruction::AddImmediate {
                        d: 4,
                        a: source_base,
                        immediate: 64,
                    },
                    Instruction::AddImmediate {
                        d: 5,
                        a: 0,
                        immediate: 5,
                    },
                    Instruction::BranchAndLink { target },
                ] if (target == "strncpy" || target.starts_with("strncpy__"))
                    && table_high == table_low
                    && table_high == table_add
                    && table_high == source_base
                    && scaled == table_scaled
            )
        }) else {
            return;
        };

        let high_half = self.fresh_virtual_general_preferring(4);
        let Instruction::AddImmediateShifted { a, immediate, .. } =
            self.output.instructions[start + 2]
        else {
            unreachable!("the pooled-array table high half was matched above");
        };
        self.output.instructions[start + 2] = Instruction::AddImmediateShifted {
            d: high_half,
            a,
            immediate,
        };
        let Instruction::AddImmediate { d, immediate, .. } =
            self.output.instructions[start + 3]
        else {
            unreachable!("the pooled-array table low half was matched above");
        };
        self.output.instructions[start + 3] = Instruction::AddImmediate {
            d,
            a: high_half,
            immediate,
        };
    }

    /// MWCC overlaps the next formatted-call address transaction with the
    /// intervening byte store. Do this while the table base is still virtual:
    /// moving the frame arguments after its two byte loads lets allocation
    /// reuse r3 for the table base, while the stored literal prefers r5.
    pub(crate) fn schedule_structured_array_pool_following_format_call(&mut self) {
        if !self.structured_array_pool_emitted {
            return;
        }
        let Some(start) = self.output.instructions.windows(13).position(|window| {
            matches!(
                window,
                [
                    Instruction::AddImmediate {
                        d: 0,
                        a: 0,
                        immediate: 47,
                    },
                    Instruction::StoreByte {
                        s: 0,
                        a: 1,
                        offset: store_offset,
                    },
                    Instruction::AddImmediate { d: 3, a: 1, .. },
                    Instruction::AddImmediateShifted { d: 4, a: 0, .. },
                    Instruction::AddImmediate {
                        d: 4,
                        a: 4,
                        immediate: 0,
                    },
                    Instruction::AddImmediate { d: 5, a: 1, .. },
                    Instruction::AddImmediateShifted {
                        d: table_high,
                        a: 0,
                        ..
                    },
                    Instruction::AddImmediate {
                        d: 0,
                        a: table_low,
                        immediate: 0,
                    },
                    Instruction::Add {
                        d: table_add,
                        a: 0,
                        ..
                    },
                    Instruction::LoadByteZero {
                        d: 6,
                        a: first_load_base,
                        offset: first_load_offset,
                    },
                    Instruction::LoadByteZero {
                        d: 7,
                        a: second_load_base,
                        offset: second_load_offset,
                    },
                    Instruction::ConditionRegisterClear { d: 6 },
                    Instruction::BranchAndLink { target },
                ] if (target == "sprintf" || target.starts_with("sprintf__"))
                    && table_high == table_low
                    && table_high == table_add
                    && table_high == first_load_base
                    && table_high == second_load_base
                    && *second_load_offset == *first_load_offset + 1
                    && *store_offset > 0
            )
        }) else {
            return;
        };

        let literal = self.fresh_virtual_general_preferring(5);
        let mut old = self.output.instructions[start..start + 12].to_vec();
        old[0] = Instruction::load_immediate(literal, 47);
        let Instruction::StoreByte { a, offset, .. } = old[1] else {
            unreachable!("the pooled-array format transaction was matched above");
        };
        old[1] = Instruction::StoreByte {
            s: literal,
            a,
            offset,
        };
        let order = [6, 0, 7, 3, 8, 1, 9, 4, 10, 2, 5, 11];
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, old_relative) in order.into_iter().enumerate() {
            self.output.instructions[start + new_relative] = old[old_relative].clone();
            permutation[start + old_relative] = start + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }

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

    /// Once the compact entry packets have fixed the saved-register homes,
    /// MWCC starts the first pool-image address before materializing its copy
    /// count. Both are independent; this is issue order only and must not feed
    /// back into allocation.
    pub(crate) fn schedule_allocated_structured_array_pool_first_image(&mut self) {
        if !self.structured_array_pool_emitted {
            return;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(2)
            .position(|window| {
                matches!(
                    window,
                    [
                        Instruction::AddImmediate {
                            d: 14..=31,
                            a: 0,
                            immediate: 32,
                        },
                        Instruction::AddImmediate {
                            d: 3,
                            a: 5,
                            immediate: 1..,
                        },
                    ]
                )
            })
        else {
            return;
        };
        self.output.instructions.swap(start, start + 1);
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        permutation.swap(start, start + 1);
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}
