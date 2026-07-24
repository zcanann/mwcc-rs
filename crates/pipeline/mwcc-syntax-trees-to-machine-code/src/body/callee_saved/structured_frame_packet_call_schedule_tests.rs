use super::*;

fn serial_fixture() -> Vec<Instruction> {
    let mut scheduled = vec![Instruction::BranchToLinkRegister; 45];
    assign_mwcc_packet_call_lanes(&mut scheduled);
    let mut serial = scheduled.clone();
    for (destination, &original) in SCHEDULE.iter().enumerate() {
        serial[original] = scheduled[destination].clone();
    }

    serial[8] = Instruction::LoadWord {
        d: 3,
        a: 1,
        offset: 20,
    };
    serial[9] = Instruction::AddImmediate {
        d: 0,
        a: 3,
        immediate: 8,
    };
    serial[10] = Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 20,
    };
    serial[11] = Instruction::load_immediate_shifted(4, -768);
    serial[12] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: -1,
    };
    serial[13] = Instruction::StoreWord {
        s: 0,
        a: 3,
        offset: 0,
    };
    serial[14] = Instruction::load_immediate_shifted(4, -3);
    serial[15] = Instruction::AddImmediate {
        d: 0,
        a: 4,
        immediate: -898,
    };
    serial[16] = Instruction::StoreWord {
        s: 0,
        a: 3,
        offset: 4,
    };
    serial[25] = Instruction::load_immediate(11, 0);
    serial[26] = Instruction::StoreWord {
        s: 11,
        a: 1,
        offset: 8,
    };
    serial[27] = Instruction::XorImmediateShifted {
        a: 15,
        s: 15,
        immediate: 32768,
    };
    serial[28] = Instruction::load_immediate_shifted(0, 17200);
    serial[29] = Instruction::load_immediate_shifted(11, 0);
    serial[30] = Instruction::LoadFloatDouble {
        d: 2,
        a: 11,
        offset: 0,
    };
    serial[31] = Instruction::StoreWord {
        s: 15,
        a: 1,
        offset: 28,
    };
    serial[32] = Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 24,
    };
    serial[35] = Instruction::XorImmediateShifted {
        a: 14,
        s: 14,
        immediate: 32768,
    };
    serial[36] = Instruction::StoreWord {
        s: 14,
        a: 1,
        offset: 36,
    };
    serial[37] = Instruction::StoreWord {
        s: 0,
        a: 1,
        offset: 32,
    };
    serial[40] = Instruction::load_immediate_shifted(11, 0);
    serial[41] = Instruction::LoadFloatSingle {
        d: 3,
        a: 11,
        offset: 0,
    };
    serial[43] = Instruction::load_immediate(12, 11);
    serial[44] = Instruction::StoreWord {
        s: 12,
        a: 1,
        offset: 12,
    };
    serial.push(Instruction::BranchAndLink {
        target: "mixed_packet_call".into(),
    });
    serial
}

#[test]
fn recognizes_the_complete_serial_packet_call() {
    assert!(is_serial_packet_call(&serial_fixture()));
}

#[test]
fn rejects_a_packet_word_without_its_producer_dependency() {
    let mut instructions = serial_fixture();
    let Instruction::AddImmediate { a, .. } = &mut instructions[12] else {
        unreachable!()
    };
    *a = 5;
    assert!(!is_serial_packet_call(&instructions));
}
