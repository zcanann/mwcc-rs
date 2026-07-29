//! Cross-term schedules for structured short-circuit conditions.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Delay a call-live pointer's saved-home definition until its leading
    /// member load has consumed the entry value.
    ///
    /// For `saved = entry->member; if (saved->field ...)`, MWCC keeps the
    /// initializer result in r4 for the first dependent load, then copies it
    /// into the callee-saved home needed by later calls. Defining the home
    /// directly is semantically equivalent, but loses that latency-hiding copy
    /// and changes every following call relocation by one instruction.
    pub(super) fn schedule_entry_member_saved_home(&mut self, function: &Function) {
        // A pure forwarding wrapper inherits the callee's value graph but not
        // its entry schedule, so MWCC keeps the direct saved-home load there.
        // A caller that continues into its own floating arithmetic after an
        // inlined helper still owns the delayed entry schedule.
        let caller_continues_after_inline = self.output.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::FloatAddSingle { .. }
                    | Instruction::FloatSubtractSingle { .. }
                    | Instruction::FloatMultiplySingle { .. }
                    | Instruction::FloatDivideSingle { .. }
                    | Instruction::FloatMultiplyAddSingle { .. }
                    | Instruction::FloatMultiplySubtractSingle { .. }
            )
        });
        let caller_has_prior_condition_transaction = function
            .statements
            .iter()
            .filter(|statement| matches!(statement, Statement::If { .. }))
            .count()
            >= 2;
        if self.inline_statement_body_substitutions != 0
            && !caller_continues_after_inline
            && !caller_has_prior_condition_transaction
        {
            return;
        }
        if let Some((start, saved)) =
            find_staggered_entry_member_saved_home(&self.output.instructions)
        {
            // Save and establish the entry parameter before consuming r3 for
            // the member initializer. The initialized pointer then stays in r3
            // through its first dependent load before moving to its saved home.
            self.move_instruction_before(start + 2, start + 1);
            self.move_instruction_before(start + 3, start + 2);
            let initializer = start + 3;
            let retained =
                self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
            match &mut self.output.instructions[initializer] {
                Instruction::LoadWord { d, .. } => *d = retained,
                _ => unreachable!(),
            }
            match &mut self.output.instructions[initializer + 1] {
                Instruction::LoadWord { a, .. } => *a = retained,
                _ => unreachable!(),
            }
            crate::insert_instruction_retargeting(
                self,
                initializer + 2,
                Instruction::AddImmediate {
                    d: saved,
                    a: retained,
                    immediate: 0,
                },
            );
            return;
        }
        let Some((initializer, saved)) = find_entry_member_saved_home(&self.output.instructions)
        else {
            return;
        };
        let retained = self.fresh_virtual_general_preferring(4);
        match &mut self.output.instructions[initializer] {
            Instruction::LoadWord { d, .. } => *d = retained,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[initializer + 1] {
            Instruction::LoadWord { a, .. } => *a = retained,
            _ => unreachable!(),
        }
        crate::insert_instruction_retargeting(
            self,
            initializer + 2,
            Instruction::AddImmediate {
                d: saved,
                a: retained,
                immediate: 0,
            },
        );
    }

    /// Fold `if (condition) goto target;` when the semantic emitter represented
    /// it as a false-edge skip over an unconditional branch. Inverting the
    /// conditional leaves one direct edge, which is MWCC's rotated-loop
    /// backedge form.
    pub(super) fn fold_structured_conditional_gotos(&mut self) {
        let mut conditional = 0;
        while conditional + 1 < self.output.instructions.len() {
            let Some((options, condition_bit, target)) =
                conditional_goto_diamond(&self.output.instructions, conditional)
            else {
                conditional += 1;
                continue;
            };
            let goto = conditional + 1;
            let has_incoming = self
                .output
                .instructions
                .iter()
                .enumerate()
                .any(|(index, instruction)| {
                    index != conditional
                        && matches!(
                            instruction,
                            Instruction::BranchConditionalForward { target, .. }
                                | Instruction::Branch { target }
                                if *target == goto
                        )
                });
            if has_incoming {
                conditional += 2;
                continue;
            }
            self.output.instructions[conditional] = Instruction::BranchConditionalForward {
                options: options ^ 8,
                condition_bit,
                target,
            };
            self.remove_structured_condition_instruction(goto);
            conditional += 1;
        }
    }

    /// Reuse entry-register values already established by a leading member
    /// comparison as the arguments of its guarded call.
    ///
    /// The receiver has been copied to a saved home for later calls, but r3 is
    /// still the untouched entry receiver and the comparison's left member is
    /// already in r4. MWCC calls directly from those values, then uses the saved
    /// receiver after the join. The complete entry-copy/load/compare/call window
    /// proves no intervening operation can have invalidated either argument.
    pub(super) fn schedule_entry_member_call_argument_reuse(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(8)
            .position(is_entry_member_call_argument_reload)
        else {
            return;
        };
        // Remove from the end so the first index remains stable.
        self.remove_structured_condition_instruction(start + 6);
        self.remove_structured_condition_instruction(start + 5);
    }

    /// Keep the guarded member receiver live through its classifier checks and
    /// the first call. The call itself clobbers r3, so only the final receiver
    /// reload before the second call remains.
    pub(crate) fn schedule_guarded_member_classifier_chain(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(15)
            .position(is_guarded_member_classifier_chain)
        else {
            return;
        };
        let (saved, entry) = match self.output.instructions[start] {
            Instruction::AddImmediate { d, a, immediate: 0 } => (d, a),
            _ => unreachable!(),
        };
        self.output.instructions[start] = Instruction::Or {
            a: saved,
            s: entry,
            b: entry,
        };
        match &mut self.output.instructions[start + 1] {
            Instruction::LoadWord { d, .. } => *d = Eabi::FIRST_GENERAL_ARGUMENT,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 2] {
            Instruction::CompareLogicalWordImmediate { a, .. } => {
                *a = Eabi::FIRST_GENERAL_ARGUMENT
            }
            _ => unreachable!(),
        }
        // Remove from the end so the earlier physical index stays stable.
        self.remove_structured_condition_instruction(start + 8);
        self.remove_structured_condition_instruction(start + 4);
    }

    /// Keep a nonnull-tested member pointer in the first call-argument register.
    ///
    /// A saved owner is still needed by later calls, but the pointer loaded for
    /// the guard is already the receiver of the first call in the taken arm.
    /// MWCC tests that value in r3 and consumes it directly instead of loading
    /// the same member again through the saved owner.
    pub(super) fn schedule_guarded_member_receiver_reuse(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(8)
            .position(is_guarded_member_receiver_reload)
        else {
            return;
        };
        let receiver = Eabi::FIRST_GENERAL_ARGUMENT;
        match &mut self.output.instructions[start + 1] {
            Instruction::LoadWord { d, .. } => *d = receiver,
            _ => unreachable!(),
        }
        match &mut self.output.instructions[start + 2] {
            Instruction::CompareLogicalWordImmediate { a, .. } => *a = receiver,
            _ => unreachable!(),
        }
        self.remove_structured_condition_instruction(start + 4);
    }

    /// Collapse two nested nonnull checks of the same member address into the
    /// receiver-producing record add plus a plain second test that MWCC keeps
    /// for the inlined wrapper boundary. The final direct call then consumes
    /// r3 without rematerializing the address.
    pub(super) fn schedule_repeated_member_address_call_guards(&mut self) {
        let mut start = 0;
        while start + 7 <= self.output.instructions.len() {
            if !is_repeated_member_address_call(&self.output.instructions[start..start + 7]) {
                start += 1;
                continue;
            }
            let (base, immediate) = match self.output.instructions[start] {
                Instruction::AddImmediateCarryingRecord { a, immediate, .. } => (a, immediate),
                _ => unreachable!(),
            };
            self.output.instructions[start] = Instruction::AddImmediateCarryingRecord {
                d: 3,
                a: base,
                immediate,
            };
            self.output.instructions[start + 2] =
                Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 };
            self.remove_structured_condition_instruction(start + 4);
            start += 6;
        }
    }

    pub(crate) fn remove_structured_condition_instruction(&mut self, at: usize) {
        self.output.instructions.remove(at);
        self.labels.removed(at, 1);
        self.output
            .relocations
            .retain(|relocation| relocation.instruction_index != at);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > at {
                relocation.instruction_index -= 1;
            }
        }
        for instruction in &mut self.output.instructions {
            match instruction {
                Instruction::BranchConditionalForward { target, .. }
                | Instruction::Branch { target }
                    if *target > at =>
                {
                    *target -= 1;
                }
                _ => {}
            }
        }
    }

    /// Reuse a nested member base loaded by the preceding `&&` term. The first
    /// false-edge branch does not clobber the loaded pointer on fallthrough, so
    /// a byte/word member test followed by another member test can share it.
    pub(super) fn reuse_short_circuit_member_base(
        &mut self,
        term_index: usize,
        term_start: usize,
    ) {
        if term_index != 0
            && reuses_preceding_bitfield_storage(&self.output.instructions, term_start)
            && !self.output.relocations.iter().any(|relocation| {
                relocation.instruction_index == term_start
                    || relocation.instruction_index + 3 == term_start
            })
        {
            let previous_load = term_start - 3;
            let retained = self.fresh_virtual_general_preferring(Eabi::FIRST_GENERAL_ARGUMENT);
            match &mut self.output.instructions[previous_load] {
                Instruction::LoadByteZero { d, .. } => *d = retained,
                _ => unreachable!(),
            }
            match &mut self.output.instructions[previous_load + 1] {
                Instruction::RotateAndMaskRecord { s, .. } => *s = retained,
                _ => unreachable!(),
            }
            match &mut self.output.instructions[term_start + 1] {
                Instruction::RotateAndMaskRecord { s, .. } => *s = retained,
                _ => unreachable!(),
            }
            self.remove_structured_condition_instruction(term_start);
            return;
        }
        if term_index == 0
            || !reuses_preceding_member_load(&self.output.instructions, term_start)
            || self
                .output
                .relocations
                .iter()
                .any(|relocation| relocation.instruction_index == term_start)
        {
            return;
        }
        self.output.instructions.remove(term_start);
        self.labels.removed(term_start, 1);
        for relocation in &mut self.output.relocations {
            if relocation.instruction_index > term_start {
                relocation.instruction_index -= 1;
            }
        }
    }
}

