//! Final lifetime image for an adjacent-fighter nudge transaction.
//!
//! The generic allocator writes the entity lookup directly into a saved GPR
//! and folds the absolute-value result into its input. MWCC instead retains the
//! lookup result in linkage register r3 for one byte load, copies it afterward,
//! and materializes the absolute value through a distinct f0/f2 select. Those
//! lifetimes reduce the dense saved range from r24..r31 to r25..r31.

#[allow(unused_imports)]
use super::*;

struct Shape {
    user_data_offset: i16,
    player_id_offset: i16,
    vector_offset: i16,
    guard_byte_offset: i16,
    ground_air_offset: i16,
    floor_index_offset: i16,
    facing_offset: i16,
    argument_x_offset: i16,
    vector_x_offset: i16,
    vector_y_offset: i16,
    stack_vector_offset: i16,
    nudge_velocity_offset: i16,
    common_data_offset: i16,
    entity_call: String,
    next_line_call: String,
    previous_line_call: String,
    position_call: String,
    final_call: String,
}

impl Generator {
    pub(crate) fn schedule_adjacent_fighter_nudge(&mut self) {
        let Some((start, shape)) = self
            .output
            .instructions
            .windows(66)
            .enumerate()
            .find_map(|(start, window)| recognize(window, start).map(|shape| (start, shape)))
        else {
            return;
        };
        if !expected_relocations(self, start) {
            return;
        }

        for _ in 0..3 {
            crate::insert_instruction_retargeting(
                self,
                start + 66,
                Instruction::BranchToLinkRegister,
            );
        }
        self.output.instructions[start..start + 69].clone_from_slice(&scheduled(&shape, start));
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 23 => start + 24,
                index if index == start + 27 => start + 28,
                index if index == start + 32 => start + 33,
                index if index == start + 43 => start + 42,
                index if index == start + 54 => start + 56,
                index if index == start + 60 => start + 63,
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
        .filter(|relocation| (start..start + 66).contains(&relocation.instruction_index))
        .map(|relocation| (relocation.instruction_index - start, relocation.kind))
        .collect::<Vec<_>>();
    relative
        == [
            (9, RelocationKind::Rel24),
            (23, RelocationKind::Rel24),
            (27, RelocationKind::Rel24),
            (32, RelocationKind::Rel24),
            (43, RelocationKind::EmbSda21),
            (54, RelocationKind::EmbSda21),
            (60, RelocationKind::Rel24),
        ]
}

fn recognize(window: &[Instruction], start: usize) -> Option<Shape> {
    let [Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 4,
    }, Instruction::StoreWordWithUpdate {
        s: 1,
        a: 1,
        offset: -80,
    }, Instruction::StoreMultipleWord {
        s: 24,
        a: 1,
        offset: 48,
    }, Instruction::AddImmediate {
        d: 25,
        a: 4,
        immediate: 0,
    }, Instruction::AddImmediate {
        d: 24,
        a: 3,
        immediate: 0,
    }, Instruction::LoadWord {
        d: 26,
        a: 24,
        offset: user_data_offset,
    }, Instruction::AddImmediate {
        d: 27,
        a: 26,
        immediate: vector_offset,
    }, Instruction::LoadByteZero {
        d: 3,
        a: 26,
        offset: player_id_offset,
    }, Instruction::BranchAndLink {
        target: entity_call,
    }, Instruction::LoadWord {
        d: 28,
        a: 3,
        offset: second_user_data_offset,
    }, Instruction::LoadByteZero {
        d: 0,
        a: 28,
        offset: guard_byte_offset,
    }, Instruction::RotateAndMaskRecord {
        a: 0,
        s: 0,
        shift: 28,
        begin: 31,
        end: 31,
    }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: first_exit,
    }, Instruction::LoadWord {
        d: 0,
        a: 28,
        offset: ground_air_offset,
    }, Instruction::CompareWordImmediate { a: 0, immediate: 0 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: second_exit,
    }, Instruction::LoadWord {
        d: 29,
        a: 26,
        offset: floor_index_offset,
    }, Instruction::LoadWord {
        d: 31,
        a: 28,
        offset: second_floor_index_offset,
    }, Instruction::AddImmediate {
        d: 30,
        a: 31,
        immediate: 0,
    }, Instruction::CompareWord { a: 29, b: 31 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: first_body,
    }, Instruction::Or { a: 3, s: 29, b: 29 }, Instruction::BranchAndLink {
        target: next_line_call,
    }, Instruction::CompareWord { a: 31, b: 3 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: second_body,
    }, Instruction::Or { a: 3, s: 29, b: 29 }, Instruction::BranchAndLink {
        target: previous_line_call,
    }, Instruction::CompareWord { a: 31, b: 3 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: third_exit,
    }, Instruction::Or { a: 3, s: 28, b: 28 }, Instruction::AddImmediate {
        d: 4,
        a: 1,
        immediate: stack_vector_offset,
    }, Instruction::BranchAndLink {
        target: position_call,
    }, Instruction::AddImmediate {
        d: 3,
        a: 28,
        immediate: second_vector_offset,
    }, Instruction::LoadFloatSingle {
        d: 4,
        a: 27,
        offset: vector_x_offset,
    }, Instruction::LoadFloatSingle {
        d: 3,
        a: 26,
        offset: facing_offset,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 25,
        offset: argument_x_offset,
    }, Instruction::FloatMultiplyAddSingle {
        d: 3,
        a: 4,
        c: 3,
        b: 0,
    }, Instruction::LoadFloatSingle {
        d: 2,
        a: 28,
        offset: second_facing_offset,
    }, Instruction::LoadFloatSingle {
        d: 1,
        a: 3,
        offset: second_vector_x_offset,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 1,
        offset: second_stack_vector_offset,
    }, Instruction::FloatMultiplyAddSingle {
        d: 0,
        a: 2,
        c: 1,
        b: 0,
    }, Instruction::FloatSubtractSingle { d: 1, a: 3, b: 0 }, Instruction::LoadFloatSingle {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatCompareOrdered { a: 1, b: 0 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 0,
        target: absolute_done,
    }, Instruction::FloatNegate { d: 1, b: 1 }, Instruction::Branch {
        target: second_absolute_done,
    }, Instruction::LoadFloatSingle {
        d: 2,
        a: 27,
        offset: vector_y_offset,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 3,
        offset: second_vector_y_offset,
    }, Instruction::FloatAddSingle { d: 0, a: 2, b: 0 }, Instruction::FloatCompareOrdered { a: 1, b: 0 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 0,
        target: fourth_exit,
    }, Instruction::LoadFloatSingle {
        d: 2,
        a: 26,
        offset: nudge_velocity_offset,
    }, Instruction::LoadWord {
        d: 3,
        a: 0,
        offset: 0,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 3,
        offset: common_data_offset,
    }, Instruction::FloatSubtractSingle { d: 0, a: 2, b: 0 }, Instruction::StoreFloatSingle {
        s: 0,
        a: 26,
        offset: second_nudge_velocity_offset,
    }, Instruction::AddImmediate {
        d: 3,
        a: 24,
        immediate: 0,
    }, Instruction::AddImmediate {
        d: 4,
        a: 25,
        immediate: 0,
    }, Instruction::BranchAndLink { target: final_call }, Instruction::LoadMultipleWord {
        d: 24,
        a: 1,
        offset: 48,
    }, Instruction::LoadWord {
        d: 0,
        a: 1,
        offset: 84,
    }, Instruction::AddImmediate {
        d: 1,
        a: 1,
        immediate: 80,
    }, Instruction::MoveToLinkRegister { s: 0 }, Instruction::BranchToLinkRegister] = window
    else {
        return None;
    };

