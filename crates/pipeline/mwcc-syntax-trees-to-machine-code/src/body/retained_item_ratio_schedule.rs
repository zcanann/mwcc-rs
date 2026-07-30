//! Final lifetime image for a retained item ratio.
//!
//! In a three-way float selection, the generic stream loads the retained
//! member before each integer-returning item query. That makes the member and
//! the divisor overlap across the call, requiring both f30 and f31. MWCC delays
//! the member load until the integer conversion is underway, then writes the
//! product into the divisor's dead f31 home. This complete-shape owner performs
//! that lifetime contraction and the issue-order changes that follow from it.

#[allow(unused_imports)]
use super::*;

const UNSCHEDULED_LEN: usize = 88;
const SCHEDULED_LEN: usize = 86;

const ORDER: [usize; SCHEDULED_LEN] = [
    0, 6, 1, 2, 3, 12, 5, 7, 8, 10, 11, 9, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 31, 30, 32, 33, 34, 35, 36, 37, 38, 29, 39, 40, 41, 42, 43, 44, 47, 46, 48, 49, 50, 51,
    52, 53, 54, 45, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 67, 65, 68, 66, 69, 70, 71, 72, 75, 73,
    76, 74, 77, 78, 79, 80, 82, 83, 84, 85, 86, 87,
];

impl Generator {
    pub(crate) fn schedule_retained_item_ratio(&mut self) {
        if self.frame_size != 56 || self.callee_saved_float != 2 {
            return;
        }
        let Some(start) = self
            .output
            .instructions
            .windows(UNSCHEDULED_LEN)
            .enumerate()
            .find_map(|(start, window)| recognize(window, start).then_some(start))
        else {
            return;
        };
        if !expected_relocations(self, start)
            || !self.output.jump_tables.is_empty()
            || self
                .output
                .data_section_displacements
                .iter()
                .any(|displacement| {
                    displacement.instruction_index == start + 4
                        || displacement.instruction_index == start + 81
                })
        {
            return;
        }

        apply_order(self, start);
        rewrite(
            &mut self.output.instructions[start..start + SCHEDULED_LEN],
            start,
        );
        self.callee_saved_float = 1;
    }
}

fn expected_relocations(generator: &Generator, start: usize) -> bool {
    let relative = generator
        .output
        .relocations
        .iter()
        .filter(|relocation| {
            (start..start + UNSCHEDULED_LEN).contains(&relocation.instruction_index)
        })
        .map(|relocation| (relocation.instruction_index - start, relocation.kind))
        .collect::<Vec<_>>();
    relative
        == [
            (6, RelocationKind::Addr16Ha),
            (7, RelocationKind::Addr16Lo),
            (13, RelocationKind::Rel24),
            (19, RelocationKind::Rel24),
            (20, RelocationKind::EmbSda21),
            (26, RelocationKind::Rel24),
            (34, RelocationKind::Rel24),
            (36, RelocationKind::EmbSda21),
            (50, RelocationKind::Rel24),
            (52, RelocationKind::EmbSda21),
            (61, RelocationKind::Rel24),
        ]
        && schedule_relocations::same_target_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 6,
            start + 7,
        )
        && schedule_relocations::same_relocated_value(
            &generator.output.relocations,
            &generator.output.constants,
            start + 36,
            start + 52,
        )
}

