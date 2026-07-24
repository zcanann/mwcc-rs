use super::*;

fn fixture() -> Vec<Instruction> {
    vec![
        Instruction::CompareWordImmediate {
            a: 14,
            immediate: 0,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 7,
        },
        Instruction::Negate { d: 7, a: 14 },
        Instruction::load_immediate(8, 0),
        Instruction::LoadHalfwordZero {
            d: 0,
            a: 26,
            offset: 6,
        },
        Instruction::Add { d: 5, a: 0, b: 14 },
        Instruction::Branch { target: 10 },
        Instruction::LoadHalfwordZero {
            d: 5,
            a: 26,
            offset: 6,
        },
        Instruction::move_register(8, 14),
        Instruction::load_immediate(7, 0),
    ]
}

#[test]
fn recognizes_an_independent_member_load_in_a_sign_clamp() {
    assert!(is_member_sign_clamp(&fixture(), 0));
}

#[test]
fn rejects_a_load_whose_base_is_written_before_it() {
    let mut instructions = fixture();
    let Instruction::LoadHalfwordZero { a, .. } = &mut instructions[4] else {
        unreachable!()
    };
    *a = 7;
    let Instruction::LoadHalfwordZero { a, .. } = &mut instructions[7] else {
        unreachable!()
    };
    *a = 7;

    assert!(!is_member_sign_clamp(&instructions, 0));
}
