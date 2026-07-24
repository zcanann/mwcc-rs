//! Folding of packet high-half constants after physical allocation.
//!
//! Invariant extraction can give a high-only constant its own register even
//! when a later packet field is proven to occupy only the low half. MWCC folds
//! that pair to `oris`; this pass does the same only when the constant has one
//! use in its live range.

#[allow(unused_imports)]
use super::*;
use mwcc_vreg::{register_operands, Class, RegisterRole};

pub(super) fn fold_masked_high_constant(instructions: &mut [Instruction]) -> Option<usize> {
    for constant_index in 0..instructions.len() {
        let Instruction::AddImmediateShifted {
            d: constant_register,
            a: 0,
            immediate,
        } = instructions[constant_index]
        else {
            continue;
        };

        let Some(or_index) = (constant_index + 1..instructions.len())
            .find(|&index| touches_general(&instructions[index], constant_register))
        else {
            continue;
        };
        let Instruction::Or { a, s, b } = instructions[or_index] else {
            continue;
        };
        let low = if s == constant_register {
            b
        } else if b == constant_register {
            s
        } else {
            continue;
        };
        if uses_general_after_first_access(instructions, or_index + 1, constant_register) {
            continue;
        }

        let Some(low_definition) = (constant_index + 1..or_index)
            .rev()
            .find(|&index| defines_general(&instructions[index], low))
        else {
            continue;
        };
        if !matches!(
            instructions[low_definition],
            Instruction::ClearLeftImmediate { a: destination, clear, .. }
                if destination == low && clear >= 16
        ) && !matches!(
            instructions[low_definition],
            Instruction::AndContiguousMask {
                a: destination,
                begin,
                end: 31,
                ..
            } if destination == low && begin >= 16
        ) {
            continue;
        }

        instructions[or_index] = Instruction::OrImmediateShifted {
            a,
            s: low,
            immediate: immediate as u16,
        };
        return Some(constant_index);
    }
    None
}

fn touches_general(instruction: &Instruction, register: u8) -> bool {
    register_operands(instruction)
        .iter()
        .any(|operand| operand.class == Class::General && operand.register == register)
}

fn defines_general(instruction: &Instruction, register: u8) -> bool {
    register_operands(instruction).iter().any(|operand| {
        operand.class == Class::General
            && operand.register == register
            && operand.role == RegisterRole::Define
    })
}

fn uses_general_after_first_access(
    instructions: &[Instruction],
    start: usize,
    register: u8,
) -> bool {
    for instruction in &instructions[start..] {
        let operands = register_operands(instruction);
        if operands.iter().any(|operand| {
            operand.class == Class::General
                && operand.register == register
                && operand.role == RegisterRole::Use
        }) {
            return true;
        }
        if operands.iter().any(|operand| {
            operand.class == Class::General
                && operand.register == register
                && operand.role == RegisterRole::Define
        }) {
            break;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_a_single_use_high_constant_into_a_masked_field() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(18, -632),
            Instruction::LoadHalfwordZero {
                d: 0,
                a: 26,
                offset: 4,
            },
            Instruction::ShiftLeftImmediate {
                a: 17,
                s: 0,
                shift: 1,
            },
            Instruction::AddImmediate {
                d: 0,
                a: 17,
                immediate: -1,
            },
            Instruction::AndContiguousMask {
                a: 0,
                s: 0,
                begin: 20,
                end: 31,
            },
            Instruction::Or { a: 0, s: 18, b: 0 },
        ];

        assert_eq!(fold_masked_high_constant(&mut instructions), Some(0));
        assert!(matches!(
            instructions[5],
            Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: 0xfd88,
            }
        ));
    }

    #[test]
    fn keeps_a_high_constant_with_another_use() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(18, -632),
            Instruction::ClearLeftImmediate {
                a: 0,
                s: 0,
                clear: 20,
            },
            Instruction::Or { a: 0, s: 18, b: 0 },
            Instruction::StoreWord {
                s: 18,
                a: 3,
                offset: 0,
            },
        ];

        assert_eq!(fold_masked_high_constant(&mut instructions), None);
    }

    #[test]
    fn requires_a_low_half_proof() {
        let mut instructions = vec![
            Instruction::load_immediate_shifted(18, -632),
            Instruction::Or { a: 0, s: 18, b: 0 },
        ];

        assert_eq!(fold_masked_high_constant(&mut instructions), None);
    }
}