fn recognize(window: &[Instruction], start: usize) -> bool {
    let [Instruction::MoveFromLinkRegister { d: 0 }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 4,
    }, Instruction::StoreWordWithUpdate {
        s: 1,
        a: 1,
        offset: -56,
    }, Instruction::StoreFloatDouble {
        s: 31,
        a: 1,
        offset: 48,
    }, Instruction::StoreFloatDouble {
        s: 30,
        a: 1,
        offset: 40,
    }, Instruction::StoreWord {
        s: 31,
        a: 1,
        offset: 36,
    }, Instruction::AddImmediateShifted { d: 5, a: 0, .. }, Instruction::AddImmediate {
        d: 31,
        a: 5,
        immediate: 0,
    }, Instruction::StoreWord {
        s: 30,
        a: 1,
        offset: 32,
    }, Instruction::LoadWord {
        d: 30,
        a: 3,
        offset: user_data_offset,
    }, Instruction::StoreWord {
        s: 29,
        a: 1,
        offset: 28,
    }, Instruction::Or { a: 29, s: 4, b: 4 }, Instruction::FloatMove { d: 31, b: 1 }, Instruction::BranchAndLink {
        target: status_call,
    }, Instruction::CompareWordImmediate {
        a: 3,
        immediate: -1,
    }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: assertion_end,
    }, Instruction::AddImmediate { d: 3, a: 31, .. }, Instruction::AddImmediate { d: 5, a: 31, .. }, Instruction::AddImmediate { d: 4, a: 0, .. }, Instruction::BranchAndLink {
        target: assert_call,
    }, Instruction::LoadFloatSingle {
        d: 0,
        a: 0,
        offset: 0,
    }, Instruction::FloatCompareUnordered { a: 0, b: 31 }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: nonzero,
    }, Instruction::LoadFloatSingle {
        d: 30,
        a: 30,
        offset: member_offset,
    }, Instruction::Branch {
        target: calculation_end,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: item_offset,
    }, Instruction::BranchAndLink { target: kind_call }, Instruction::CompareWordImmediate {
        a: 3,
        immediate: kind,
    }, Instruction::BranchConditionalForward {
        options: 4,
        condition_bit: 2,
        target: second_arm,
    }, tail @ ..] = window
    else {
        return false;
    };
    assertion_end == &(start + 20)
        && nonzero == &(start + 25)
        && calculation_end == &(start + 60)
        && second_arm == &(start + 45)
        && !status_call.is_empty()
        && !assert_call.is_empty()
        && recognize_ratio_arm(&tail[..16], *member_offset, *item_offset, Some(start + 60))
        && recognize_ratio_arm(&tail[16..31], *member_offset, *item_offset, None)
        && recognize_tail(&tail[31..35], start, *item_offset, *kind, kind_call)
        && recognize_callback(&tail[35..43], *item_offset, Some(start + 79))
        && recognize_callback(&tail[43..50], *item_offset, None)
        && recognize_epilogue(&tail[50..])
        && *user_data_offset != *item_offset
}

fn recognize_ratio_arm(
    window: &[Instruction],
    member_offset: i16,
    item_offset: i16,
    exit: Option<usize>,
) -> bool {
    let [Instruction::LoadFloatSingle {
        d: 30,
        a: 30,
        offset: arm_member_offset,
    }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset: arm_item_offset,
    }, Instruction::ShiftLeftImmediate {
        a: 4,
        s: 29,
        shift: 2,
    }, Instruction::Add { d: 4, a: 31, b: 4 }, Instruction::LoadWord { d: 4, a: 4, .. }, Instruction::BranchAndLink { target: ratio_call }, Instruction::XorImmediateShifted {
        a: 0,
        s: 3,
        immediate: 32768,
    }, Instruction::LoadFloatDouble {
        d: 2,
        a: 0,
        offset: 0,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 12,
    }, Instruction::AddImmediateShifted {
        d: 0,
        a: 0,
        immediate: 17200,
    }, Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 8,
    }, Instruction::LoadFloatDouble {
        d: 0,
        a: 1,
        offset: 8,
    }, Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 }, Instruction::FloatDivideSingle { d: 0, a: 0, b: 31 }, Instruction::FloatMultiplySingle { d: 30, a: 30, c: 0 }, rest @ ..] =
        window
    else {
        return false;
    };
    arm_member_offset == &member_offset
        && arm_item_offset == &item_offset
        && !ratio_call.is_empty()
        && match (rest, exit) {
            ([Instruction::Branch { target }], Some(exit)) => target == &exit,
            ([], None) => true,
            _ => false,
        }
}

fn recognize_tail(
    window: &[Instruction],
    start: usize,
    item_offset: i16,
    kind: i16,
    kind_call: &str,
) -> bool {
    matches!(window, [
        Instruction::LoadWord { d: 3, a: 30, offset },
        Instruction::BranchAndLink { target },
        Instruction::CompareWordImmediate { a: 3, immediate },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 2,
            target: callback,
        },
    ] if offset == &item_offset
        && target == kind_call
        && immediate == &kind
        && callback == &(start + 72))
}