fn conditional_goto_diamond(
    instructions: &[Instruction],
    conditional: usize,
) -> Option<(u8, u8, usize)> {
    let [
        Instruction::BranchConditionalForward {
            options,
            condition_bit,
            target: skip,
        },
        Instruction::Branch { target },
        ..
    ] = instructions.get(conditional..)?
    else {
        return None;
    };
    (*skip == conditional + 2 && *target != conditional + 1).then_some((
        *options,
        *condition_bit,
        *target,
    ))
}

fn find_entry_member_saved_home(instructions: &[Instruction]) -> Option<(usize, u8)> {
    for initializer in 0..instructions.len().saturating_sub(3) {
        let [
            Instruction::LoadWord {
                d: saved,
                a: 3,
                ..
            },
            Instruction::LoadWord {
                d: tested,
                a: member_base,
                ..
            },
            Instruction::CompareWordImmediate { a: compared, .. },
            Instruction::BranchConditionalForward { .. },
            ..
        ] = &instructions[initializer..]
        else {
            continue;
        };
        if saved != member_base
            || tested != compared
            || !mwcc_vreg::Reg::is_virtual_field(*saved)
            || instructions[..initializer]
                .iter()
                .any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        {
            continue;
        }
        let Some(first_call) = instructions[initializer + 4..]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
            .map(|offset| initializer + 4 + offset)
        else {
            continue;
        };
        let used_after_call = instructions[first_call + 1..].iter().any(|instruction| {
            mwcc_vreg::register_operands(instruction)
                .into_iter()
                .any(|operand| {
                    operand.class == mwcc_vreg::Class::General
                        && operand.role == mwcc_vreg::RegisterRole::Use
                        && operand.register == *saved
                })
        });
        if used_after_call {
            return Some((initializer, *saved));
        }
    }
    None
}

