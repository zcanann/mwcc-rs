use super::*;

fn serial_fixture() -> Vec<Instruction> {
    vec![
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 8,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        },
        Instruction::load_immediate_shifted(4, -4336),
        Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 3312,
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        },
        Instruction::load_immediate_shifted(4, 3866),
        Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: 29004,
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        },
        Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 20,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 8,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        },
        Instruction::load_immediate_shifted(4, -768),
        Instruction::AddImmediate {
            d: 0,
            a: 4,
            immediate: -7169,
        },
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        },
        Instruction::load_immediate(0, -1480),
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        },
        Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 20,
        },
        Instruction::AddImmediate {
            d: 0,
            a: 3,
            immediate: 8,
        },
        Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        },
        Instruction::load_immediate_shifted(0, -1280),
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 0,
        },
        Instruction::load_immediate(0, -224),
        Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: 4,
        },
        Instruction::LoadHalfwordZero {
            d: 10,
            a: 26,
            offset: 4,
        },
        Instruction::load_immediate(4, 4096),
        Instruction::RotateAndMask {
            a: 0,
            s: 10,
            shift: 1,
            begin: 15,
            end: 30,
        },
        Instruction::DivideWordUnsigned { d: 4, a: 4, b: 0 },
        Instruction::CompareWordImmediate {
            a: 15,
            immediate: 0,
        },
        Instruction::BranchConditionalForward {
            options: 4,
            condition_bit: 0,
            target: 32,
        },
    ]
}

#[test]
fn recognizes_three_serial_packets_before_the_divisor() {
    assert_eq!(
        serial_preloop_packet_lanes(&serial_fixture()),
        Some(DivisorLanes {
            width: 10,
            quotient: 4,
        })
    );
}

#[test]
fn rejects_a_cursor_store_through_the_wrong_packet_base() {
    let mut instructions = serial_fixture();
    let Instruction::StoreWord { a, .. } = &mut instructions[13] else {
        unreachable!()
    };
    *a = 9;
    assert_eq!(serial_preloop_packet_lanes(&instructions), None);
}

#[test]
fn preserves_nondefault_divisor_lanes() {
    let mut instructions = serial_fixture();
    let Instruction::LoadHalfwordZero { d, .. } = &mut instructions[23] else {
        unreachable!()
    };
    *d = 11;
    let Instruction::RotateAndMask { s, .. } = &mut instructions[25] else {
        unreachable!()
    };
    *s = 11;
    let Instruction::DivideWordUnsigned { d, .. } = &mut instructions[26] else {
        unreachable!()
    };
    *d = 30;

    assert_eq!(
        serial_preloop_packet_lanes(&instructions),
        Some(DivisorLanes {
            width: 11,
            quotient: 30,
        })
    );
}
