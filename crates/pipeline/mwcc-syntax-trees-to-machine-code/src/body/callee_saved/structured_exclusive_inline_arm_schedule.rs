//! Scheduling for two mutually exclusive statement-body inline expansions.
//!
//! Build 163 colors the caller receiver and the inline-local receiver into one
//! saved home in each arm, but retains the first arm's attributes across its
//! two calls. The second arm still contributes an optimizer value node for its
//! unused attributes without emitting the load. Both arms then use the same
//! store/call and callback-publication schedules.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn schedule_exclusive_inline_arms(&mut self, function: &Function) {
        if self.inline_statement_body_substitutions < 2
            || self.legacy_inline_expansion_frame_bytes == 0
            || !function.locals.iter().any(|local| {
                local.array_length.is_some()
                    && !super::structured_locals::body_uses_local(&function.statements, &local.name)
            })
        {
            return;
        }
        let Some(plan) = exclusive_inline_arm_plan(&self.output.instructions) else {
            return;
        };

        self.prefer_virtual_general(plan.entry, 29);
        self.prefer_virtual_general(plan.receiver, 30);
        self.prefer_virtual_general(plan.attributes, 31);
        self.legacy_callee_saved_frame_layout =
            LegacyCalleeSavedFrameLayout::RetainEntryParameterTable;

        // The second inline initializes an attribute local which its constant
        // call path never reads. Preserve the optimizer node, not its load.
        self.remove_structured_condition_instruction(plan.second_arm + 1);

        // load receiver; zero; first store; copy receiver; remaining stores
        self.move_instruction_before(plan.second_arm + 6, plan.second_arm + 3);
        schedule_inline_final_call(self, plan.second_arm + 8);
        overlap_callback_publication(self, plan.second_arm + 16);

        // zero fills the attribute-load latency; the receiver copy fills the
        // first store's issue slot.
        self.move_instruction_before(plan.first_arm + 2, plan.first_arm + 1);
        self.move_instruction_before(plan.first_arm + 7, plan.first_arm + 3);
        schedule_inline_final_call(self, plan.first_arm + 9);
        overlap_callback_publication(self, plan.first_arm + 17);

        // Save the highest-pressure attribute home first, followed by receiver
        // and entry. Copy the entry before its dependent receiver load.
        let saved_prefix = plan.first_arm - 11;
        self.move_instruction_before(saved_prefix + 4, saved_prefix);
        self.move_instruction_before(saved_prefix + 3, saved_prefix + 2);
        self.move_instruction_before(saved_prefix + 4, saved_prefix + 3);
        rank_scheduled_save_slots(&mut self.output.instructions[saved_prefix..saved_prefix + 3]);
    }
}

fn schedule_inline_final_call(generator: &mut Generator, start: usize) {
    // lfs f1; copy receiver; lfs f2; state; fmr f3,f1; flags; rate; call
    generator.move_instruction_before(start + 3, start);
    generator.move_instruction_before(start + 4, start + 2);
    generator.move_instruction_before(start + 5, start + 4);
}

fn overlap_callback_publication(generator: &mut Generator, start: usize) {
    // high1; low1; high2; store1; low2; store2
    generator.move_instruction_before(start + 3, start + 2);
}