fn find_staggered_entry_member_saved_home(
    instructions: &[Instruction],
) -> Option<(usize, u8)> {
    instructions.windows(6).enumerate().find_map(|(start, window)| {
        match window {
            [
                Instruction::StoreWord {
                    s: saved,
                    a: 1,
                    ..
                },
                Instruction::LoadWord {
                    d: initialized,
                    a: 3,
                    ..
                },
                Instruction::StoreWord {
                    s: entry_home,
                    a: 1,
                    ..
                },
                Instruction::Or {
                    a: copied_home,
                    s: 3,
                    b: 3,
                },
                Instruction::LoadWord {
                    a: member_base, ..
                },
                Instruction::CompareWordImmediate { .. },
            ] if saved == initialized
                && saved == member_base
                && entry_home == copied_home
                && saved != entry_home
                && mwcc_vreg::Reg::is_virtual_field(*saved)
                && mwcc_vreg::Reg::is_virtual_field(*entry_home) =>
            {
                Some((start, *saved))
            }
            _ => None,
        }
    })
}

/// Redirect a forward branch through any forward-only unconditional branch at
/// its destination. Nested diamonds initially target their own join; after the
/// parent diamond is complete that join may itself be the parent's
/// skip-to-continuation branch. This applies equally to a conditional false
/// edge and an unconditional arm exit.
pub(super) fn thread_forward_unconditional_branch_chains(instructions: &mut [Instruction]) {
    for index in 0..instructions.len() {
        let target = match instructions[index] {
            Instruction::Branch { target }
            | Instruction::BranchConditionalForward { target, .. } => target,
            _ => continue,
        };
        let mut destination = target;
        let mut remaining = instructions.len();
        while destination > index && remaining != 0 {
            let Some(Instruction::Branch { target: next }) = instructions.get(destination) else {
                break;
            };
            if *next <= destination {
                break;
            }
            destination = *next;
            remaining -= 1;
        }
        if destination != target {
            match &mut instructions[index] {
                Instruction::Branch { target }
                | Instruction::BranchConditionalForward { target, .. } => *target = destination,
                _ => unreachable!("the source branch was matched above"),
            }
        }
    }
}

