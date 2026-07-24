use super::*;

fn conversion_pair() -> Vec<Instruction> {
    vec![
        Instruction::XorImmediateShifted {
            a: 3,
            s: 3,
            immediate: 0x8000,
        },
        Instruction::AddImmediateShifted {
            d: 0,
            a: 0,
            immediate: 0x4330,
        },
        Instruction::AddImmediateShifted {
            d: 5,
            a: 0,
            immediate: 0,
        },
        Instruction::LoadFloatDouble {
            d: 1,
            a: 5,
            offset: 0,
        },
        Instruction::StoreWord {
            s: 3,
            a: 1,
            offset: 12,
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
        Instruction::FloatSubtractSingle { d: 1, a: 0, b: 1 },
        Instruction::XorImmediateShifted {
            a: 4,
            s: 4,
            immediate: 0x8000,
        },
        Instruction::AddImmediateShifted {
            d: 0,
            a: 0,
            immediate: 0x4330,
        },
        Instruction::AddImmediateShifted {
            d: 6,
            a: 0,
            immediate: 0,
        },
        Instruction::LoadFloatDouble {
            d: 2,
            a: 6,
            offset: 0,
        },
        Instruction::StoreWord {
            s: 4,
            a: 1,
            offset: 20,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        },
        Instruction::LoadFloatDouble {
            d: 0,
            a: 1,
            offset: 16,
        },
        Instruction::FloatSubtractSingle { d: 2, a: 0, b: 2 },
    ]
}

#[test]
fn recognizes_adjacent_signed_conversion_images() {
    assert_eq!(signed_conversion_pair(&conversion_pair()), Some(0));
}

#[test]
fn rejects_a_second_image_with_the_wrong_low_word() {
    let mut instructions = conversion_pair();
    let Instruction::StoreWord { s, .. } = &mut instructions[12] else {
        unreachable!()
    };
    *s = 7;

    assert_eq!(signed_conversion_pair(&instructions), None);
}
