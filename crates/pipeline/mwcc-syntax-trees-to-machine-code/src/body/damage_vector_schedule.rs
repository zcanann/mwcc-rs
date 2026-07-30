//! Final lifetime schedule for damage-angle vector construction.
//!
//! The generic stream reloads the applied knockback before the angle call and
//! reloads pi on the lower arm of the facing-direction adjustment. MWCC carries
//! both values through those edges. Removing the two reloads also produces its
//! compact 56-byte frame and its f30/f31 role assignment.

#[allow(unused_imports)]
use super::*;

struct Shape {
    knockback_offset: i16,
    facing_offset: i16,
    collision_x_offset: i16,
    collision_y_offset: i16,
    gobj_offset: i16,
    common_force_offset: i16,
    source_player_offset: i16,
    game_mode_call: String,
    angle_call: String,
    position_call: String,
    item_call: String,
    get_player_value_call: String,
    set_player_value_call: String,
}

impl Generator {
    pub(crate) fn schedule_damage_vector_transaction(&mut self) {
        let Some((start, shape)) = self
            .output
            .instructions
            .windows(73)
            .enumerate()
            .find_map(|(start, window)| recognize(window, start).map(|shape| (start, shape)))
        else {
            return;
        };
        if !expected_relocations(self, start) {
            return;
        }

        for relative in [32, 16] {
            crate::remove_instruction_retargeting_to_next(self, start + relative);
        }
        self.output.instructions[start..start + 71].clone_from_slice(&scheduled(&shape, start));
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 37 => start + 34,
                index if index == start + 40 => start + 42,
                index if index == start + 41 => start + 40,
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
        .filter(|relocation| (start..start + 73).contains(&relocation.instruction_index))
        .map(|relocation| (relocation.instruction_index - start, relocation.kind))
        .collect::<Vec<_>>();
    relative
        == [
            (9, RelocationKind::Rel24),
            (13, RelocationKind::EmbSda21),
            (19, RelocationKind::Rel24),
            (22, RelocationKind::EmbSda21),
            (25, RelocationKind::EmbSda21),
            (28, RelocationKind::EmbSda21),
            (32, RelocationKind::EmbSda21),
            (39, RelocationKind::EmbSda21),
            (42, RelocationKind::EmbSda21),
            (43, RelocationKind::EmbSda21),
            (47, RelocationKind::Rel24),
            (54, RelocationKind::Rel24),
            (60, RelocationKind::Rel24),
            (64, RelocationKind::Rel24),
        ]
        && [22, 39].into_iter().all(|index| {
            schedule_relocations::same_relocated_value(
                &generator.output.relocations,
                &generator.output.constants,
                start + 13,
                start + index,
            )
        })
        && schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 25,
            start + 32,
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
    }, Instruction::StoreFloatDouble {
        s: 30,
        a: 1,
        offset: 48,
    }, Instruction::StoreWord {
        s: 31,
        a: 1,
        offset: 44,
    }, Instruction::AddImmediate {
        d: 31,
        a: 4,
        immediate: 0,
    }, Instruction::StoreWord {
        s: 30,
        a: 1,
        offset: 40,
    }, Instruction::AddImmediate {
        d: 30,
        a: 3,
        immediate: 0,
    }, Instruction::BranchAndLink {
        target: game_mode_call,
    }, Instruction::CompareWordImmediate { a: 3, immediate: 0 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: early_exit,
    }, Instruction::LoadFloatSingle {
        d: 1,
        a: 30,
        offset: knockback_offset,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatCompareUnordered { a: 1, b: 0 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: zero_knockback,
    }, Instruction::LoadFloatSingle {
        d: 31,
        a: 30,
        offset: second_knockback_offset,
    }, Instruction::Or { a: 3, s: 30, b: 30 }, Instruction::FloatMove { d: 1, b: 31 }, Instruction::BranchAndLink { target: angle_call }, Instruction::FloatMove { d: 30, b: 1 }, Instruction::LoadFloatSingle {
        d: 31,
        a: 30,
        offset: facing_offset,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatCompareOrdered { a: 31, b: 0 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 1,
        target: vector_start,
    }, Instruction::LoadFloatDouble {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatCompareOrdered { a: 30, b: 0 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 1,
        target: lower_angle,
    }, Instruction::LoadFloatDouble {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatSubtractDouble { d: 30, a: 0, b: 30 }, Instruction::RoundToSingle { d: 30, b: 30 }, Instruction::Branch {
        target: second_vector_start,
    }, Instruction::LoadFloatDouble {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatSubtractDouble { d: 30, a: 0, b: 30 }, Instruction::RoundToSingle { d: 30, b: 30 }, Instruction::LoadFloatSingle {
        d: 0,
        a: 30,
        offset: collision_x_offset,
    }, Instruction::StoreFloatSingle {
        s: 0,
        a: 1,
        offset: 16,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 30,
        offset: collision_y_offset,
    }, Instruction::StoreFloatSingle {
        s: 0,
        a: 1,
        offset: 20,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::StoreFloatSingle {
        s: 0,
        a: 1,
        offset: 24,
    }, Instruction::Branch {
        target: arguments_start,
    }, Instruction::LoadFloatSingle {
        d: 30,
        a: 0,
        offset: 0,
    }, Instruction::LoadWord {
        d: 3,
        a: 0,
        offset: 0,
    }, Instruction::LoadFloatSingle {
        d: 31,
        a: 3,
        offset: common_force_offset,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: gobj_offset,
    }, Instruction::AddImmediate {
        d: 4,
        a: 1,
        immediate: 16,
    }, Instruction::BranchAndLink {
        target: position_call,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: second_gobj_offset,
    }, Instruction::AddImmediate {
        d: 4,
        a: 1,
        immediate: 16,
    }, Instruction::Or { a: 5, s: 31, b: 31 }, Instruction::AddImmediate {
        d: 6,
        a: 0,
        immediate: 1,
    }, Instruction::FloatMove { d: 1, b: 30 }, Instruction::FloatMove { d: 2, b: 31 }, Instruction::BranchAndLink { target: item_call }, Instruction::AddImmediate {
        d: 31,
        a: 3,
        immediate: 0,
    }, Instruction::LoadWord {
        d: 0,
        a: 30,
        offset: source_player_offset,
    }, Instruction::CompareWordImmediate { a: 0, immediate: 6 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: second_early_exit,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: second_source_player_offset,
    }, Instruction::BranchAndLink {
        target: get_player_value_call,
    }, Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 0,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: third_source_player_offset,
    }, Instruction::Add { d: 4, a: 31, b: 0 }, Instruction::BranchAndLink {
        target: set_player_value_call,
    }, Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 68,
    }, Instruction::LoadFloatDouble {
        d: 31,
        a: 1,
        offset: 56,
    }, Instruction::LoadFloatDouble {
        d: 30,
        a: 1,
        offset: 48,
    }, Instruction::LoadWord {
        d: 31,
        a: 1,
        offset: 44,
    }, Instruction::LoadWord {
        d: 30,
        a: 1,
        offset: 40,
    }, Instruction::AddImmediate {
        d: 1,
        a: 1,
        immediate: 64,
    }, Instruction::MoveToLinkRegister { s: 0 }, Instruction::BranchToLinkRegister] = window
    else {
        return None;
    };

    (knockback_offset == second_knockback_offset
        && collision_x_offset.checked_add(4) == Some(*collision_y_offset)
        && gobj_offset == second_gobj_offset
        && source_player_offset == second_source_player_offset
        && source_player_offset == third_source_player_offset
        && early_exit == &(start + 65)
        && second_early_exit == early_exit
        && zero_knockback == &(start + 42)
        && vector_start == &(start + 35)
        && second_vector_start == vector_start
        && lower_angle == &(start + 32)
        && arguments_start == &(start + 48))
        .then(|| Shape {
            knockback_offset: *knockback_offset,
            facing_offset: *facing_offset,
            collision_x_offset: *collision_x_offset,
            collision_y_offset: *collision_y_offset,
            gobj_offset: *gobj_offset,
            common_force_offset: *common_force_offset,
            source_player_offset: *source_player_offset,
            game_mode_call: game_mode_call.clone(),
            angle_call: angle_call.clone(),
            position_call: position_call.clone(),
            item_call: item_call.clone(),
            get_player_value_call: get_player_value_call.clone(),
            set_player_value_call: set_player_value_call.clone(),
        })
}

fn scheduled(shape: &Shape, start: usize) -> [Instruction; 71] {
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
            offset: -56,
        },
        Instruction::StoreFloatDouble {
            s: 31,
            a: 1,
            offset: 48,
        },
        Instruction::StoreFloatDouble {
            s: 30,
            a: 1,
            offset: 40,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 36,
        },
        Instruction::AddImmediate {
            d: 31,
            a: 4,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: 30,
            a: 1,
            offset: 32,
        },
        Instruction::AddImmediate {
            d: 30,
            a: 3,
            immediate: 0,
        },
        Instruction::BranchAndLink {
            target: shape.game_mode_call.clone(),
        },
        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 63,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 30,
            offset: shape.knockback_offset,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::FloatCompareUnordered { a: 1, b: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 40,
        },
        Instruction::FloatMove { d: 30, b: 1 },
        Instruction::move_register(3, 30),
        Instruction::BranchAndLink {
            target: shape.angle_call.clone(),
        },
        Instruction::LoadFloatSingle {
            d: 2,
            a: 30,
            offset: shape.facing_offset,
        },
        Instruction::FloatMove { d: 31, b: 1 },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: start + 33,
        },
        Instruction::LoadFloatDouble {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::FloatCompareOrdered { a: 31, b: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 1,
            target: start + 31,
        },
        Instruction::LoadFloatDouble {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::FloatSubtractDouble { d: 31, a: 0, b: 31 },
        Instruction::RoundToSingle { d: 31, b: 31 },
        Instruction::Branch { target: start + 33 },
        Instruction::FloatSubtractDouble { d: 31, a: 0, b: 31 },
        Instruction::RoundToSingle { d: 31, b: 31 },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 30,
            offset: shape.collision_x_offset,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::StoreFloatSingle {
            s: 1,
            a: 1,
            offset: 16,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 30,
            offset: shape.collision_y_offset,
        },
        Instruction::StoreFloatSingle {
            s: 1,
            a: 1,
            offset: 20,
        },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 1,
            offset: 24,
        },
        Instruction::Branch { target: start + 46 },
        Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 16,
        },
        Instruction::LoadFloatSingle {
            d: 31,
            a: 0,
            offset: 0,
        },
        Instruction::LoadFloatSingle {
            d: 30,
            a: 3,
            offset: shape.common_force_offset,
        },
        Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: shape.gobj_offset,
        },
        Instruction::BranchAndLink {
            target: shape.position_call.clone(),
        },
        Instruction::FloatMove { d: 1, b: 31 },
        Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: shape.gobj_offset,
        },
        Instruction::FloatMove { d: 2, b: 30 },
        Instruction::AddImmediate {
            d: 5,
            a: 31,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: 16,
        },
        Instruction::AddImmediate {
            d: 6,
            a: 0,
            immediate: 1,
        },
        Instruction::BranchAndLink {
            target: shape.item_call.clone(),
        },
        Instruction::LoadWord {
            d: 0,
            a: 30,
            offset: shape.source_player_offset,
        },
        Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        },
        Instruction::CompareWordImmediate { a: 0, immediate: 6 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 63,
        },
        Instruction::move_register(3, 0),
        Instruction::BranchAndLink {
            target: shape.get_player_value_call.clone(),
        },
        Instruction::move_register(0, 3),
        Instruction::LoadWord {
            d: 3,
            a: 30,
            offset: shape.source_player_offset,
        },
        Instruction::Add { d: 4, a: 31, b: 0 },
        Instruction::BranchAndLink {
            target: shape.set_player_value_call.clone(),
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 60,
        },
        Instruction::LoadFloatDouble {
            d: 31,
            a: 1,
            offset: 48,
        },
        Instruction::LoadFloatDouble {
            d: 30,
            a: 1,
            offset: 40,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 36,
        },
        Instruction::LoadWord {
            d: 30,
            a: 1,
            offset: 32,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 56,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_knockback_and_pi_across_the_damage_edges() {
        let instructions = scheduled(
            &Shape {
                knockback_offset: 6224,
                facing_offset: 6212,
                collision_x_offset: 6228,
                collision_y_offset: 6232,
                gobj_offset: 0,
                common_force_offset: 1472,
                source_player_offset: 6340,
                game_mode_call: "mode".into(),
                angle_call: "angle".into(),
                position_call: "position".into(),
                item_call: "item".into(),
                get_player_value_call: "get".into(),
                set_player_value_call: "set".into(),
            },
            5,
        );

        assert!(matches!(
            instructions[16..=18],
            [
                Instruction::FloatMove { d: 30, b: 1 },
                Instruction::Or { a: 3, s: 30, b: 30 },
                Instruction::BranchAndLink { .. },
            ]
        ));
        assert!(matches!(
            instructions[24..=31],
            [
                Instruction::LoadFloatDouble { d: 0, .. },
                Instruction::FloatCompareOrdered { a: 31, b: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::LoadFloatDouble { d: 0, .. },
                Instruction::FloatSubtractDouble { d: 31, .. },
                Instruction::RoundToSingle { d: 31, b: 31 },
                Instruction::Branch { .. },
                Instruction::FloatSubtractDouble { d: 31, a: 0, b: 31 },
            ]
        ));
    }
}