fn is_repeated_member_address_call(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::AddImmediateCarryingRecord { d: 0, a: first_base, immediate: first_offset },
        Instruction::BranchConditionalForward { .. },
        Instruction::AddImmediateCarryingRecord { d: 0, a: second_base, immediate: second_offset },
        Instruction::BranchConditionalForward { .. },
        Instruction::AddImmediate { d: 3, a: call_base, immediate: call_offset },
        Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
        Instruction::BranchAndLink { .. },
    ] if first_base == second_base
        && first_base == call_base
        && first_offset == second_offset
        && first_offset == call_offset)
}

fn is_entry_member_call_argument_reload(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::Or { a: saved, s: 3, b: 3 },
        Instruction::LoadWord { d: 4, a: 3, offset: compared_offset },
        Instruction::LoadWord { d: 0, a: 3, .. },
        Instruction::CompareWord { a: 4, b: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::Or { a: 3, s: call_receiver, b: call_receiver_again },
        Instruction::LoadWord { d: 4, a: call_base, offset: call_offset },
        Instruction::BranchAndLink { .. },
    ] if saved == call_receiver
        && saved == call_receiver_again
        && saved == call_base
        && compared_offset == call_offset)
}

fn is_guarded_member_receiver_reload(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::Or { a: saved, s: entry, b: entry_again },
        Instruction::LoadWord { d: tested, a: test_base, offset: test_offset },
        Instruction::CompareLogicalWordImmediate { a: compared, immediate: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord { d: 3, a: call_base, offset: call_offset },
        Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
        Instruction::AddImmediate { d: 6, a: 0, immediate: 0 },
        Instruction::BranchAndLink { .. },
    ] if saved != entry
        && entry == entry_again
        && test_base == entry
        && tested == compared
        && *tested != 3
        && call_base == saved
        && test_offset == call_offset)
}

