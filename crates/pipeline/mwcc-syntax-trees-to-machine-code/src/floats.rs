//! Floating-point expression evaluation and multiply-add contraction.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

impl Generator {

    /// Evaluate a float expression into float register `destination`.
    pub(crate) fn evaluate_float(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::Variable(name) => {
                if self.locations.contains_key(name) {
                    let source = self.float_register_of(name)?;
                    if source != destination {
                        self.output.instructions.push(Instruction::FloatMove { d: destination, b: source });
                    }
                    Ok(())
                } else {
                    self.emit_global_load(name, destination)
                }
            }
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
            Expression::Member { base, offset, member_type } => self.emit_member_load(base, *offset, *member_type, destination),
            Expression::Index { base, index } => self.emit_subscript(base, index, destination),
            Expression::Call { name, arguments } => self.emit_call(name, arguments, Some(destination), true),
            Expression::Binary { operator, left, right } => {
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.try_emit_float_fused(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                if !fits_single_scratch(expression, destination == FLOAT_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_float_operands(*operator, left, right, destination)?;
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
            Expression::FloatLiteral(value) => {
                self.load_float_constant(destination, *value as f32);
                Ok(())
            }
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

    /// Whether `operand` is a float value loaded from memory: a float struct
    /// member, a dereference of a float pointer, or a file-scope float global. Such
    /// an operand loads into a float register (its general base register, if any,
    /// is untouched).
    fn is_float_located(&self, operand: &Expression) -> bool {
        if let Some((_, _, member_type)) = as_member(operand) {
            return member_type == Type::Float;
        }
        if let Some(pointer) = as_dereference(operand) {
            return matches!(self.pointee_of(pointer), Ok(Pointee::Float));
        }
        if let Expression::Variable(name) = operand {
            if !self.locations.contains_key(name) {
                return self.globals.get(name) == Some(&Type::Float);
            }
        }
        false
    }

    /// Place float operands when at least one is loaded from memory (a float member
    /// or `*float_pointer`). A single located operand loads into the scratch (its
    /// leaf partner stays home), two load left into the destination and right into
    /// the scratch, and a located-with-constant loads the constant first.
    fn place_float_located_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
        if self.is_float_located(left) && self.is_float_located(right) {
            if destination == FLOAT_SCRATCH {
                return Err(Diagnostic::error("two float loads need a non-scratch destination (roadmap)"));
            }
            self.emit_located_operand(left, destination)?;
            self.emit_located_operand(right, FLOAT_SCRATCH)?;
            return Operands::ordered(destination, FLOAT_SCRATCH);
        }
        if self.is_float_located(left) {
            if let Expression::FloatLiteral(value) = right {
                if destination == FLOAT_SCRATCH {
                    return Err(Diagnostic::error("float load with constant needs a non-scratch destination (roadmap)"));
                }
                // mwcc loads the constant first, then the memory operand.
                self.load_float_constant(destination, *value as f32);
                self.emit_located_operand(left, FLOAT_SCRATCH)?;
                // Commutative ops lead with the constant; subtraction is load - constant.
                return if operator == BinaryOperator::Subtract {
                    Operands::ordered(FLOAT_SCRATCH, destination)
                } else {
                    Operands::ordered(destination, FLOAT_SCRATCH)
                };
            }
            let right_register = self.float_register_of_leaf(right)?;
            self.emit_located_operand(left, FLOAT_SCRATCH)?;
            return Operands::ordered(FLOAT_SCRATCH, right_register);
        }
        if self.is_float_located(right) {
            if let Expression::FloatLiteral(value) = left {
                if destination == FLOAT_SCRATCH {
                    return Err(Diagnostic::error("float load with constant needs a non-scratch destination (roadmap)"));
                }
                self.load_float_constant(destination, *value as f32);
                self.emit_located_operand(right, FLOAT_SCRATCH)?;
                return Operands::ordered(destination, FLOAT_SCRATCH);
            }
            let left_register = self.float_register_of_leaf(left)?;
            self.emit_located_operand(right, FLOAT_SCRATCH)?;
            return Operands::ordered(left_register, FLOAT_SCRATCH);
        }
        unreachable!("caller checked one side is a float load")
    }

    pub(crate) fn place_float_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
        // A float operand loaded from memory (a member or `*float_pointer`) loads
        // into a float register; the general base register is untouched, so it can
        // even land straight in the float destination.
        if self.is_float_located(left) || self.is_float_located(right) {
            return self.place_float_located_operands(operator, left, right, destination);
        }
        // A float constant operand is loaded from `.sdata2` into the scratch
        // register; the other (leaf-variable) operand stays in place. mwcc emits
        // the constant as the first source of the (commutative) operation.
        if let Expression::FloatLiteral(value) = right {
            if matches!(left, Expression::Variable(_)) {
                let left_register = self.float_register_of_leaf(left)?;
                self.load_float_constant(FLOAT_SCRATCH, *value as f32);
                return Operands::reversed(left_register, FLOAT_SCRATCH);
            }
        }
        if let Expression::FloatLiteral(value) = left {
            if matches!(right, Expression::Variable(_)) {
                let right_register = self.float_register_of_leaf(right)?;
                self.load_float_constant(FLOAT_SCRATCH, *value as f32);
                return Operands::ordered(FLOAT_SCRATCH, right_register);
            }
        }
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
