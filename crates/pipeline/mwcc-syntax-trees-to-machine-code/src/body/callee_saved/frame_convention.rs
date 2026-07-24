//! Shared plain non-leaf linkage sequences across compiler generations.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Canonicalize adjacent linkage-first GPR saves and restores by physical
    /// register number. Virtual home order follows source lifetimes, but MWCC's
    /// frame slots remain r31 downward regardless of that semantic order.
    pub(crate) fn normalize_linkage_first_saved_register_order(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        for index in 0..self.output.instructions.len().saturating_sub(1) {
            let saved = match (
                &self.output.instructions[index],
                &self.output.instructions[index + 1],
            ) {
                (
                    Instruction::StoreWord { s: first, a: 1, offset: first_offset },
                    Instruction::StoreWord { s: second, a: 1, offset: second_offset },
                ) if (14..=31).contains(first)
                    && (14..=31).contains(second)
                    && first < second
                    && *first_offset == second_offset.saturating_add(4) => Some((*first, *second)),
                _ => None,
            };
            if let Some((first, second)) = saved {
                let Instruction::StoreWord { s, .. } = &mut self.output.instructions[index] else {
                    unreachable!()
                };
                *s = second;
                let Instruction::StoreWord { s, .. } = &mut self.output.instructions[index + 1] else {
                    unreachable!()
                };
                *s = first;
                continue;
            }
            let restored = match (
                &self.output.instructions[index],
                &self.output.instructions[index + 1],
            ) {
                (
                    Instruction::LoadWord { d: first, a: 1, offset: first_offset },
                    Instruction::LoadWord { d: second, a: 1, offset: second_offset },
                ) if (14..=31).contains(first)
                    && (14..=31).contains(second)
                    && first < second
                    && *first_offset == second_offset.saturating_add(4) => Some((*first, *second)),
                _ => None,
            };
            if let Some((first, second)) = restored {
                let Instruction::LoadWord { d, .. } = &mut self.output.instructions[index] else {
                    unreachable!()
                };
                *d = second;
                let Instruction::LoadWord { d, .. } = &mut self.output.instructions[index + 1] else {
                    unreachable!()
                };
                *d = first;
            }
        }
    }

    /// Fill the linkage slot left by an inlined statement body with its
    /// independent zero store value. The retained frame lane proves this is an
    /// inline-composed body rather than an ordinary local initialization.
    pub(crate) fn schedule_linkage_first_inline_zero(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.legacy_inline_expansion_frame_bytes == 0
        {
            return;
        }
        let Some(start) = self.output.instructions.windows(6).position(|window| {
            matches!(window, [
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, offset: 4 },
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::StoreWord { s: saved, a: 1, .. },
                Instruction::LoadWord { d: alias, a: 3, .. },
                Instruction::AddImmediate { d: 0, a: 0, immediate: 0 },
            ] if saved == alias)
        }) else {
            return;
        };
        let from = start + 5;
        let to = start + 2;
        let instruction = self.output.instructions.remove(from);
        self.output.instructions.insert(to, instruction);
        self.labels.moved_before(from, to);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = if relocation.instruction_index == from {
                to
            } else if (to..from).contains(&relocation.instruction_index) {
                relocation.instruction_index + 1
            } else {
                relocation.instruction_index
            };
        }
    }

    /// Emit the EABI helper-call frame used when a dense suffix of GPRs is
    /// cheaper to save through `_savegpr_N` than with individual stores. The
    /// matching epilogue is owned by [`Self::emit_restgpr_frame_epilogue`].
    pub(crate) fn emit_savegpr_frame_prologue(&mut self, first: u8, frame_size: i16) {
        debug_assert!((14..=31).contains(&first));
        self.frame_size = frame_size;
        self.non_leaf = true;
        self.callee_saved = (first..=31).collect();

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -frame_size,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: frame_size + 4,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: frame_size,
        });
        let helper = format!("_savegpr_{first}");
        self.record_relocation(RelocationKind::Rel24, &helper);
        self.output
            .instructions
            .push(Instruction::BranchAndLink { target: helper });
    }

    /// Close a frame opened by [`Self::emit_savegpr_frame_prologue`]. Register
    /// restoration happens before the LR reload, matching MWCC's helper ABI.
    pub(crate) fn emit_restgpr_frame_epilogue(&mut self, first: u8) {
        debug_assert_eq!(self.callee_saved.first().copied(), Some(first));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: self.frame_size,
        });
        let helper = format!("_restgpr_{first}");
        self.record_relocation(RelocationKind::Rel24, &helper);
        self.output
            .instructions
            .push(Instruction::BranchAndLink { target: helper });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: self.frame_size + 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: self.frame_size,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }

    /// Build 163 materializes values from a collision-resolving or computed
    /// callee-saved home with `addi d,s,0`. The semantic owner supplies the
    /// instruction range and source homes; ordinary forwarding copies remain
    /// untouched.
    pub(crate) fn normalize_legacy_materialization_copies(&mut self, start: usize, sources: &[u8]) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        for instruction in &mut self.output.instructions[start..] {
            if let Instruction::Or { a, s, b } = *instruction {
                if s == b && sources.contains(&s) {
                    *instruction = Instruction::AddImmediate {
                        d: a,
                        a: s,
                        immediate: 0,
                    };
                }
            }
        }
    }

    /// Build 163 retains three compiler bookkeeping ordinals after each member
    /// of the verified queue-helper family. This is semantic-family state, not
    /// the ordinary framed-function ABI stride.
    pub(crate) fn pin_queue_helper_post_function_bump(&mut self) {
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.output.post_function_anonymous_bump = Some(4);
        }
    }

    /// Convert an allocator-emitted 2.4.x GPR-save frame into build 163's
    /// linkage-first layout after scheduling and physical allocation. Keeping
    /// this as a shape-preserving normalization lets the many semantic owners
    /// share one ABI policy without duplicating their body schedules.
    pub(crate) fn normalize_linkage_first_callee_saved_frame(
        &mut self,
        pending_allocated_float_saves: bool,
    ) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.non_leaf
            || self.callee_saved.is_empty()
            || (self.callee_saved_float != 0 && !pending_allocated_float_saves)
            || self.frame_size == 0
        {
            return;
        }
        let old_size = self.frame_size;
        if !matches!(
            self.output.instructions.first(),
            Some(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset }) if *offset == -old_size
        ) {
            // A convention-aware owner (queue posting, initialization) already
            // emitted its final linkage-first frame.
            return;
        }
        let Some(link_store) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreWord { s: 0, a: 1, offset } if *offset == old_size + 4)
        }) else {
            return;
        };
        let mut physical_saved = Vec::with_capacity(self.callee_saved.len());
        for index in 0..self.callee_saved.len() {
            let old_offset = old_size - 4 * (index as i16 + 1);
            let register =
                self.output
                    .instructions
                    .iter()
                    .find_map(|instruction| match instruction {
                        Instruction::StoreWord { s, a: 1, offset } if *offset == old_offset => {
                            Some(*s)
                        }
                        _ => None,
                    });
            let Some(register) = register else { return };
            physical_saved.push(register);
        }
        let first_call = self
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .unwrap_or(self.output.instructions.len());
        // Build 163's extra lane belongs to an ALU-materialized value that is
        // already live before the first call (parameters and computed locals).
        // A home first defined by a memory load, or only by a call result after
        // the call, retains the logical frame size. This is the allocator's
        // value-origin distinction, not a semantic-family whitelist.
        let materialized_home_before_call =
            self.output.instructions[..first_call]
                .iter()
                .any(|instruction| {
                    let defines_home =
                        mwcc_vreg::register_operands(instruction)
                            .into_iter()
                            .any(|operand| {
                                operand.role == mwcc_vreg::RegisterRole::Define
                                    && operand.class == mwcc_vreg::Class::General
                                    && physical_saved.contains(&operand.register)
                            });
                    defines_home
                        && !matches!(
                            instruction,
                            Instruction::LoadWord { .. }
                                | Instruction::LoadByteZero { .. }
                                | Instruction::LoadHalfwordZero { .. }
                                | Instruction::LoadHalfwordAlgebraic { .. }
                                | Instruction::LoadWordIndexed { .. }
                                | Instruction::LoadByteZeroIndexed { .. }
                                | Instruction::LoadHalfwordZeroIndexed { .. }
                                | Instruction::LoadHalfwordAlgebraicIndexed { .. }
                        )
                });
        let loaded_home_before_call = self.output.instructions[..first_call]
            .iter()
            .any(|instruction| {
                matches!(instruction, Instruction::LoadWord { d, .. } if physical_saved.contains(d))
            });
        let promoted_parameter_count = self.output.instructions[..first_call]
            .iter()
            .filter(|instruction| {
                matches!(instruction, Instruction::Or { a, s, b }
                    if s == b && physical_saved.contains(a))
            })
            .count();
        let preserve_logical_size = self.legacy_callee_saved_frame_layout
            == LegacyCalleeSavedFrameLayout::PreserveLogicalSize;
        let reserve_forwarded_parameter_lane = self.legacy_callee_saved_frame_layout
            == LegacyCalleeSavedFrameLayout::ReserveForwardedParameterLane;
        let retain_eager_local_lane = self.legacy_callee_saved_frame_layout
            == LegacyCalleeSavedFrameLayout::RetainEagerLocalLane;
        // Build 163 keeps dead call-initializer results in its frame-pressure
        // accounting even after eliminating the values. Only that erased-local
        // case exposes the pairwise lane count; ordinary promoted values retain
        // the established single inferred lane regardless of their count.
        let extra_lane_count = if preserve_logical_size {
            0
        } else if self.legacy_discarded_call_locals == 0 {
            if materialized_home_before_call
                && self.legacy_callee_saved_frame_layout
                    == LegacyCalleeSavedFrameLayout::RetainEntryParameterTable
            {
                // Build 163 retains the incoming parameter table in pairs of
                // 32-bit words whenever an entry value is materialized into a
                // saved home. This is why an otherwise-unused third parameter
                // grows a 24-byte one-home frame to 32 bytes, and why a double
                // has the same effect: both make the footprint three words.
                self.entry_parameter_words.div_ceil(2).max(1)
            } else if materialized_home_before_call {
                1
            } else if retain_eager_local_lane
                && physical_saved.len() == 2
                && loaded_home_before_call
            {
                1
            } else {
                usize::from(reserve_forwarded_parameter_lane)
            }
        } else {
            let retained_parameter_lanes = if materialized_home_before_call
                && self.legacy_callee_saved_frame_layout
                    == LegacyCalleeSavedFrameLayout::RetainEntryParameterTable
            {
                self.entry_parameter_words.div_ceil(2).max(1)
            } else if materialized_home_before_call {
                1
            } else if retain_eager_local_lane
                && physical_saved.len() == 2
                && loaded_home_before_call
            {
                1
            } else {
                usize::from(reserve_forwarded_parameter_lane)
            };
            let promoted_values = promoted_parameter_count.max(retained_parameter_lanes);
            (promoted_values + self.legacy_discarded_call_locals).div_ceil(2)
        };
        let entry_lane_bytes = i16::try_from(extra_lane_count * 8).unwrap_or(i16::MAX);
        let inline_lane_bytes =
            i16::try_from(self.legacy_inline_expansion_frame_bytes).unwrap_or(i16::MAX);
        // A single inlined aggregate setter reuses the retained two-parameter
        // entry-table lane as the anonymous slot below its frame-resident
        // aggregate. The lane therefore moves the aggregate up by one word
        // pair but grows the frame only once; treating the two provenances as
        // additive produces an oversized frame and leaves the aggregate at +8.
        let shares_inline_aggregate_lane = entry_lane_bytes == 8
            && inline_lane_bytes == 8
            && physical_saved.len() == 1
            && self.frame_slots.len() == 1
            && self.frame_slots.values().all(|slot| {
                slot.offset == 8
                    && !slot.is_array
                    && matches!(slot.value_type, Type::Struct { size: 12, .. })
            });
        // A scalarized statement-body inline uses the eager local's retained
        // lane as its own optimizer residue. The provenance is recorded twice
        // (value origin and inline expansion), but represents one physical
        // eight-byte lane, just like the frame-resident aggregate case above.
        let shares_inline_eager_lane = retain_eager_local_lane
            && entry_lane_bytes == 8
            && inline_lane_bytes == 8
            && physical_saved.len() == 2;
        let retained_frame_bytes = if shares_inline_aggregate_lane || shares_inline_eager_lane {
            entry_lane_bytes.max(inline_lane_bytes)
        } else {
            entry_lane_bytes.saturating_add(inline_lane_bytes)
        };
        let base_size = old_size.saturating_add(retained_frame_bytes);
        let conversion_size = if self.callee_saved_conversion_bytes == 0 {
            old_size
        } else {
            let conversion_end = old_size
                .saturating_add(self.callee_saved_conversion_bytes)
                .saturating_add(i16::try_from(physical_saved.len() * 4).unwrap_or(i16::MAX));
            conversion_end.saturating_add(7) & !7
        };
        let new_size = base_size.max(conversion_size);
        if shares_inline_aggregate_lane {
            let slots: Vec<_> = self
                .frame_slots
                .values()
                .map(|slot| (slot.offset, i16::from(slot.size)))
                .collect();
            relayout_frame_slot_displacements(&mut self.output.instructions, &slots, 8);
            for slot in self.frame_slots.values_mut() {
                slot.offset = slot.offset.saturating_add(8);
            }
        }

        if let Instruction::StoreWordWithUpdate { offset, .. } = &mut self.output.instructions[0] {
            *offset = -new_size;
        }
        if let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[link_store] {
            *offset = 4;
        }
        for (index, &register) in physical_saved.iter().enumerate() {
            let old_offset = old_size - 4 * (index as i16 + 1);
            let new_offset = new_size - 4 * (index as i16 + 1);
            relayout_callee_saved_slot(
                &mut self.output.instructions,
                register,
                old_offset,
                new_offset,
            );
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::LoadWord { d: 0, a: 1, offset } if *offset == old_size + 4 => {
                    *offset = new_size + 4;
                }
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == old_size => {
                    *immediate = new_size;
                }
                _ => {}
            }
        }

        // Two-or-more INCOMING PARAMETERS promoted together use
        // `addi rS,rA,0` in build 163. Count pre-call home copies rather than
        // saved registers: a later call result may occupy another saved home,
        // but does not change the lone parameter copy's `mr` encoding.
        for index in 0..first_call {
            let (destination, source) = match self.output.instructions[index] {
                Instruction::Or { a, s, b } if s == b && physical_saved.contains(&a) => (a, s),
                _ => continue,
            };
            // A single promoted parameter normally keeps `mr`. It switches to
            // `addi` when first-call setup overwrites the incoming register
            // (for example `bar(0)` after saving r3), because this is a
            // collision-resolving copy. Two promoted parameters always form
            // such a copy group.
            // Only a straight-line redefinition creates an entry-copy
            // collision. A control-flow edge ends that schedule; definitions
            // in a loop target do not change the prologue copy encoding.
            let straight_line_end = self.output.instructions[index + 1..first_call]
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Branch { .. } | Instruction::BranchConditionalForward { .. }
                    )
                })
                .map_or(first_call, |offset| index + 1 + offset);
            let source_redefined_by_materialization = self.output.instructions
                [index + 1..straight_line_end]
                .iter()
                .any(|later| {
                    !matches!(
                        later,
                        Instruction::LoadWord { .. }
                            | Instruction::LoadByteZero { .. }
                            | Instruction::LoadHalfwordZero { .. }
                            | Instruction::LoadHalfwordAlgebraic { .. }
                            | Instruction::LoadWordIndexed { .. }
                            | Instruction::LoadByteZeroIndexed { .. }
                            | Instruction::LoadHalfwordZeroIndexed { .. }
                            | Instruction::LoadHalfwordAlgebraicIndexed { .. }
                    ) && mwcc_vreg::register_operands(later)
                        .into_iter()
                        .any(|operand| {
                            operand.role == mwcc_vreg::RegisterRole::Define
                                && operand.class == mwcc_vreg::Class::General
                                && operand.register == source
                        })
                });
            if promoted_parameter_count >= 2
                || (self.legacy_inline_expansion_frame_bytes == 0
                    && source_redefined_by_materialization)
            {
                self.output.instructions[index] = Instruction::AddImmediate {
                    d: destination,
                    a: source,
                    immediate: 0,
                };
            }
        }

        // A retained eager-local lane gives the entry-loaded object the lower
        // home while a later call result occupies the higher one. Build 163
        // materializes the eager object into r3 with `addi`, distinguishing
        // this collision-resolved value from an ordinary forwarding move.
        if retain_eager_local_lane && physical_saved.len() == 2 {
            for instruction in &mut self.output.instructions[..first_call] {
                if let Instruction::Or { a: 3, s, b } = *instruction {
                    if s == b && physical_saved.contains(&s) {
                        *instruction = Instruction::AddImmediate {
                            d: 3,
                            a: s,
                            immediate: 0,
                        };
                    }
                }
            }
        }

        // [stwu, mflr, scheduled gap..., stw LR] ->
        // [mflr, scheduled gap..., stw LR, stwu].
        self.output.instructions[..=link_store].rotate_left(1);
        remap_prefix_rotate_left(&mut self.output.relocations, link_store);
        self.schedule_linkage_first_entry_arguments();
        // The same linkage-first convention tears down in the inverse order:
        // restore SP before writing LR. Most allocator-owned epilogues already
        // arrive in that order; hand-emitted loop owners still carry the 2.4.x
        // `mtlr; addi r1` pair, so normalize every such final pair here.
        for index in 0..self.output.instructions.len().saturating_sub(1) {
            if matches!(
                self.output.instructions[index],
                Instruction::MoveToLinkRegister { s: 0 }
            ) && matches!(
                self.output.instructions[index + 1],
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate
                } if immediate == new_size
            ) {
                self.output.instructions.swap(index, index + 1);
            }
        }
        self.frame_size = new_size;
    }

    /// Normalize an owner-emitted plain 2.4.x call frame when it has no locals
    /// or saved registers. The strict stack-reference check distinguishes this
    /// from same-sized address-taken-local frames.
    pub(crate) fn normalize_linkage_first_plain_nonleaf_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.non_leaf
            || !self.callee_saved.is_empty()
            || self.callee_saved_float != 0
            || self.frame_size != 16
            || !matches!(
                self.output.instructions.first(),
                Some(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16
                })
            )
        {
            return;
        }
        let Some(link_store) = self.output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20
                }
            )
        }) else {
            return;
        };
        let restored_stack_link_load = self.behavior.plain_linkage_epilogue_style
            == PlainLinkageEpilogueStyle::StackRestoreBeforeReload;
        let stack_restore = self.output.instructions.iter().position(|instruction| {
            matches!(
                instruction,
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 16
                }
            )
        });
        let has_other_stack_reference = self.output.instructions.iter().enumerate().any(
            |(index, instruction)| match instruction {
                Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16,
                } => index != 0,
                Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                }
                | Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 20,
                }
                | Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 16,
                } => false,
                Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 4,
                } if restored_stack_link_load
                    && stack_restore.is_some_and(|restore| restore < index) =>
                {
                    false
                }
                Instruction::StoreWord { a: 1, .. }
                | Instruction::StoreByte { a: 1, .. }
                | Instruction::StoreHalfword { a: 1, .. }
                | Instruction::StoreFloatSingle { a: 1, .. }
                | Instruction::LoadFloatDouble { a: 1, .. }
                | Instruction::LoadWord { a: 1, .. }
                | Instruction::LoadByteZero { a: 1, .. }
                | Instruction::LoadHalfwordZero { a: 1, .. }
                | Instruction::LoadHalfwordAlgebraic { a: 1, .. }
                | Instruction::LoadFloatSingle { a: 1, .. }
                | Instruction::StoreFloatDouble { a: 1, .. }
                | Instruction::PairedSingleQuantizedLoad { a: 1, .. }
                | Instruction::PairedSingleQuantizedStore { a: 1, .. }
                | Instruction::StoreMultipleWord { a: 1, .. }
                | Instruction::LoadMultipleWord { a: 1, .. }
                | Instruction::AddImmediate { a: 1, .. } => true,
                _ => false,
            },
        );
        if has_other_stack_reference {
            return;
        }

        if let Instruction::StoreWordWithUpdate { offset, .. } = &mut self.output.instructions[0] {
            *offset = -8;
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWord { s: 0, a: 1, offset } if *offset == 20 => *offset = 4,
                Instruction::LoadWord { d: 0, a: 1, offset } if *offset == 20 => *offset = 12,
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == 16 => {
                    *immediate = 8;
                }
                _ => {}
            }
        }
        self.output.instructions[..=link_store].rotate_left(1);
        self.delay_plain_frame_update_past_condition_prefix(link_store);
        for index in 0..self.output.instructions.len().saturating_sub(1) {
            if matches!(
                self.output.instructions[index],
                Instruction::MoveToLinkRegister { s: 0 }
            ) && matches!(
                self.output.instructions[index + 1],
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: 8
                }
            ) {
                self.output.instructions.swap(index, index + 1);
            }
        }
        self.frame_size = 8;
    }

    /// Move a build-163 plain frame's final stack update past a register-only
    /// narrow condition prefix. Memory conditions deliberately stay below the
    /// update because their load uses r0 after LR is safely stored.
    fn delay_plain_frame_update_past_condition_prefix(&mut self, frame_update: usize) {
        if !matches!(
            self.output.instructions.get(frame_update),
            Some(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -8
            })
        ) {
            return;
        }

        // Build 163 delays the final stack update until after a register-only
        // narrow condition has been widened/compared. Memory conditions stay
        // below the update because their load uses r0 after LR is safely stored.
        // Keep this normalization deliberately narrow: it only crosses the
        // extension/mask + compare family, never a load or arbitrary ALU node.
        let Some(branch_offset) = self.output.instructions[frame_update + 1..]
            .iter()
            .position(|instruction| {
                matches!(instruction, Instruction::BranchConditionalForward { .. })
            })
        else {
            return;
        };
        let branch = frame_update + 1 + branch_offset;
        let prefix = &self.output.instructions[frame_update + 1..branch];
        let contains_narrowing = prefix.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::ExtendSignByte { .. }
                    | Instruction::ExtendSignByteRecord { .. }
                    | Instruction::ExtendSignHalfword { .. }
                    | Instruction::ExtendSignHalfwordRecord { .. }
                    | Instruction::ClearLeftImmediate { .. }
                    | Instruction::ClearLeftImmediateRecord { .. }
            )
        });
        let is_narrow_compare_prefix = !prefix.is_empty()
            && prefix.iter().all(|instruction| {
                matches!(
                    instruction,
                    Instruction::ExtendSignByte { .. }
                        | Instruction::ExtendSignByteRecord { .. }
                        | Instruction::ExtendSignHalfword { .. }
                        | Instruction::ExtendSignHalfwordRecord { .. }
                        | Instruction::ClearLeftImmediate { .. }
                        | Instruction::ClearLeftImmediateRecord { .. }
                        | Instruction::CompareWordImmediate { .. }
                        | Instruction::CompareWord { .. }
                        | Instruction::CompareLogicalWordImmediate { .. }
                        | Instruction::CompareLogicalWord { .. }
                )
            });
        // A discarded assertion materializes the result of `a && b` around
        // this same frame update. Its first compare already precedes the LR
        // store; the two ready `li` operations fill the second linkage gap.
        let is_assertion_value_prefix = matches!(
            (
                self.output.instructions.get(frame_update.checked_sub(1).unwrap_or(0)),
                prefix,
            ),
            (
                Some(Instruction::StoreWord { s: 0, a: 1, offset: 4 }),
                [Instruction::AddImmediate { d: 0, a: 0, immediate: 1 }, Instruction::AddImmediate { d, a: 0, immediate: 0 }]
            ) if *d != 0
        );
        if (contains_narrowing && is_narrow_compare_prefix) || is_assertion_value_prefix {
            self.output.instructions[frame_update..branch].rotate_left(1);
        }
    }

    /// Build 163 reserves an additional eight bytes below the spill image for
    /// frameless numeric-conversion functions. The conversion owners share the
    /// same selected slot-relative body, so apply the generation's leaf-frame
    /// layout after selection while preserving every non-stack instruction.
    pub(crate) fn normalize_linkage_first_conversion_frame(&mut self) {
        if self.normalize_legacy_nonleaf_call_result_conversion_frame() {
            return;
        }
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.non_leaf
            || !self.output.has_conversion
            || self.frame_size == 0
            || self.output.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::StoreFloatDouble { s: 1, a: 1, .. }
                )
            })
        {
            return;
        }
        let mut pushes =
            self.output
                .instructions
                .iter()
                .enumerate()
                .filter_map(|(index, instruction)| match instruction {
                    Instruction::StoreWordWithUpdate { s: 1, a: 1, offset } if *offset < 0 => {
                        Some((index, -*offset))
                    }
                    _ => None,
                });
        let Some((frame_push, old_size)) = pushes.next() else {
            return;
        };
        if pushes.next().is_some() {
            return;
        }
        // A double mixed-promotion keeps another FP operand live after the
        // conversion subtract. Its mainline logical frame is one slot larger
        // than the hard-coded conversion scratch push; build 163 then adds its
        // ordinary eight-byte lower pad as well.
        let mixed_double_promotion = self.output.instructions.iter().any(|instruction| {
            matches!(instruction, Instruction::FloatSubtractDouble { d: 0, .. })
        }) && self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::FloatAddDouble { .. } | Instruction::FloatMultiplyDouble { .. }
            )
        });
        let logical_size = old_size.max(self.frame_size)
            + if mixed_double_promotion && self.frame_size <= old_size {
                8
            } else {
                0
            };
        let new_size = logical_size + 8;
        let stack_shift = new_size - old_size;

        for (index, instruction) in self.output.instructions.iter_mut().enumerate() {
            if index == frame_push {
                if let Instruction::StoreWordWithUpdate { offset, .. } = instruction {
                    *offset = -new_size;
                }
                continue;
            }
            match instruction {
                Instruction::StoreWord { a: 1, offset, .. }
                | Instruction::StoreByte { a: 1, offset, .. }
                | Instruction::StoreHalfword { a: 1, offset, .. }
                | Instruction::StoreFloatSingle { a: 1, offset, .. }
                | Instruction::LoadFloatDouble { a: 1, offset, .. }
                | Instruction::LoadWord { a: 1, offset, .. }
                | Instruction::LoadByteZero { a: 1, offset, .. }
                | Instruction::LoadHalfwordZero { a: 1, offset, .. }
                | Instruction::LoadHalfwordAlgebraic { a: 1, offset, .. }
                | Instruction::LoadFloatSingle { a: 1, offset, .. }
                | Instruction::StoreFloatDouble { a: 1, offset, .. }
                | Instruction::PairedSingleQuantizedLoad { a: 1, offset, .. }
                | Instruction::PairedSingleQuantizedStore { a: 1, offset, .. }
                | Instruction::StoreMultipleWord { a: 1, offset, .. }
                | Instruction::LoadMultipleWord { a: 1, offset, .. }
                    if *offset >= 8 =>
                {
                    *offset += stack_shift
                }
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == old_size => {
                    if let Instruction::AddImmediate { immediate, .. } = instruction {
                        *immediate = new_size;
                    }
                }
                Instruction::AddImmediate {
                    a: 1, immediate, ..
                } if *immediate >= 8 => {
                    *immediate += stack_shift;
                }
                _ => {}
            }
        }
        self.frame_size = new_size;
    }

    /// Build 163's call-result conversion reuses the linkage-first non-leaf
    /// frame. A value stored from f0 needs the 16-byte logical conversion frame;
    /// a value returned in f1 reserves one additional eight-byte lane.
    fn normalize_legacy_nonleaf_call_result_conversion_frame(&mut self) -> bool {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.int_call_result_conversion_style
                != mwcc_versions::IntCallResultConversionStyle::LegacyBiasFirst
            || !self.non_leaf
            || !self.output.has_conversion
        {
            return false;
        }
        let Some(frame_push) = self.output.instructions.iter().position(|instruction| {
            matches!(instruction, Instruction::StoreWordWithUpdate { s: 1, a: 1, offset } if matches!(*offset, -8 | -16))
        }) else {
            return false;
        };
        let old_size = match self.output.instructions[frame_push] {
            Instruction::StoreWordWithUpdate { offset, .. } => -offset,
            _ => unreachable!(),
        };
        let returned = self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::FloatSubtractDouble { d, .. }
                    | Instruction::FloatSubtractSingle { d, .. }
                    if *d == Eabi::float_result().number
            )
        });
        let padding = if returned { 8 } else { 0 };
        let new_size = (16 + padding).max(old_size + 8);
        let stack_shift = new_size - old_size;

        if let Instruction::StoreWordWithUpdate { offset, .. } =
            &mut self.output.instructions[frame_push]
        {
            *offset = -new_size;
        }
        for instruction in &mut self.output.instructions[frame_push + 1..] {
            match instruction {
                Instruction::StoreWord { a: 1, offset, .. }
                | Instruction::LoadWord { a: 1, offset, .. }
                | Instruction::LoadFloatDouble { a: 1, offset, .. }
                    if *offset >= 8 =>
                {
                    *offset += stack_shift;
                }
                Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate,
                } if *immediate == old_size => {
                    if let Instruction::AddImmediate { immediate, .. } = instruction {
                        *immediate = new_size;
                    }
                }
                _ => {}
            }
        }
        // A structured 2.4.x owner starts `stwu; mflr; stw LR`. Build 163
        // writes LR through the caller linkage area, starts the independent
        // conversion high word, and only then updates SP.
        if frame_push == 0
            && matches!(self.output.instructions.get(1), Some(Instruction::MoveFromLinkRegister { d: 0 }))
            && matches!(self.output.instructions.get(2), Some(Instruction::StoreWord { s: 0, a: 1, .. }))
        {
            if let Instruction::StoreWord { offset, .. } = &mut self.output.instructions[2] {
                *offset = 4;
            }
            self.output.instructions[..3].rotate_left(1);
            let updated_push = 2;
            if let Some(high_word) = self.output.instructions[updated_push + 1..]
                .iter()
                .position(|instruction| {
                    matches!(instruction, Instruction::AddImmediateShifted { d: 0, a: 0, immediate: 17200 })
                })
                .map(|offset| updated_push + 1 + offset)
            {
                let instruction = self.output.instructions.remove(high_word);
                self.output.instructions.insert(updated_push, instruction);
            }
        }
        self.frame_size = new_size;
        true
    }

    /// Copy a parameter or call result into its callee-saved home using the
    /// generation's allocator-selected idiom.
    pub(crate) fn emit_callee_saved_home_copy(&mut self, destination: u8, source: u8) {
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.output
                    .instructions
                    .push(Instruction::move_register(destination, source));
            }
            FrameConvention::LinkageFirst => {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: source,
                    immediate: 0,
                });
            }
        }
    }

    /// Begin a linkage-first non-leaf function, optionally preserving GPRs at
    /// the top of an 8-byte-aligned frame. The incoming stack pointer remains
    /// addressable until LR has been stored in its caller linkage area.
    pub(crate) fn emit_linkage_first_nonleaf_prologue(&mut self, callee_saved: &[u8]) {
        debug_assert_eq!(
            self.behavior.frame_convention,
            FrameConvention::LinkageFirst
        );
        self.non_leaf = true;
        self.callee_saved = callee_saved.to_vec();
        let unaligned_size = 8 + 4 * callee_saved.len() as i16;
        self.frame_size = (unaligned_size + 7) & !7;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -self.frame_size,
            });
        for (index, &register) in callee_saved.iter().enumerate() {
            let offset = self.frame_size - 4 * (index as i16 + 1);
            self.output.instructions.push(Instruction::StoreWord {
                s: register,
                a: 1,
                offset,
            });
        }
    }

    /// Begin a non-leaf function with no callee-saved registers or locals.
    /// Build 163 writes the linkage area through the incoming SP before its
    /// 8-byte update; 2.4.x predecrements a 16-byte frame first.
    pub(crate) fn emit_plain_nonleaf_prologue(&mut self) -> usize {
        self.non_leaf = true;
        match self.behavior.frame_convention {
            FrameConvention::Predecrement => {
                self.frame_size = 16;
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -16,
                    });
                self.output
                    .instructions
                    .push(Instruction::MoveFromLinkRegister { d: 0 });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                });
                self.output.instructions.len() - 1
            }
            FrameConvention::LinkageFirst => {
                self.emit_linkage_first_nonleaf_prologue(&[]);
                self.output.instructions.len() - 2
            }
        }
    }
}

