//! Eight-byte issue packets for the two measured legacy schedules.
//! Register lifetimes are packet-local except the buffer, index, and word homes.
use mwcc_machine_code::Instruction;

pub(super) fn addi(d: u8, a: u8, immediate: i16) -> Instruction {
    Instruction::AddImmediate { d, a, immediate }
}
fn complement(d: u8, a: u8) -> Instruction {
    Instruction::SubtractFromImmediate { d, a, immediate: 3 }
}
fn scale(a: u8, s: u8) -> Instruction {
    Instruction::ShiftLeftImmediate { a, s, shift: 3 }
}
fn byte(d: u8, a: u8, offset: i16) -> Instruction {
    Instruction::LoadByteZero { d, a, offset }
}
fn left(a: u8, s: u8, b: u8) -> Instruction {
    Instruction::ShiftLeftWord { a, s, b }
}
fn right(a: u8, s: u8, b: u8) -> Instruction {
    Instruction::ShiftRightWord { a, s, b }
}
fn merge(a: u8, b: u8) -> Instruction {
    Instruction::Or { a, s: a, b }
}
fn negative(d: u8, a: u8) -> Instruction {
    Instruction::Negate { d, a }
}
fn store(s: u8, offset: i16) -> Instruction {
    Instruction::StoreByte { s, a: 3, offset }
}

pub(super) fn pack(patched: bool) -> Vec<Instruction> {
    if patched {
        // r11 buffer, r31 index, r12 accumulated word. Counts are hoisted
        // around the byte loads; pointer/index updates fill arithmetic slots.
        vec![
            complement(7, 31),
            addi(0, 31, 1),
            complement(6, 0),
            addi(0, 31, 2),
            byte(8, 11, 0),
            scale(7, 7),
            negative(22, 31),
            byte(9, 11, 1),
            left(10, 8, 7),
            scale(8, 6),
            byte(7, 11, 2),
            complement(0, 0),
            addi(24, 31, 4),
            scale(6, 0),
            byte(0, 11, 3),
            complement(24, 24),
            addi(26, 31, 5),
            scale(22, 22),
            byte(23, 11, 4),
            complement(26, 26),
            addi(28, 31, 6),
            addi(30, 31, 7),
            byte(25, 11, 5),
            complement(28, 28),
            byte(27, 11, 6),
            byte(29, 11, 7),
            complement(30, 30),
            merge(12, 10),
            addi(11, 11, 8),
            left(8, 9, 8),
            merge(12, 8),
            addi(31, 31, 8),
            left(6, 7, 6),
            scale(24, 24),
            merge(12, 6),
            left(0, 0, 22),
            merge(12, 0),
            left(0, 23, 24),
            scale(26, 26),
            merge(12, 0),
            left(0, 25, 26),
            scale(28, 28),
            merge(12, 0),
            left(0, 27, 28),
            scale(30, 30),
            merge(12, 0),
            left(0, 29, 30),
            merge(12, 0),
        ]
    } else {
        // r31 buffer, r29 index, r30 accumulated word. The dependency-first
        // packet loads all eight bytes before reducing their shifted values.
        vec![
            complement(6, 29),
            byte(12, 31, 0),
            addi(0, 29, 1),
            byte(11, 31, 1),
            scale(10, 6),
            byte(9, 31, 2),
            complement(8, 0),
            byte(7, 31, 3),
            addi(6, 29, 2),
            byte(0, 31, 4),
            left(12, 12, 10),
            byte(23, 31, 5),
            scale(10, 8),
            byte(25, 31, 6),
            complement(6, 6),
            byte(27, 31, 7),
            scale(8, 6),
            negative(6, 29),
            addi(22, 29, 4),
            scale(6, 6),
            complement(22, 22),
            addi(24, 29, 5),
            scale(22, 22),
            complement(24, 24),
            addi(26, 29, 6),
            scale(24, 24),
            complement(26, 26),
            addi(28, 29, 7),
            scale(26, 26),
            complement(28, 28),
            scale(28, 28),
            merge(30, 12),
            left(10, 11, 10),
            merge(30, 10),
            left(8, 9, 8),
            merge(30, 8),
            left(6, 7, 6),
            merge(30, 6),
            left(0, 0, 22),
            merge(30, 0),
            left(0, 23, 24),
            merge(30, 0),
            left(0, 25, 26),
            merge(30, 0),
            left(0, 27, 28),
            merge(30, 0),
            addi(31, 31, 8),
            addi(29, 29, 8),
        ]
    }
}

pub(super) fn unpack(patched: bool) -> Vec<Instruction> {
    if patched {
        vec![
            complement(6, 5),
            addi(7, 5, 1),
            scale(8, 6),
            addi(6, 5, 2),
            complement(12, 7),
            addi(9, 5, 4),
            right(31, 0, 8),
            addi(8, 5, 5),
            complement(11, 6),
            store(31, 0),
            scale(12, 12),
            addi(7, 5, 6),
            right(12, 0, 12),
            addi(6, 5, 7),
            negative(10, 5),
            store(12, 1),
            complement(9, 9),
            addi(5, 5, 8),
            scale(11, 11),
            complement(8, 8),
            right(11, 0, 11),
            complement(7, 7),
            store(11, 2),
            scale(10, 10),
            right(10, 0, 10),
            scale(9, 9),
            store(10, 3),
            right(9, 0, 9),
            scale(8, 8),
            store(9, 4),
            right(8, 0, 8),
            scale(7, 7),
            store(8, 5),
            right(7, 0, 7),
            complement(6, 6),
            store(7, 6),
            scale(6, 6),
            right(6, 0, 6),
            store(6, 7),
            addi(3, 3, 8),
        ]
    } else {
        vec![
            complement(6, 5),
            scale(7, 6),
            addi(6, 5, 1),
            right(8, 0, 7),
            complement(6, 6),
            store(8, 0),
            scale(7, 6),
            addi(6, 5, 2),
            right(12, 0, 7),
            complement(6, 6),
            store(12, 1),
            scale(6, 6),
            right(11, 0, 6),
            negative(6, 5),
            store(11, 2),
            scale(7, 6),
            addi(6, 5, 4),
            right(10, 0, 7),
            complement(6, 6),
            store(10, 3),
            scale(7, 6),
            addi(6, 5, 5),
            right(9, 0, 7),
            complement(6, 6),
            store(9, 4),
            scale(7, 6),
            right(8, 0, 7),
            addi(6, 5, 6),
            store(8, 5),
            complement(7, 6),
            addi(6, 5, 7),
            scale(7, 7),
            right(7, 0, 7),
            complement(6, 6),
            store(7, 6),
            scale(6, 6),
            right(6, 0, 6),
            store(6, 7),
            addi(3, 3, 8),
            addi(5, 5, 8),
        ]
    }
}
