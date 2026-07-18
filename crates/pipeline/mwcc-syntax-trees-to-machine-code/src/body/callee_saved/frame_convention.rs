//! Shared plain non-leaf linkage sequences across compiler generations.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
    pub(crate) fn normalize_linkage_first_callee_saved_frame(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.non_leaf
            || self.callee_saved.is_empty()
            || self.callee_saved_float != 0
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
        let reserve_extra_lane = materialized_home_before_call
            || self.legacy_callee_saved_frame_layout
                == LegacyCalleeSavedFrameLayout::ReserveForwardedParameterLane;
        let new_size = old_size + if reserve_extra_lane { 8 } else { 0 };

        if let Instruction::StoreWordWithUpdate { offset, .. } = &mut self.output.instructions[0] {
            *offset = -new_size;
        }
        for (index, &register) in physical_saved.iter().enumerate() {
            let old_offset = old_size - 4 * (index as i16 + 1);
            let new_offset = new_size - 4 * (index as i16 + 1);
            for instruction in &mut self.output.instructions {
                match instruction {
                    Instruction::StoreWord { s, a: 1, offset }
                        if *s == register && *offset == old_offset =>
                    {
                        *offset = new_offset
                    }
                    Instruction::LoadWord { d, a: 1, offset }
                        if *d == register && *offset == old_offset =>
                    {
                        *offset = new_offset
                    }
                    _ => {}
                }
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::StoreWord { s: 0, a: 1, offset } if *offset == old_size + 4 => {
                    *offset = 4;
                }
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
        let promoted_parameter_count = self.output.instructions[..first_call]
            .iter()
            .filter(|instruction| {
                matches!(instruction, Instruction::Or { a, s, b }
                    if s == b && physical_saved.contains(a))
            })
            .count();
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
            let source_redefined = self.output.instructions[index + 1..straight_line_end]
                .iter()
                .any(|later| {
                    mwcc_vreg::register_operands(later)
                        .into_iter()
                        .any(|operand| {
                            operand.role == mwcc_vreg::RegisterRole::Define
                                && operand.class == mwcc_vreg::Class::General
                                && operand.register == source
                        })
                });
            if promoted_parameter_count >= 2 || source_redefined {
                self.output.instructions[index] = Instruction::AddImmediate {
                    d: destination,
                    a: source,
                    immediate: 0,
                };
            }
        }

        // [stwu, mflr, scheduled gap..., stw LR] ->
        // [mflr, scheduled gap..., stw LR, stwu].
        self.output.instructions[..=link_store].rotate_left(1);
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
        self.delay_plain_frame_update_past_narrow_condition(link_store);
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
    fn delay_plain_frame_update_past_narrow_condition(&mut self, frame_update: usize) {
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
        if contains_narrowing && is_narrow_compare_prefix {
            self.output.instructions[frame_update..branch].rotate_left(1);
        }
    }

    /// Build 163 reserves an additional eight bytes below the spill image for
    /// frameless numeric-conversion functions. The conversion owners share the
    /// same selected slot-relative body, so apply the generation's leaf-frame
    /// layout after selection while preserving every non-stack instruction.
    pub(crate) fn normalize_linkage_first_conversion_frame(&mut self) {
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
