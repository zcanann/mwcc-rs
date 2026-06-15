//! The two-operand register pair and the instruction combiners.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::BinaryOperator;
use crate::analysis::is_comparison;

pub(crate) struct Operands {
    pub(crate) left: u8,
    pub(crate) right: u8,
    pub(crate) reversed: bool,
}

impl Operands {
    pub(crate) fn ordered(left: u8, right: u8) -> Compilation<Operands> {
        Ok(Operands { left, right, reversed: false })
    }
    pub(crate) fn reversed(left: u8, right: u8) -> Compilation<Operands> {
        Ok(Operands { left, right, reversed: true })
    }
    /// The (first, second) operand registers for a commutative instruction.
    pub(crate) fn commutative(&self) -> (u8, u8) {
        if self.reversed {
            (self.right, self.left)
        } else {
            (self.left, self.right)
        }
    }
}

/// Build the instruction combining two general operands into `destination`.
/// Subtraction is ordered so `subf` computes `left - right`.
pub(crate) fn general_combine(operator: BinaryOperator, destination: u8, operands: Operands) -> Compilation<Instruction> {
    let (first, second) = operands.commutative();
    Ok(match operator {
        BinaryOperator::Add => Instruction::Add { d: destination, a: first, b: second },
        BinaryOperator::Subtract => Instruction::SubtractFrom { d: destination, a: operands.right, b: operands.left },
        BinaryOperator::Multiply => Instruction::MultiplyLow { d: destination, a: first, b: second },
        BinaryOperator::BitAnd => Instruction::And { a: destination, s: first, b: second },
        BinaryOperator::BitOr => Instruction::Or { a: destination, s: first, b: second },
        BinaryOperator::BitXor => Instruction::Xor { a: destination, s: first, b: second },
        // shifts are not commutative: keep source order
        BinaryOperator::ShiftLeft => Instruction::ShiftLeftWord { a: destination, s: operands.left, b: operands.right },
        BinaryOperator::ShiftRight => return Err(Diagnostic::error("'>>' needs signed/unsigned types (roadmap)")),
        BinaryOperator::Divide => return Err(Diagnostic::error("integer division not yet supported")),
        // comparisons are handled before reaching the generic combiner
        operator if is_comparison(operator) => return Err(Diagnostic::error("comparison reached the generic combiner")),
        _ => return Err(Diagnostic::error("unsupported integer operator")),
    })
}

pub(crate) fn float_combine(operator: BinaryOperator, destination: u8, operands: Operands, double: bool) -> Compilation<Instruction> {
    let (first, second) = operands.commutative();
    Ok(match operator {
        BinaryOperator::Add if double => Instruction::FloatAddDouble { d: destination, a: first, b: second },
        BinaryOperator::Subtract if double => Instruction::FloatSubtractDouble { d: destination, a: operands.left, b: operands.right },
        BinaryOperator::Multiply if double => Instruction::FloatMultiplyDouble { d: destination, a: first, c: second },
        BinaryOperator::Add => Instruction::FloatAddSingle { d: destination, a: first, b: second },
        BinaryOperator::Subtract => Instruction::FloatSubtractSingle { d: destination, a: operands.left, b: operands.right },
        BinaryOperator::Multiply => Instruction::FloatMultiplySingle { d: destination, a: first, c: second },
        BinaryOperator::Divide => Instruction::FloatDivideSingle { d: destination, a: operands.left, b: operands.right },
        BinaryOperator::BitAnd
        | BinaryOperator::BitOr
        | BinaryOperator::BitXor
        | BinaryOperator::ShiftLeft
        | BinaryOperator::ShiftRight => return Err(Diagnostic::error("bitwise operators are not valid on floats")),
        _ => return Err(Diagnostic::error("float comparisons not yet supported")),
    })
}