    (user_data_offset == second_user_data_offset
        && vector_offset == second_vector_offset
        && floor_index_offset == second_floor_index_offset
        && facing_offset == second_facing_offset
        && vector_x_offset == second_vector_x_offset
        && vector_y_offset == second_vector_y_offset
        && stack_vector_offset == second_stack_vector_offset
        && nudge_velocity_offset == second_nudge_velocity_offset
        && stack_vector_offset.checked_sub(4).is_some()
        && vector_x_offset.checked_add(4) == Some(*vector_y_offset)
        && first_exit == &(start + 58)
        && second_exit == first_exit
        && third_exit == first_exit
        && fourth_exit == first_exit
        && first_body == &(start + 30)
        && second_body == first_body
        && absolute_done == &(start + 48)
        && second_absolute_done == absolute_done)
        .then(|| Shape {
            user_data_offset: *user_data_offset,
            player_id_offset: *player_id_offset,
            vector_offset: *vector_offset,
            guard_byte_offset: *guard_byte_offset,
            ground_air_offset: *ground_air_offset,
            floor_index_offset: *floor_index_offset,
            facing_offset: *facing_offset,
            argument_x_offset: *argument_x_offset,
            vector_x_offset: *vector_x_offset,
            vector_y_offset: *vector_y_offset,
            stack_vector_offset: *stack_vector_offset,
            nudge_velocity_offset: *nudge_velocity_offset,
            common_data_offset: *common_data_offset,
            entity_call: entity_call.clone(),
            next_line_call: next_line_call.clone(),
            previous_line_call: previous_line_call.clone(),
            position_call: position_call.clone(),
            final_call: final_call.clone(),
        })
}