fn is_guarded_member_classifier_chain(window: &[Instruction]) -> bool {
    matches!(window, [
        Instruction::AddImmediate { d: saved, a: entry, immediate: 0 },
        Instruction::LoadWord { d: tested, a: test_base, offset: test_offset },
        Instruction::CompareLogicalWordImmediate { a: compared, immediate: 0 },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord { d: 3, a: classifier_base, offset: classifier_offset },
        Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 0 },
        Instruction::CompareLogicalWordImmediate { a: 0, .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord { d: 3, a: kind_base, offset: kind_offset },
        Instruction::BranchAndLink { .. },
        Instruction::CompareWordImmediate { a: 3, .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord { d: 3, a: final_base, offset: final_offset },
        _,
        Instruction::BranchAndLink { .. },
    ] if saved != entry
        && tested == compared
        && test_base == entry
        && classifier_base == saved
        && kind_base == saved
        && final_base == saved
        && test_offset == classifier_offset
        && test_offset == kind_offset
        && test_offset == final_offset)
}

fn reuses_preceding_member_load(instructions: &[Instruction], term_start: usize) -> bool {
    let Some(previous) = term_start.checked_sub(4) else {
        return false;
    };
    let Some([
        Instruction::LoadWord {
            d: previous_result,
            a: previous_base,
            offset: previous_offset,
        },
        Instruction::LoadByteZero { a: tested_base, .. },
        Instruction::CompareLogicalWordImmediate { .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadWord {
            d: current_result,
            a: current_base,
            offset: current_offset,
        },
        ..
    ]) = instructions.get(previous..)
    else {
        return false;
    };
    previous_result == current_result
        && previous_base == current_base
        && previous_offset == current_offset
        && tested_base == previous_result
}

fn reuses_preceding_bitfield_storage(instructions: &[Instruction], term_start: usize) -> bool {
    let Some(previous) = term_start.checked_sub(3) else {
        return false;
    };
    matches!(instructions.get(previous..), Some([
        Instruction::LoadByteZero {
            d: 0,
            a: previous_base,
            offset: previous_offset,
        },
        Instruction::RotateAndMaskRecord { a: 0, s: 0, .. },
        Instruction::BranchConditionalForward { .. },
        Instruction::LoadByteZero {
            d: 0,
            a: current_base,
            offset: current_offset,
        },
        Instruction::RotateAndMaskRecord { a: 0, s: 0, .. },
        ..
    ]) if previous_base == current_base && previous_offset == current_offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_entry_member_home_live_past_a_call() {
        let saved = mwcc_vreg::Reg::general(0).to_field();
        let instructions = [
            Instruction::LoadWord {
                d: saved,
                a: 3,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 0,
                a: saved,
                offset: 224,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 6,
            },
            Instruction::BranchAndLink {
                target: "predicate".into(),
            },
            Instruction::Or {
                a: 3,
                s: saved,
                b: saved,
            },
            Instruction::BranchAndLink {
                target: "consume".into(),
            },
        ];

        assert_eq!(
            find_entry_member_saved_home(&instructions),
            Some((0, saved))
        );
    }

    #[test]
    fn rejects_an_entry_member_value_dead_after_the_first_call() {
        let saved = mwcc_vreg::Reg::general(0).to_field();
        let instructions = [
            Instruction::LoadWord {
                d: saved,
                a: 3,
                offset: 44,
            },
            Instruction::LoadWord {
                d: 0,
                a: saved,
                offset: 224,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 6,
            },
            Instruction::BranchAndLink {
                target: "predicate".into(),
            },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(find_entry_member_saved_home(&instructions), None);
    }

    #[test]
    fn recognizes_a_member_initializer_staggered_between_two_saved_homes() {
        let saved = mwcc_vreg::Reg::general(0).to_field();
        let entry_home = mwcc_vreg::Reg::general(1).to_field();
        let instructions = [
            Instruction::StoreWord {
                s: saved,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: saved,
                a: 3,
                offset: 44,
            },
            Instruction::StoreWord {
                s: entry_home,
                a: 1,
                offset: 16,
            },
            Instruction::Or {
                a: entry_home,
                s: 3,
                b: 3,
            },
            Instruction::LoadWord {
                d: 0,
                a: saved,
                offset: 224,
            },
            Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        ];

        assert_eq!(
            find_staggered_entry_member_saved_home(&instructions),
            Some((0, saved))
        );
    }

    #[test]
    fn threads_a_nested_diamond_skip_through_its_parent_skip() {
        let mut instructions = vec![
            Instruction::Branch { target: 2 },
            Instruction::load_immediate(3, 1),
            Instruction::Branch { target: 4 },
            Instruction::load_immediate(3, 2),
            Instruction::BranchToLinkRegister,
        ];

        thread_forward_unconditional_branch_chains(&mut instructions);

        assert_eq!(instructions[0], Instruction::Branch { target: 4 });
        assert_eq!(instructions[2], Instruction::Branch { target: 4 });
    }

    #[test]
    fn threads_a_conditional_false_edge_through_an_arm_exit() {
        let mut instructions = vec![
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 2,
            },
            Instruction::load_immediate(3, 1),
            Instruction::Branch { target: 4 },
            Instruction::load_immediate(3, 2),
            Instruction::BranchToLinkRegister,
        ];

        thread_forward_unconditional_branch_chains(&mut instructions);

        assert_eq!(
            instructions[0],
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: 4,
            }
        );
    }

    #[test]
    fn recognizes_a_false_edge_skip_over_a_loop_backedge() {
        let instructions = [
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 2,
            },
            Instruction::Branch { target: 0 },
            Instruction::BranchToLinkRegister,
        ];

        assert_eq!(
            conditional_goto_diamond(&instructions, 0),
            Some((12, 2, 0))
        );
    }

    #[test]
    fn recognizes_a_guard_receiver_reloaded_through_its_saved_owner() {
        let instructions = [
            Instruction::Or { a: 31, s: 3, b: 3 },
            Instruction::LoadWord { d: 0, a: 3, offset: 8352 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 12 },
            Instruction::LoadWord { d: 3, a: 31, offset: 8352 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 6, a: 0, immediate: 0 },
            Instruction::BranchAndLink { target: "callee".into() },
        ];
        assert!(is_guarded_member_receiver_reload(&instructions));
    }

    #[test]
    fn recognizes_entry_member_arguments_reloaded_for_a_guarded_call() {
        let instructions = [
            Instruction::Or {
                a: 31,
                s: 3,
                b: 3,
            },
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: 448,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: 464,
            },
            Instruction::CompareWord { a: 4, b: 0 },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 8,
            },
            Instruction::Or {
                a: 3,
                s: 31,
                b: 31,
            },
            Instruction::LoadWord {
                d: 4,
                a: 31,
                offset: 448,
            },
            Instruction::BranchAndLink {
                target: "setup".into(),
            },
        ];

        assert!(is_entry_member_call_argument_reload(&instructions));
    }

    #[test]
    fn recognizes_a_guarded_member_classifier_call_chain() {
        let instructions = [
            Instruction::AddImmediate { d: 30, a: 3, immediate: 0 },
            Instruction::LoadWord { d: 0, a: 3, offset: 6516 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 15 },
            Instruction::LoadWord { d: 3, a: 30, offset: 6516 },
            Instruction::LoadHalfwordZero { d: 0, a: 3, offset: 0 },
            Instruction::CompareLogicalWordImmediate { a: 0, immediate: 6 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 15 },
            Instruction::LoadWord { d: 3, a: 30, offset: 6516 },
            Instruction::BranchAndLink { target: "kind".into() },
            Instruction::CompareWordImmediate { a: 3, immediate: 12 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 15 },
            Instruction::LoadWord { d: 3, a: 30, offset: 6516 },
            Instruction::Or { a: 4, s: 31, b: 31 },
            Instruction::BranchAndLink { target: "consume".into() },
        ];
        assert!(is_guarded_member_classifier_chain(&instructions));
    }

    #[test]
    fn recognizes_a_member_base_live_across_the_first_false_edge() {
        let instructions = [
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 392,
            },
            Instruction::LoadByteZero {
                d: 0,
                a: 3,
                offset: 36,
            },
            Instruction::CompareLogicalWordImmediate {
                a: 0,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 12,
                condition_bit: 2,
                target: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: 392,
            },
        ];
        assert!(reuses_preceding_member_load(&instructions, 4));
    }

    #[test]
    fn recognizes_bitfield_storage_live_across_the_first_false_edge() {
        let instructions = [
            Instruction::LoadByteZero { d: 0, a: 31, offset: 8729 },
            Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 26, begin: 31, end: 31 },
            Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: 8 },
            Instruction::LoadByteZero { d: 0, a: 31, offset: 8729 },
            Instruction::RotateAndMaskRecord { a: 0, s: 0, shift: 30, begin: 31, end: 31 },
        ];
        assert!(reuses_preceding_bitfield_storage(&instructions, 3));
    }

    #[test]
    fn recognizes_nested_member_address_guards_feeding_a_call() {
        let instructions = [
            Instruction::AddImmediateCarryingRecord { d: 0, a: 31, immediate: 64 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 7 },
            Instruction::AddImmediateCarryingRecord { d: 0, a: 31, immediate: 64 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 7 },
            Instruction::AddImmediate { d: 3, a: 31, immediate: 64 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 0 },
            Instruction::BranchAndLink { target: "__dt__6CTokenFv".to_string() },
        ];
        assert!(is_repeated_member_address_call(&instructions));
    }
}
