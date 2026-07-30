//! Final lifetime schedule for a guarded item-charge update.
//!
//! MWCC keeps the integer clamp bound in r3 through the true edge and writes it
//! directly, while the generic stream reloads both the global and its bound.
//! It also keeps the entity lookup result in r3 briefly enough to fold the null
//! test into `mr.`. Together those choices determine the saved-GPR roles and
//! the two integer-conversion stack images used by the rest of the transaction.

#[allow(unused_imports)]
use super::*;

struct Shape {
    user_data_offset: i16,
    scale_offset: i16,
    item_guard_offset: i16,
    charge_offset: i16,
    charge_bound_offset: i16,
    fractional_charge_offset: i16,
    item_data_offset: i16,
    item_kind_table_offset: i16,
    item_kind_offset: i16,
    position_offset: i16,
    facing_offset: i16,
    sound_id: i16,
    sound_volume: i16,
    sound_pan: i16,
    update_call: String,
    create_call: String,
    attach_call: String,
    sound_call: String,
}

impl Generator {
    pub(crate) fn schedule_guarded_item_charge(&mut self) {
        let Some((start, shape)) = self
            .output
            .instructions
            .windows(75)
            .enumerate()
            .find_map(|(start, window)| recognize(window, start).map(|shape| (start, shape)))
        else {
            return;
        };
        if !expected_relocations(self, start) {
            return;
        }

        for relative in [28, 27] {
            crate::remove_instruction_retargeting_to_next(self, start + relative);
        }
        self.output.instructions[start..start + 73].clone_from_slice(&scheduled(&shape, start));
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 10 => start + 9,
                index if index == start + 22 => start + 23,
                index if index == start + 32 => start + 31,
                index if index == start + 42 => start + 43,
                index if index == start + 50 => start + 51,
                index => index,
            };
        }
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
    }
}

fn expected_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| (start..start + 75).contains(&relocation.instruction_index))
        .map(|relocation| (relocation.instruction_index - start, relocation.kind))
        .collect::<Vec<_>>();
    relative
        == [
            (10, RelocationKind::EmbSda21),
            (22, RelocationKind::EmbSda21),
            (27, RelocationKind::EmbSda21),
            (34, RelocationKind::EmbSda21),
            (44, RelocationKind::Rel24),
            (52, RelocationKind::Rel24),
            (59, RelocationKind::Rel24),
            (61, RelocationKind::Rel24),
            (66, RelocationKind::Rel24),
        ]
        && schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 22,
            start + 27,
        )
}