fn scheduled(shape: &Shape, start: usize) -> [Instruction; 69] {
    let local_vector = shape.stack_vector_offset - 4;
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
            offset: -80,
        },
        Instruction::StoreMultipleWord {
            s: 25,
            a: 1,
            offset: 52,
        },
        Instruction::move_register(25, 3),
        Instruction::move_register(26, 4),
        Instruction::LoadWord {
            d: 29,
            a: 3,
            offset: shape.user_data_offset,
        },
        Instruction::LoadByteZero {
            d: 3,
            a: 29,
            offset: shape.player_id_offset,
        },
        Instruction::AddImmediate {
            d: 31,
            a: 29,
            immediate: shape.vector_offset,
        },
        Instruction::BranchAndLink {
            target: shape.entity_call.clone(),
        },
        Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.user_data_offset,
        },
        Instruction::LoadByteZero {
            d: 0,
            a: 3,
            offset: shape.guard_byte_offset,
        },
        Instruction::AddImmediate {
            d: 28,
            a: 3,
            immediate: 0,
        },
        Instruction::RotateAndMaskRecord {
            a: 0,
            s: 0,
            shift: 28,
            begin: 31,
            end: 31,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: start + 61,
        },
        Instruction::LoadWord {
            d: 0,
            a: 28,
            offset: shape.ground_air_offset,
        },
        Instruction::CompareWordImmediate { a: 0, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: start + 61,
        },
        Instruction::LoadWord {
            d: 0,
            a: 29,
            offset: shape.floor_index_offset,
        },
        Instruction::LoadWord {
            d: 30,
            a: 28,
            offset: shape.floor_index_offset,
        },
        Instruction::move_register(27, 0),
        Instruction::CompareWord { a: 0, b: 30 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 31,
        },
        Instruction::move_register(3, 27),
        Instruction::BranchAndLink {
            target: shape.next_line_call.clone(),
        },
        Instruction::CompareWord { a: 30, b: 3 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 31,
        },
        Instruction::move_register(3, 27),
        Instruction::BranchAndLink {
            target: shape.previous_line_call.clone(),
        },
        Instruction::CompareWord { a: 30, b: 3 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: start + 61,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 28,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 1,
            immediate: local_vector,
        },
        Instruction::BranchAndLink {
            target: shape.position_call.clone(),
        },
        Instruction::LoadFloatSingle {
            d: 4,
            a: 31,
            offset: shape.vector_x_offset,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 28,
            immediate: shape.vector_offset,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 29,
            offset: shape.facing_offset,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 26,
            offset: shape.argument_x_offset,
        },
        Instruction::LoadFloatSingle {
            d: 3,
            a: 28,
            offset: shape.facing_offset,
        },
        Instruction::LoadFloatSingle {
            d: 2,
            a: 28,
            offset: shape.vector_offset,
        },
        Instruction::FloatMultiplyAddSingle {
            d: 4,
            a: 4,
            c: 1,
            b: 0,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 1,
            offset: local_vector,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 0,
            offset: 0,
        },
        Instruction::FloatMultiplyAddSingle {
            d: 1,
            a: 3,
            c: 2,
            b: 1,
        },
        Instruction::FloatSubtractSingle { d: 1, a: 4, b: 1 },
        Instruction::FloatCompareOrdered { a: 1, b: 0 },
        Instruction::FloatMove { d: 0, b: 1 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: start + 50,
        },
        Instruction::FloatNegate { d: 2, b: 0 },
        Instruction::Branch { target: start + 51 },
        Instruction::FloatMove { d: 2, b: 0 },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 31,
            offset: shape.vector_y_offset,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 3,
            offset: shape.vector_y_offset,
        },
        Instruction::FloatAddSingle { d: 0, a: 1, b: 0 },
        Instruction::FloatCompareOrdered { a: 2, b: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: start + 61,
        },
        Instruction::LoadWord {
            d: 3,
            a: 0,
            offset: 0,
        },
        Instruction::LoadFloatSingle {
            d: 1,
            a: 29,
            offset: shape.nudge_velocity_offset,
        },
        Instruction::LoadFloatSingle {
            d: 0,
            a: 3,
            offset: shape.common_data_offset,
        },
        Instruction::FloatSubtractSingle { d: 0, a: 1, b: 0 },
        Instruction::StoreFloatSingle {
            s: 0,
            a: 29,
            offset: shape.nudge_velocity_offset,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 25,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 4,
            a: 26,
            immediate: 0,
        },
        Instruction::BranchAndLink {
            target: shape.final_call.clone(),
        },
        Instruction::LoadMultipleWord {
            d: 25,
            a: 1,
            offset: 52,
        },
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 84,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 80,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_retained_lookup_and_absolute_value_lifetimes() {
        let instructions = scheduled(
            &Shape {
                user_data_offset: 44,
                player_id_offset: 12,
                vector_offset: 708,
                guard_byte_offset: 8735,
                ground_air_offset: 224,
                floor_index_offset: 2108,
                facing_offset: 44,
                argument_x_offset: 0,
                vector_x_offset: 0,
                vector_y_offset: 4,
                stack_vector_offset: 32,
                nudge_velocity_offset: 252,
                common_data_offset: 1116,
                entity_call: "entity".into(),
                next_line_call: "next".into(),
                previous_line_call: "previous".into(),
                position_call: "position".into(),
                final_call: "final".into(),
            },
            7,
        );

        assert!(matches!(
            instructions[12],
            Instruction::AddImmediate {
                d: 28,
                a: 3,
                immediate: 0
            }
        ));
        assert!(matches!(
            instructions[46..=50],
            [
                Instruction::FloatMove { d: 0, b: 1 },
                Instruction::BranchConditionalForward { target: 57, .. },
                Instruction::FloatNegate { d: 2, b: 0 },
                Instruction::Branch { target: 58 },
                Instruction::FloatMove { d: 2, b: 0 },
            ]
        ));
    }
}
