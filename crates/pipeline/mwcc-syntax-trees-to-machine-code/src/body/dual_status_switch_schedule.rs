//! Dense/sparse dispatch schedule for two guarded item-status switches.
//!
//! Structured CFG normalization currently turns nested return switches into
//! equality ladders before the generic switch emitter can select a jump table.
//! This complete-shape late owner restores MWCC's dense seven-case table for the
//! first status family and its balanced three-case tree for the second.

#[allow(unused_imports)]
use super::*;
use mwcc_machine_code::{JumpTable, Relocation, RelocationTarget};

struct Shape {
    user_data_offset: i16,
    item_offset: i16,
    first_kind: i16,
    second_kind: i16,
    kind_call: String,
    status_call: String,
}

impl Generator {
    pub(crate) fn schedule_dual_status_switches(&mut self) {
        let Some((start, shape)) = self
            .output
            .instructions
            .windows(77)
            .enumerate()
            .find_map(|(start, window)| recognize(window, start).map(|shape| (start, shape)))
        else {
            return;
        };
        if !expected_relocations(self, start)
            || !self.output.jump_tables.is_empty()
            || self
                .output
                .data_section_displacements
                .iter()
                .any(|displacement| (start..start + 77).contains(&displacement.instruction_index))
        {
            return;
        }

        for _ in 0..14 {
            crate::remove_instruction_retargeting_to_next(self, start + 63);
        }
        self.output.instructions[start..start + 63].clone_from_slice(&scheduled(&shape, start));
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start + 9 => start + 8,
                index if index == start + 13 => start + 12,
                index if index == start + 52 => start + 39,
                index if index == start + 56 => start + 43,
                index => index,
            };
        }
        self.output.relocations.extend([
            Relocation {
                instruction_index: start + 16,
                kind: RelocationKind::Addr16Ha,
                target: RelocationTarget::JumpTable,
            },
            Relocation {
                instruction_index: start + 17,
                kind: RelocationKind::Addr16Lo,
                target: RelocationTarget::JumpTable,
            },
        ]);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);

        self.output.jump_tables.push(JumpTable {
            entries: [34, 30, 32, 22, 24, 26, 28]
                .into_iter()
                .map(|relative| ((start + relative) * 4) as u32)
                .collect(),
            anonymous_offset: 8,
        });
    }
}

fn expected_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| (start..start + 77).contains(&relocation.instruction_index))
        .map(|relocation| (relocation.instruction_index - start, relocation.kind))
        .collect::<Vec<_>>();
    relative
        == [
            (9, RelocationKind::Rel24),
            (13, RelocationKind::Rel24),
            (52, RelocationKind::Rel24),
            (56, RelocationKind::Rel24),
        ]
        && schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 9,
            start + 52,
        )
        && schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 13,
            start + 56,
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
        offset: -24,
    }, Instruction::StoreWord {
        s: 31,
        a: 1,
        offset: 20,
    }, Instruction::LoadWord {
        d: 31,
        a: 3,
        offset: user_data_offset,
    }, Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: item_offset,
    }, Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: first_default,
    }, Instruction::LoadWord {
        d: 3,
        a: 31,
        offset: second_item_offset,
    }, Instruction::BranchAndLink { target: kind_call }, Instruction::CompareWordImmediate {
        a: 3,
        immediate: first_kind,
    }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: second_first_default,
    }, Instruction::LoadWord {
        d: 3,
        a: 31,
        offset: third_item_offset,
    }, Instruction::BranchAndLink {
        target: status_call,
    }, first_ladder @ ..] = window
    else {
        return None;
    };
    if item_offset != second_item_offset
        || item_offset != third_item_offset
        || first_default != &(start + 48)
        || second_first_default != first_default
        || first_ladder.len() != 63
        || !recognize_first_ladder(&first_ladder[..34], start)
    {
        return None;
    }

    let [Instruction::LoadWord {
        d: 0,
        a: 31,
        offset: fourth_item_offset,
    }, Instruction::CompareLogicalWordImmediate { a: 0, immediate: 0 }, Instruction::BranchConditionalForward {
        options: 12,
        condition_bit: 2,
        target: second_default,
    }, Instruction::LoadWord {
        d: 3,
        a: 31,
        offset: fifth_item_offset,
    }, Instruction::BranchAndLink {
        target: second_kind_call,
    }, Instruction::CompareWordImmediate {
        a: 3,
        immediate: second_kind,
    }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: third_default,
    }, Instruction::LoadWord {
        d: 3,
        a: 31,
        offset: sixth_item_offset,
    }, Instruction::BranchAndLink {
        target: second_status_call,
    }, second_ladder @ ..] = &first_ladder[34..]
    else {
        return None;
    };
    if item_offset != fourth_item_offset
        || item_offset != fifth_item_offset
        || item_offset != sixth_item_offset
        || kind_call != second_kind_call
        || status_call != second_status_call
        || second_default != &(start + 71)
        || third_default != second_default
        || !recognize_second_ladder(second_ladder, start)
    {
        return None;
    }

    Some(Shape {
        user_data_offset: *user_data_offset,
        item_offset: *item_offset,
        first_kind: *first_kind,
        second_kind: *second_kind,
        kind_call: kind_call.clone(),
        status_call: status_call.clone(),
    })
}