fn recognize_callback(
    window: &[Instruction],
    item_offset: i16,
    exit: Option<usize>,
) -> bool {
    let [Instruction::ShiftLeftImmediate {
        a: 12,
        s: 29,
        shift: 2,
    }, Instruction::Add {
        d: 12,
        a: 31,
        b: 12,
    }, Instruction::LoadWord { d: 12, a: 12, .. }, Instruction::FloatMove { d: 1, b: 30 }, Instruction::LoadWord {
        d: 3,
        a: 30,
        offset,
    }, Instruction::MoveToLinkRegister { s: 12 }, Instruction::BranchToLinkRegisterAndLink, rest @ ..] =
        window
    else {
        return false;
    };
    offset == &item_offset
        && match (rest, exit) {
            ([Instruction::Branch { target }], Some(exit)) => target == &exit,
            ([], None) => true,
            _ => false,
        }
}

fn recognize_epilogue(window: &[Instruction]) -> bool {
    matches!(
        window,
        [
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 60
            },
            Instruction::LoadFloatDouble {
                d: 31,
                a: 1,
                offset: 48
            },
            Instruction::LoadFloatDouble {
                d: 30,
                a: 1,
                offset: 40
            },
            Instruction::LoadWord {
                d: 31,
                a: 1,
                offset: 36
            },
            Instruction::LoadWord {
                d: 30,
                a: 1,
                offset: 32
            },
            Instruction::LoadWord {
                d: 29,
                a: 1,
                offset: 28
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 56
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]
    )
}

fn apply_order(generator: &mut Generator, start: usize) {
    let old_len = generator.output.instructions.len();
    let old = generator.output.instructions[start..start + UNSCHEDULED_LEN].to_vec();
    let scheduled = ORDER
        .iter()
        .map(|relative| old[*relative].clone())
        .collect::<Vec<_>>();
    generator
        .output
        .instructions
        .splice(start..start + UNSCHEDULED_LEN, scheduled);

    let mut permutation = (0..old_len).collect::<Vec<_>>();
    for (new, old) in ORDER.iter().copied().enumerate() {
        permutation[start + old] = start + new;
    }
    permutation[start + 4] = permutation[start + 5];
    permutation[start + 81] = permutation[start + 82];
    for (old, new) in permutation
        .iter_mut()
        .enumerate()
        .skip(start + UNSCHEDULED_LEN)
    {
        *new = old - (UNSCHEDULED_LEN - SCHEDULED_LEN);
    }
    crate::remap_instruction_indices(generator, &permutation);
}

fn rewrite(window: &mut [Instruction], start: usize) {
    window[10] = Instruction::AddImmediate {
        d: 29,
        a: 4,
        immediate: 0,
    };
    rewrite_stack_offset(&mut window[6], 44);
    rewrite_stack_offset(&mut window[8], 40);
    rewrite_stack_offset(&mut window[9], 36);
    rewrite_float_destination(&mut window[22], 31);
    rewrite_ratio_arm(&mut window[28..44]);
    rewrite_ratio_arm(&mut window[44..59]);
    rewrite_callback(&mut window[63..71]);
    rewrite_callback(&mut window[71..78]);
    rewrite_stack_offset(&mut window[80], 44);
    rewrite_stack_offset(&mut window[81], 40);
    rewrite_stack_offset(&mut window[82], 36);

    let Instruction::BranchConditionalForward { target, .. } = &mut window[27] else {
        unreachable!();
    };
    *target = start + 44;
}

fn rewrite_ratio_arm(window: &mut [Instruction]) {
    let Instruction::ShiftLeftImmediate { a, .. } = &mut window[0] else {
        unreachable!();
    };
    *a = 0;
    let Instruction::Add { b, .. } = &mut window[2] else {
        unreachable!();
    };
    *b = 0;
    rewrite_stack_offset(&mut window[7], 28);
    rewrite_float_destination(&mut window[9], 0);
    rewrite_stack_offset(&mut window[10], 24);
    let Instruction::LoadFloatDouble { d, offset, .. } = &mut window[11] else {
        unreachable!();
    };
    *d = 1;
    *offset = 24;
    let Instruction::FloatSubtractSingle { d, a, .. } = &mut window[12] else {
        unreachable!();
    };
    *d = 1;
    *a = 1;
    let Instruction::FloatDivideSingle { d, a, .. } = &mut window[13] else {
        unreachable!();
    };
    *d = 1;
    *a = 1;
    window[14] = Instruction::FloatMultiplySingle { d: 31, a: 0, c: 1 };
}