fn recognize(window: &[Instruction], start: usize) -> Option<Shape> {
    let [Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 4,
    }, Instruction::StoreWordWithUpdate {
        s: 1,
        a: 1,
        offset: -64,
    }, Instruction::StoreFloatDouble {
        s: 31,
        a: 1,
        offset: 56,
    }, Instruction::StoreWord {
        s: 31,
        a: 1,
        offset: 52,
    }, Instruction::StoreWord {
        s: 30,
        a: 1,
        offset: 48,
    }, Instruction::StoreWord {
        s: 29,
        a: 1,
        offset: 44,
    }, Instruction::Or { a: 29, s: 3, b: 3 }, Instruction::FloatMove { d: 31, b: 1 }, Instruction::LoadWord {
        d: 30,
        a: 29,
        offset: user_data_offset,
    }, Instruction::LoadWord {
        d: 3,
        a: 0,
        offset: 0,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 3,
        offset: scale_offset,
    }, Instruction::FloatMultiplySingle { d: 0, a: 31, c: 0 }, Instruction::ConvertToIntegerWordZero { d: 0, b: 0 }, Instruction::StoreFloatDouble {
        s: 0,
        a: 1,
        offset: 16,
    }, Instruction::LoadWord {
        d: 31,
        a: 1,
        offset: 20,
    }, Instruction::LoadWord {
        d: 0,
        a: 30,
        offset: item_guard_offset,
    }, Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: else_start,
    }, Instruction::LoadWord {
        d: 0,
        a: 30,
        offset: charge_offset,
    }, Instruction::Add { d: 0, a: 0, b: 31 }, Instruction::StoreWord {
        s: 0,
        a: 30,
        offset: second_charge_offset,
    }, Instruction::LoadWord {
        d: 4,
        a: 0,
        offset: 0,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: third_charge_offset,
    }, Instruction::LoadWord {
        d: 0,
        a: 4,
        offset: charge_bound_offset,
    }, Instruction::CompareWord { a: 3, b: 0 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 1,
        target: clamp_done,
    }, Instruction::LoadWord {
        d: 3,
        a: 0,
        offset: 0,
    }, Instruction::LoadWord {
        d: 0,
        a: 3,
        offset: second_charge_bound_offset,
    }, Instruction::StoreWord {
        s: 0,
        a: 30,
        offset: fourth_charge_offset,
    }, Instruction::LoadWord {
        d: 31,
        a: 30,
        offset: fractional_charge_offset,
    }, Instruction::XorImmediateShifted {
        a: 0,
        s: 31,
        immediate: 0x8000,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 12,
    }, Instruction::AddImmediateShifted {
        d: 0,
        a: 0,
        immediate: 0x4330,
    }, Instruction::LoadFloatDouble {
        d: 2,
        a: 0,
        offset: 0,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 8,
    }, Instruction::LoadFloatDouble {
        d: 0,
        a: 1,
        offset: 8,
    }, Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 }, Instruction::FloatAddSingle { d: 0, a: 0, b: 31 }, Instruction::ConvertToIntegerWordZero { d: 0, b: 0 }, Instruction::StoreFloatDouble {
        s: 0,
        a: 1,
        offset: 24,
    }, Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 28,
    }, Instruction::StoreWord {
        s: 0,
        a: 30,
        offset: second_fractional_charge_offset,
    }, Instruction::Or { a: 3, s: 29, b: 29 }, Instruction::BranchAndLink {
        target: update_call,
    }, Instruction::Branch {
        target: update_done,
    }, Instruction::Or { a: 3, s: 29, b: 29 }, Instruction::AddImmediate {
        d: 4,
        a: 30,
        immediate: position_offset,
    }, Instruction::LoadWord {
        d: 5,
        a: 30,
        offset: item_data_offset,
    }, Instruction::LoadWord {
        d: 5,
        a: 5,
        offset: item_kind_table_offset,
    }, Instruction::LoadByteZero {
        d: 5,
        a: 5,
        offset: item_kind_offset,
    }, Instruction::LoadFloatSingle {
        d: 1,
        a: 30,
        offset: facing_offset,
    }, Instruction::BranchAndLink {
        target: create_call,
    }, Instruction::AddImmediate {
        d: 4,
        a: 3,
        immediate: 0,
    }, Instruction::CompareLogicalWordImmediate { a: 4, immediate: 0 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: second_update_done,
    }, Instruction::Or { a: 3, s: 29, b: 29 }, Instruction::Or { a: 5, s: 31, b: 31 }, Instruction::FloatMove { d: 1, b: 31 }, Instruction::BranchAndLink {
        target: attach_call,
    }, Instruction::Or { a: 3, s: 29, b: 29 }, Instruction::BranchAndLink {
        target: second_update_call,
    }, Instruction::AddImmediate {
        d: 3,
        a: 30,
        immediate: 0,
    }, Instruction::AddImmediate {
        d: 4,
        a: 0,
        immediate: sound_id,
    }, Instruction::AddImmediate {
        d: 5,
        a: 0,
        immediate: sound_volume,
    }, Instruction::AddImmediate {
        d: 6,
        a: 0,
        immediate: sound_pan,
    }, Instruction::BranchAndLink { target: sound_call }, Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 68,
    }, Instruction::LoadFloatDouble {
        d: 31,
        a: 1,
        offset: 56,
    }, Instruction::LoadWord {
        d: 30,
        a: 1,
        offset: 52,
    }, Instruction::LoadWord {
        d: 31,
        a: 1,
        offset: 48,
    }, Instruction::LoadWord {
        d: 29,
        a: 1,
        offset: 44,
    }, Instruction::AddImmediate {
        d: 1,
        a: 1,
        immediate: 64,
    }, Instruction::MoveToLinkRegister { s: 0 }, Instruction::BranchToLinkRegister] = window
    else {
        return None;
    };

    (charge_offset == second_charge_offset
        && charge_offset == third_charge_offset
        && charge_offset == fourth_charge_offset
        && charge_bound_offset == second_charge_bound_offset
        && fractional_charge_offset == second_fractional_charge_offset
        && update_call == second_update_call
        && else_start == &(start + 46)
        && clamp_done == &(start + 30)
        && update_done == &(start + 62)
        && second_update_done == update_done)
        .then(|| Shape {
            user_data_offset: *user_data_offset,
            scale_offset: *scale_offset,
            item_guard_offset: *item_guard_offset,
            charge_offset: *charge_offset,
            charge_bound_offset: *charge_bound_offset,
            fractional_charge_offset: *fractional_charge_offset,
            item_data_offset: *item_data_offset,
            item_kind_table_offset: *item_kind_table_offset,
            item_kind_offset: *item_kind_offset,
            position_offset: *position_offset,
            facing_offset: *facing_offset,
            sound_id: *sound_id,
            sound_volume: *sound_volume,
            sound_pan: *sound_pan,
            update_call: update_call.clone(),
            create_call: create_call.clone(),
            attach_call: attach_call.clone(),
            sound_call: sound_call.clone(),
        })
}