fn rank_scheduled_save_slots(saves: &mut [Instruction]) {
    let mut offsets: Vec<_> = saves
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::StoreWord { a: 1, offset, .. } => Some(*offset),
            _ => None,
        })
        .collect();
    if offsets.len() != saves.len() {
        return;
    }
    offsets.sort_unstable_by(|left, right| right.cmp(left));
    for (instruction, offset) in saves.iter_mut().zip(offsets) {
        let Instruction::StoreWord {
            a: 1,
            offset: save_offset,
            ..
        } = instruction
        else {
            unreachable!("scheduled save slot changed after validation");
        };
        *save_offset = offset;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExclusiveInlineArmPlan {
    first_arm: usize,
    second_arm: usize,
    entry: u8,
    receiver: u8,
    attributes: u8,
}

fn exclusive_inline_arm_plan(instructions: &[Instruction]) -> Option<ExclusiveInlineArmPlan> {
    (11..instructions.len().saturating_sub(46)).find_map(|first_arm| {
        let second_arm = first_arm + 24;
        let (entry, receiver, attributes, final_call) =
            first_inline_arm(&instructions[first_arm..first_arm + 24])?;
        let (second_entry, second_receiver, second_final_call) =
            second_inline_arm(&instructions[second_arm..second_arm + 23])?;
        if entry != second_entry
            || receiver != second_receiver
            || final_call != second_final_call
            || !recognizes_inline_entry_prefix(
                &instructions[first_arm - 11..first_arm],
                entry,
                receiver,
                attributes,
                second_arm,
            )
        {
            return None;
        }
        Some(ExclusiveInlineArmPlan {
            first_arm,
            second_arm,
            entry,
            receiver,
            attributes,
        })
    })
}

fn first_inline_arm(window: &[Instruction]) -> Option<(u8, u8, u8, &str)> {
    let [
        Instruction::LoadWord {
            d: receiver,
            a: entry,
            offset: 44,
        },
        Instruction::LoadWord {
            d: attributes,
            a: attribute_base,
            ..
        },
        zero,
        first_store,
        second_store,
        third_store,
        fourth_store,
        first_receiver,
        Instruction::BranchAndLink { .. },
        final_receiver,
        state,
        flags,
        Instruction::LoadFloatSingle { d: 1, .. },
        Instruction::LoadFloatSingle {
            d: 2,
            a: float_base,
            ..
        },
        Instruction::FloatMove { d: 3, b: 1 },
        rate,
        Instruction::BranchAndLink { target: final_call },
        callback_high_1,
        callback_low_1,
        callback_store_1,
        callback_high_2,
        callback_low_2,
        callback_store_2,
        Instruction::Branch { .. },
    ] = window
    else {
        return None;
    };
    if attribute_base != receiver
        || float_base != attributes
        || !is_zero(zero)
        || !stores_zero_in(
            [first_store, second_store, third_store, fourth_store],
            *receiver,
        )
        || !copies_to_argument(first_receiver, *receiver)
        || !copies_to_argument(final_receiver, *entry)
        || !loads_argument(state, 4)
        || !loads_argument(flags, 5)
        || !loads_argument(rate, 6)
        || !callback_pair(
            [
                callback_high_1,
                callback_low_1,
                callback_store_1,
                callback_high_2,
                callback_low_2,
                callback_store_2,
            ],
            *receiver,
        )
    {
        return None;
    }
    Some((*entry, *receiver, *attributes, final_call))
}

fn second_inline_arm(window: &[Instruction]) -> Option<(u8, u8, &str)> {
    let [
        Instruction::LoadWord {
            d: receiver,
            a: entry,
            offset: 44,
        },
        Instruction::LoadWord {
            d: unused_attributes,
            a: attribute_base,
            ..
        },
        zero,
        first_store,
        second_store,
        third_store,
        fourth_store,
        first_receiver,
        Instruction::BranchAndLink { .. },
        final_receiver,
        state,
        flags,
        Instruction::LoadFloatSingle { d: 1, .. },
        Instruction::LoadFloatSingle {
            d: 2,
            a: float_base,
            ..
        },
        Instruction::FloatMove { d: 3, b: 1 },
        rate,
        Instruction::BranchAndLink { target: final_call },
        callback_high_1,
        callback_low_1,
        callback_store_1,
        callback_high_2,
        callback_low_2,
        callback_store_2,
    ] = window
    else {
        return None;
    };
    if attribute_base != receiver
        || float_base == unused_attributes
        || !is_zero(zero)
        || !stores_zero_in(
            [first_store, second_store, third_store, fourth_store],
            *receiver,
        )
        || !copies_to_argument(first_receiver, *receiver)
        || !copies_to_argument(final_receiver, *entry)
        || !loads_argument(state, 4)
        || !loads_argument(flags, 5)
        || !loads_argument(rate, 6)
        || !callback_pair(
            [
                callback_high_1,
                callback_low_1,
                callback_store_1,
                callback_high_2,
                callback_low_2,
                callback_store_2,
            ],
            *receiver,
        )
        || window[2..]
            .iter()
            .flat_map(mwcc_vreg::register_operands)
            .any(|operand| operand.register == *unused_attributes)
    {
        return None;
    }
    Some((*entry, *receiver, final_call))
}

fn recognizes_inline_entry_prefix(
    window: &[Instruction],
    entry: u8,
    receiver: u8,
    attributes: u8,
    second_arm: usize,
) -> bool {
    matches!(
        window,
        [
            Instruction::StoreWord {
                s: saved_receiver,
                a: 1,
                ..
            },
            Instruction::LoadWord {
                d: loaded_receiver,
                a: 3,
                offset: 44,
            },
            Instruction::StoreWord {
                s: saved_entry,
                a: 1,
                ..
            },
            copied_entry,
            Instruction::StoreWord {
                s: saved_attributes,
                a: 1,
                ..
            },
            Instruction::BranchAndLink { .. },
            _,
            Instruction::BranchConditionalForward { .. },
            Instruction::LoadWord {
                a: guarded_receiver,
                ..
            },
            _,
            Instruction::BranchConditionalForward { target, .. },
        ] if *saved_receiver == receiver
            && *loaded_receiver == receiver
            && *saved_entry == entry
            && copies_from_entry(copied_entry, entry)
            && *saved_attributes == attributes
            && *guarded_receiver == receiver
            && *target == second_arm
    )
}

fn is_zero(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediate {
            d: 0,
            a: 0,
            immediate: 0
        }
    )
}