fn recognize_first_ladder(window: &[Instruction], start: usize) -> bool {
    let comparisons = [4, 5, 6, 7, 8, 9, 10];
    let results = [6, 4, 5, 0, 1, 2, 3];
    let mut cursor = 0;
    for (case_index, (&comparison, &result)) in comparisons.iter().zip(results.iter()).enumerate() {
        let last = case_index + 1 == comparisons.len();
        let width = if last { 4 } else { 5 };
        let Some(group) = window.get(cursor..cursor + width) else {
            return false;
        };
        if !matches!(
            group,
            [
                Instruction::CompareWordImmediate { a: 3, immediate },
                Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target,
                },
                Instruction::AddImmediate {
                    d: 3,
                    a: 0,
                    immediate: loaded,
                },
                Instruction::Branch { target: exit },
                ..
            ] if *immediate == comparison
                && *loaded == result
                && *target == if last { start + 48 } else { start + 19 + case_index * 5 }
                && *exit == start + 72
        ) {
            return false;
        }
        if !last
            && !matches!(
                group[4],
                Instruction::Branch { target } if target == start + 48
            )
        {
            return false;
        }
        cursor += width;
    }
    cursor == window.len()
}

fn recognize_second_ladder(window: &[Instruction], start: usize) -> bool {
    matches!(
        window,
        [
            Instruction::CompareWordImmediate {
                a: 3,
                immediate: 0,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: first_next,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 6,
            },
            Instruction::Branch { target: first_exit },
            Instruction::Branch { target: first_default },
            Instruction::CompareWordImmediate {
                a: 3,
                immediate: 1,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: second_next,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 4,
            },
            Instruction::Branch { target: second_exit },
            Instruction::Branch { target: second_default },
            Instruction::CompareWordImmediate {
                a: 3,
                immediate: 2,
            },
            Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: third_default,
            },
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 6,
            },
            Instruction::Branch { target: third_exit },
            Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: -1,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ] if *first_next == start + 62
            && *second_next == start + 67
            && *first_exit == start + 72
            && *second_exit == start + 72
            && *third_exit == start + 72
            && *first_default == start + 71
            && *second_default == start + 71
            && *third_default == start + 71
    )
}

fn scheduled(shape: &Shape, start: usize) -> [Instruction; 63] {
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
            offset: -24,
        },
        Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 20,
        },
        Instruction::LoadWord {
            d: 31,
            a: 3,
            offset: shape.user_data_offset,
        },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.item_offset,
        },
        Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 36,
        },
        Instruction::BranchAndLink {
            target: shape.kind_call.clone(),
        },
        Instruction::CompareWordImmediate {
            a: 3,
            immediate: shape.first_kind,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: start + 36,
        },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.item_offset,
        },
        Instruction::BranchAndLink {
            target: shape.status_call.clone(),
        },
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: -4,
        },
        Instruction::CompareLogicalWordImmediate { a: 0, immediate: 6 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 1,
            target: start + 36,
        },
        Instruction::AddImmediateShifted {
            d: 3,
            a: 0,
            immediate: 0,
        },
        Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: 0,
        },
        Instruction::ShiftLeftImmediate {
            a: 0,
            s: 0,
            shift: 2,
        },
        Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 },
        Instruction::MoveToCountRegister { s: 0 },
        Instruction::BranchToCountRegister,
        Instruction::load_immediate(3, 0),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 1),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 2),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 3),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 4),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 5),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 6),
        Instruction::Branch { target: start + 58 },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.item_offset,
        },
        Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 57,
        },
        Instruction::BranchAndLink {
            target: shape.kind_call.clone(),
        },
        Instruction::CompareWordImmediate {
            a: 3,
            immediate: shape.second_kind,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: start + 57,
        },
        Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.item_offset,
        },
        Instruction::BranchAndLink {
            target: shape.status_call.clone(),
        },
        Instruction::CompareWordImmediate { a: 3, immediate: 1 },
        Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: start + 53,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: start + 50,
        },
        Instruction::CompareWordImmediate { a: 3, immediate: 0 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: start + 55,
        },
        Instruction::Branch { target: start + 57 },
        Instruction::CompareWordImmediate { a: 3, immediate: 3 },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: start + 57,
        },
        Instruction::Branch { target: start + 55 },
        Instruction::load_immediate(3, 4),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, 6),
        Instruction::Branch { target: start + 58 },
        Instruction::load_immediate(3, -1),
        Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        },
        Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 20,
        },
        Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        },
        Instruction::MoveToLinkRegister { s: 0 },
        Instruction::BranchToLinkRegister,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_dense_table_and_sparse_tree_images() {
        let instructions = scheduled(
            &Shape {
                user_data_offset: 44,
                item_offset: 6516,
                first_kind: 13,
                second_kind: 103,
                kind_call: "kind".into(),
                status_call: "status".into(),
            },
            0,
        );

        assert!(matches!(
            instructions[16..=21],
            [
                Instruction::AddImmediateShifted { d: 3, .. },
                Instruction::AddImmediate { d: 3, a: 3, .. },
                Instruction::ShiftLeftImmediate { a: 0, s: 0, .. },
                Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 },
                Instruction::MoveToCountRegister { s: 0 },
                Instruction::BranchToCountRegister,
            ]
        ));
        assert!(matches!(
            instructions[44..=52],
            [
                Instruction::CompareWordImmediate { a: 3, immediate: 1 },
                Instruction::BranchConditionalForward { .. },
                Instruction::BranchConditionalForward { .. },
                Instruction::CompareWordImmediate { a: 3, immediate: 0 },
                Instruction::BranchConditionalForward { .. },
                Instruction::Branch { .. },
                Instruction::CompareWordImmediate { a: 3, immediate: 3 },
                Instruction::BranchConditionalForward { .. },
                Instruction::Branch { .. },
            ]
        ));
    }
}
