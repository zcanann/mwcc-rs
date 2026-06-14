//! Floating-point expression evaluation and multiply-add contraction.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

impl Generator {

    /// Evaluate a float expression into float register `destination`.
    pub(crate) fn evaluate_float(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::Variable(name) => {
                let source = self.float_register_of(name)?;
                if source != destination {
                    self.output.instructions.push(Instruction::FloatMove { d: destination, b: source });
                }
                Ok(())
            }
            Expression::Binary { operator, left, right } => {
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.try_emit_float_fused(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                if !fits_single_scratch(expression, destination == FLOAT_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_float_operands(left, right, destination)?;
                self.output.instructions.push(float_combine(*operator, destination, operands)?);
                Ok(())
            }
            Expression::Unary { operator: UnaryOperator::Negate, operand } => {
                // -(-x) == x
                if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = operand.as_ref() {
                    return self.evaluate_float(inner, destination);
                }
                // A leaf negates in place; a sub-expression goes through the scratch.
                let source = if is_complex(operand) {
                    if !fits_single_scratch(operand, true) {
                        return Err(Diagnostic::error("float negation operand needs the full register allocator (roadmap M1)"));
                    }
                    self.evaluate_float(operand, FLOAT_SCRATCH)?;
                    FLOAT_SCRATCH
                } else {
                    self.float_register_of_leaf(operand)?
                };
                self.output.instructions.push(Instruction::FloatNegate { d: destination, b: source });
                Ok(())
            }
            Expression::Unary { .. } => Err(Diagnostic::error("only float negation is supported as a float unary")),
            Expression::Conditional { condition, when_true, when_false } => {
                self.emit_float_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { operand, .. } => self.emit_cast_to_float(operand, destination),
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literals need the constant pool (roadmap M3)")),
            Expression::IntegerLiteral(_) => Err(Diagnostic::error("integer literal in float context")),
        }
    }

    /// Try to fuse `left op right` into a multiply-add when one side is a
    /// multiplication. mwcc contracts these under -fp_contract on, so we either
    /// fuse or stop honestly — never fall back to a separate multiply.
    pub(crate) fn try_emit_float_fused(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if let Some((x, y)) = as_multiplication(left) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(right)?;
            self.output.instructions.push(match operator {
                BinaryOperator::Add => Instruction::FloatMultiplyAddSingle { d: destination, a: multiplicand, c: multiplier, b: addend },
                BinaryOperator::Subtract => Instruction::FloatMultiplySubtractSingle { d: destination, a: multiplicand, c: multiplier, b: addend },
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        if let Some((x, y)) = as_multiplication(right) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(left)?;
            self.output.instructions.push(match operator {
                BinaryOperator::Add => Instruction::FloatMultiplyAddSingle { d: destination, a: multiplicand, c: multiplier, b: addend },
                BinaryOperator::Subtract => Instruction::FloatNegativeMultiplySubtractSingle { d: destination, a: multiplicand, c: multiplier, b: addend },
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn place_float_addend(&mut self, expression: &Expression) -> Compilation<u8> {
        if is_complex(expression) {
            if !fits_single_scratch(expression, true) {
                return Err(Diagnostic::error("fused multiply-add addend needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_float(expression, FLOAT_SCRATCH)?;
            Ok(FLOAT_SCRATCH)
        } else {
            self.float_register_of_leaf(expression)
        }
    }

    pub(crate) fn place_float_operands(&mut self, left: &Expression, right: &Expression, _destination: u8) -> Compilation<Operands> {
        match (is_complex(left), is_complex(right)) {
            (false, false) => Operands::ordered(self.float_register_of_leaf(left)?, self.float_register_of_leaf(right)?),
            (true, false) => {
                self.evaluate_float(left, FLOAT_SCRATCH)?;
                Operands::reversed(FLOAT_SCRATCH, self.float_register_of_leaf(right)?)
            }
            (false, true) => {
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                Operands::ordered(self.float_register_of_leaf(left)?, FLOAT_SCRATCH)
            }
            (true, true) => {
                let temp = self.with_reserved_inputs(right, |generator| {
                    let temp = generator.lowest_free_float()?;
                    generator.evaluate_float(left, temp)?;
                    Ok(temp)
                })?;
                let temp_added = self.reserved.insert(temp);
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                if temp_added {
                    self.reserved.remove(&temp);
                }
                Operands::ordered(temp, FLOAT_SCRATCH)
            }
        }
    }
}