fn stores_zero_in(stores: [&Instruction; 4], receiver: u8) -> bool {
    stores.into_iter().all(|instruction| {
        matches!(
            instruction,
            Instruction::StoreWord { s: 0, a, .. } if *a == receiver
        )
    })
}

fn copies_to_argument(instruction: &Instruction, source: u8) -> bool {
    matches!(
        instruction,
        Instruction::Or { a: 3, s, b } if *s == source && *b == source
    )
}

fn copies_from_entry(instruction: &Instruction, destination: u8) -> bool {
    matches!(
        instruction,
        Instruction::Or { a, s: 3, b: 3 } if *a == destination
    )
}

fn loads_argument(instruction: &Instruction, register: u8) -> bool {
    matches!(
        instruction,
        Instruction::AddImmediate { d, a: 0, .. } if *d == register
    )
}

fn callback_pair(callbacks: [&Instruction; 6], receiver: u8) -> bool {
    let [
        Instruction::AddImmediateShifted {
            d: first_high,
            a: 0,
            ..
        },
        Instruction::AddImmediate {
            d: first_low,
            a: first_low_base,
            ..
        },
        Instruction::StoreWord {
            s: first_stored,
            a: first_store_base,
            ..
        },
        Instruction::AddImmediateShifted {
            d: second_high,
            a: 0,
            ..
        },
        Instruction::AddImmediate {
            d: second_low,
            a: second_low_base,
            ..
        },
        Instruction::StoreWord {
            s: second_stored,
            a: second_store_base,
            ..
        },
    ] = callbacks
    else {
        return false;
    };
    first_high == first_low_base
        && first_low == first_stored
        && second_high == second_low_base
        && second_low == second_stored
        && *first_store_base == receiver
        && *second_store_base == receiver
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_descending_slots_to_scheduled_save_pressure() {
        let mut saves = [
            Instruction::StoreWord {
                s: 34,
                a: 1,
                offset: 36,
            },
            Instruction::StoreWord {
                s: 32,
                a: 1,
                offset: 44,
            },
            Instruction::StoreWord {
                s: 33,
                a: 1,
                offset: 40,
            },
        ];

        rank_scheduled_save_slots(&mut saves);

        assert!(matches!(
            saves,
            [
                Instruction::StoreWord {
                    s: 34,
                    offset: 44,
                    ..
                },
                Instruction::StoreWord {
                    s: 32,
                    offset: 40,
                    ..
                },
                Instruction::StoreWord {
                    s: 33,
                    offset: 36,
                    ..
                },
            ]
        ));
    }
}