fn rewrite_callback(window: &mut [Instruction]) {
    let Instruction::ShiftLeftImmediate { a, .. } = &mut window[0] else {
        unreachable!();
    };
    *a = 0;
    let Instruction::FloatMove { b, .. } = &mut window[1] else {
        unreachable!();
    };
    *b = 31;
    window[2] = Instruction::Add { d: 4, a: 31, b: 0 };
    let Instruction::LoadWord { a, .. } = &mut window[4] else {
        unreachable!();
    };
    *a = 4;
}

fn rewrite_stack_offset(instruction: &mut Instruction, replacement: i16) {
    match instruction {
        Instruction::StoreWord { offset, .. } | Instruction::LoadWord { offset, .. } => {
            *offset = replacement;
        }
        _ => unreachable!(),
    }
}

fn rewrite_float_destination(instruction: &mut Instruction, replacement: u8) {
    let Instruction::LoadFloatSingle { d, .. } = instruction else {
        unreachable!();
    };
    *d = replacement;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contracts_the_two_float_homes_without_dropping_other_instructions() {
        assert_eq!(ORDER.len(), SCHEDULED_LEN);
        let mut retained = ORDER.to_vec();
        retained.sort_unstable();
        assert_eq!(
            retained,
            (0..UNSCHEDULED_LEN)
                .filter(|index| *index != 4 && *index != 81)
                .collect::<Vec<_>>()
        );
        assert_eq!(&ORDER[..13], &[0, 6, 1, 2, 3, 12, 5, 7, 8, 10, 11, 9, 13]);
        assert_eq!(&ORDER[78..], &[79, 80, 82, 83, 84, 85, 86, 87]);
    }

    #[test]
    fn rewrites_one_ratio_arm_into_the_dead_divisor_home() {
        let mut arm = vec![
            Instruction::ShiftLeftImmediate {
                a: 4,
                s: 29,
                shift: 2,
            },
            Instruction::LoadWord {
                d: 3,
                a: 30,
                offset: 6516,
            },
            Instruction::Add { d: 4, a: 31, b: 4 },
            Instruction::LoadWord {
                d: 4,
                a: 4,
                offset: 184,
            },
            Instruction::BranchAndLink {
                target: "ratio".into(),
            },
            Instruction::XorImmediateShifted {
                a: 0,
                s: 3,
                immediate: 32768,
            },
            Instruction::LoadFloatDouble {
                d: 2,
                a: 0,
                offset: 0,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::AddImmediateShifted {
                d: 0,
                a: 0,
                immediate: 17200,
            },
            Instruction::LoadFloatSingle {
                d: 30,
                a: 30,
                offset: 2204,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::LoadFloatDouble {
                d: 0,
                a: 1,
                offset: 8,
            },
            Instruction::FloatSubtractSingle { d: 0, a: 0, b: 2 },
            Instruction::FloatDivideSingle { d: 0, a: 0, b: 31 },
            Instruction::FloatMultiplySingle { d: 30, a: 30, c: 0 },
        ];

        rewrite_ratio_arm(&mut arm);

        assert!(matches!(
            arm[9..=14],
            [
                Instruction::LoadFloatSingle { d: 0, .. },
                Instruction::StoreWord { offset: 24, .. },
                Instruction::LoadFloatDouble {
                    d: 1,
                    offset: 24,
                    ..
                },
                Instruction::FloatSubtractSingle { d: 1, a: 1, .. },
                Instruction::FloatDivideSingle { d: 1, a: 1, b: 31 },
                Instruction::FloatMultiplySingle { d: 31, a: 0, c: 1 },
            ]
        ));
    }
}