/// Keep a callee-saved frame slot tied to the physical register captured by
/// its prologue store. The save use and epilogue reload definition are
/// disconnected live ranges, so allocation may initially assign the reload a
/// different physical identity even though both represent one ABI slot.
fn relayout_callee_saved_slot(
    instructions: &mut [Instruction],
    saved_register: u8,
    old_offset: i16,
    new_offset: i16,
) {
    for instruction in instructions {
        match instruction {
            Instruction::StoreWord { s, a: 1, offset }
                if *s == saved_register && *offset == old_offset =>
            {
                *offset = new_offset;
            }
            Instruction::LoadWord { d, a: 1, offset } if *offset == old_offset => {
                *d = saved_register;
                *offset = new_offset;
            }
            _ => {}
        }
    }
}

/// Move displacement-based references to a frame-local region by `shift`.
/// The caller supplies a snapshot of the old slots so adjacent slots cannot
/// cause an already-moved reference to be shifted a second time.
fn relayout_frame_slot_displacements(
    instructions: &mut [Instruction],
    slots: &[(i16, i16)],
    shift: i16,
) {
    let belongs_to_slot = |offset: i16| {
        slots.iter().any(|(start, size)| {
            start
                .checked_add(*size)
                .is_some_and(|end| (*start..end).contains(&offset))
        })
    };
    for instruction in instructions {
        let displacement = match instruction {
            Instruction::StoreWord { a: 1, offset, .. }
            | Instruction::StoreByte { a: 1, offset, .. }
            | Instruction::StoreHalfword { a: 1, offset, .. }
            | Instruction::StoreFloatSingle { a: 1, offset, .. }
            | Instruction::StoreFloatDouble { a: 1, offset, .. }
            | Instruction::LoadWord { a: 1, offset, .. }
            | Instruction::LoadByteZero { a: 1, offset, .. }
            | Instruction::LoadHalfwordZero { a: 1, offset, .. }
            | Instruction::LoadHalfwordAlgebraic { a: 1, offset, .. }
            | Instruction::LoadFloatSingle { a: 1, offset, .. }
            | Instruction::LoadFloatDouble { a: 1, offset, .. }
            | Instruction::PairedSingleQuantizedLoad { a: 1, offset, .. }
            | Instruction::PairedSingleQuantizedStore { a: 1, offset, .. } => Some(offset),
            Instruction::AddImmediate {
                a: 1, immediate, ..
            } => Some(immediate),
            _ => None,
        };
        if let Some(displacement) = displacement.filter(|offset| belongs_to_slot(**offset)) {
            *displacement = displacement.saturating_add(shift);
        }
    }
}