fn scheduled(shape: &Shape, start: usize) -> [Instruction; 73] {
    [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        },
        Instruction::StoreWordWithUpdate {
            s: 1,
            a: 1,
            offset: -64,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 56,
        },
        Instruction::FloatMove { d: 31, b: 1 },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 52,
        },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 48,
        },
        Instruction::StoreWord {
            s: 29,
            a: 1,
            offset: 44,
        },
        Instruction::move_register(29, 3),
        Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        },
        Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.user_data_offset,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 4,
            offset: shape.scale_offset,
        },
        Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.item_guard_offset,
        },
        Instruction::move_register(31, 3),
        Instruction::FloatMultiplySingle { d: 0, a: 31, c: 0 },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 },
        Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
        Instruction::StoreFloatDouble {
            s: 0,
            a: 1,
            offset: 32,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 36,
        },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 45,
        },
        Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: shape.charge_offset,
        },
        Instruction::Add { d: 0, a: 0, b: 30 },
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: shape.charge_offset,
        },
        Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        },
        Instruction::LoadWord {
            d: 0,
            a: 31,
            offset: shape.charge_offset,
        },
        Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.charge_bound_offset,
        },
        Instruction::CompareWord { a: 0, b: 3 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: start + 29,
        },
        Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: shape.charge_offset,
        },
        Instruction::LoadWord {
            d: 4,
            a: 31,
            offset: shape.fractional_charge_offset,
        },
        Instruction::AddImmediateShifted {
            d: 0,
            a: 0,
            immediate: 0x4330,
        },
        Instruction::LoadFloatDouble {
            d: 1,
            a: 0,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 0,
        },
        Instruction::XorImmediateShifted {
            a: 4,
            s: 4,
            immediate: 0x8000,
        },
        Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 36,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 32,
        },
        Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 32,
        },
        Instruction::FloatSubtractSingle { d: 0, a: 0, b: 1 },
        Instruction::FloatAddSingle { d: 0, a: 0, b: 31 },
        Instruction::ConvertToIntegerWordZero { d: 0, b: 0 },
        Instruction::StoreFloatDouble {
            s: 0,
            a: 1,
            offset: 24,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        },
        Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: shape.fractional_charge_offset,
        },
        Instruction::BranchAndLink {
            target: shape.update_call.clone(),
        },
        Instruction::Branch { target: start + 60 },
        Instruction::LoadWord {
            d: 5,
            a: 31,
            offset: shape.item_data_offset,
        },
        Instruction::move_register(3, 29),
        Instruction::LoadFloatSingle {
            d: 1,
            a: 31,
            offset: shape.facing_offset,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 31,
            immediate: shape.position_offset,
        },
        Instruction::LoadWord {
            d: 5,
            a: 5,
            offset: shape.item_kind_table_offset,
        },
        Instruction::LoadByteZero {
            d: 5,
            a: 5,
            offset: shape.item_kind_offset,
        },
        Instruction::BranchAndLink {
            target: shape.create_call.clone(),
        },
        Instruction::OrRecord { a: 4, s: 3, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 60,
        },
        Instruction::FloatMove { d: 1, b: 31 },
        Instruction::AddImmediate {
            d: 3,
            a: 29,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 5,
            a: 30,
            immediate: 0,
        },
        Instruction::BranchAndLink {
            target: shape.attach_call.clone(),
        },
        Instruction::move_register(3, 29),
        Instruction::BranchAndLink {
            target: shape.update_call.clone(),
        },
        Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 0,
            immediate: shape.sound_id,
        },
        Instruction::AddImmediate {
            d: 5,
            a: 0,
            immediate: shape.sound_volume,
        },
        Instruction::AddImmediate {
            d: 6,
            a: 0,
            immediate: shape.sound_pan,
        },
        Instruction::BranchAndLink {
            target: shape.sound_call.clone(),
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 68,
        },
        Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 56,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 52,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 48,
        },
        Instruction::LoadWord {
            d: 29,
            a: 1,
            offset: 44,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 64,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_clamp_bound_and_records_the_item_result() {
        let instructions = scheduled(
            &Shape {
                user_data_offset: 44,
                scale_offset: 1796,
                item_guard_offset: 6528,
                charge_offset: 8216,
                charge_bound_offset: 1792,
                fractional_charge_offset: 8228,
                item_data_offset: 268,
                item_kind_table_offset: 8,
                item_kind_offset: 18,
                position_offset: 176,
                facing_offset: 44,
                sound_id: 287,
                sound_volume: 127,
                sound_pan: 64,
                update_call: "update".into(),
                create_call: "create".into(),
                attach_call: "attach".into(),
                sound_call: "sound".into(),
            },
            11,
        );

        assert!(matches!(
            instructions[23..=28],
            [
                Instruction::LoadWord { d: 3, a: 0, .. },
                Instruction::LoadWord { d: 0, a: 31, .. },
                Instruction::LoadWord { d: 3, a: 3, .. },
                Instruction::CompareWord { a: 0, b: 3 },
                Instruction::BranchConditionalForward { target: 40, .. },
                Instruction::StoreWord { s: 3, a: 31, .. },
            ]
        ));
        assert!(matches!(
            instructions[52],
            Instruction::OrRecord { a: 4, s: 3, b: 3 }
        ));
    }
}