/// Remap instruction-index relocations after `[0..=end]` rotates left once.
fn remap_prefix_rotate_left(
    relocations: &mut [mwcc_machine_code::Relocation],
    end: usize,
) {
    for relocation in relocations {
        relocation.instruction_index = match relocation.instruction_index {
            0 => end,
            index if index <= end => index - 1,
            index => index,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{Relocation, RelocationKind, RelocationTarget};

    #[test]
    fn prefix_rotation_keeps_relocations_on_their_instructions() {
        let mut relocations = (0..5)
            .map(|instruction_index| Relocation {
                instruction_index,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::External("symbol".to_string()),
            })
            .collect::<Vec<_>>();

        remap_prefix_rotate_left(&mut relocations, 3);

        assert_eq!(
            relocations
                .iter()
                .map(|relocation| relocation.instruction_index)
                .collect::<Vec<_>>(),
            [3, 0, 1, 2, 4]
        );
    }

    #[test]
    fn callee_saved_slot_restores_the_register_that_was_saved() {
        let mut instructions = vec![
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 20,
            },
        ];

        relayout_callee_saved_slot(&mut instructions, 30, 20, 36);

        assert_eq!(
            instructions,
            [
                Instruction::StoreWord {
                    s: 30,
                    a: 1,
                    offset: 36,
                },
                Instruction::LoadWord {
                    d: 30,
                    a: 1,
                    offset: 36,
                },
            ]
        );
    }

    #[test]
    fn frame_slot_relayout_moves_only_local_displacements() {
        let mut instructions = vec![
            Instruction::StoreFloatSingle {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 16,
            },
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 28,
            },
            Instruction::AddImmediate {
                d: 4,
                a: 1,
                immediate: 8,
            },
        ];

        relayout_frame_slot_displacements(&mut instructions, &[(8, 12)], 8);

        assert!(matches!(instructions[0], Instruction::StoreFloatSingle { offset: 16, .. }));
        assert!(matches!(instructions[1], Instruction::LoadWord { offset: 24, .. }));
        assert!(matches!(instructions[2], Instruction::StoreWord { offset: 28, .. }));
        assert!(matches!(instructions[3], Instruction::AddImmediate { immediate: 16, .. }));
    }
}
