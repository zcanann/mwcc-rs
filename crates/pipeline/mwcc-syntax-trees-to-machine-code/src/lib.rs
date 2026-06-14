//! Pipeline: syntax trees -> machine code.
//!
//! For the v0 subset this fuses lowering, instruction selection, and register
//! assignment into one pass that reproduces mwcceppc's output for leaf functions
//! returning a single expression. As the language grows these become distinct
//! phases — a typed-tree lowering, an instruction selector, and crucially a
//! standalone **register allocator** and **scheduler** (roadmap M1/M2), which is
//! where exact byte-matching is actually decided.
//!
//! ## Expression evaluation
//!
//! mwcc evaluates an expression tree keeping one value in a destination register
//! and spilling the other operand of a binary node into the scratch register
//! `r0` (`f0` for floats). This pass reproduces that for the tree shapes where a
//! single scratch register suffices:
//!
//! - a leaf, or `leaf op leaf`;
//! - `complex op leaf` / `leaf op complex` — the complex side threads through the
//!   scratch register;
//! - `complex op complex` at a non-scratch destination — the left side evaluates
//!   into the destination, the right into the scratch.
//!
//! Trees that need a second scratch register, that re-associate commutative
//! chains, or that reuse a still-live operand register, need the full allocator
//! and are rejected honestly (a diagnostic) rather than mis-compiled.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use mwcc_target::Eabi;
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

/// The scratch register mwcc spills the secondary operand of a binary node into.
const GENERAL_SCRATCH: u8 = 0; // r0
const FLOAT_SCRATCH: u8 = 0; // f0

/// Lower a parsed function to machine code for the given compiler build.
pub fn lower_function(function: &Function, _build: CompilerBuild) -> Compilation<MachineFunction> {
    let mut generator = Generator {
        output: MachineFunction::new(function.name.clone()),
        locations: HashMap::new(),
    };
    generator.assign_parameters(function)?;
    match function.return_type {
        Type::Int => generator.evaluate_general(&function.return_expression, Eabi::general_result().number)?,
        Type::Float => generator.evaluate_float(&function.return_expression, Eabi::float_result().number)?,
        Type::Void => {}
    }
    generator.output.instructions.push(Instruction::BranchToLinkRegister);
    Ok(generator.output)
}

#[derive(Clone, Copy, PartialEq)]
enum ValueClass {
    General,
    Float,
}

struct Location {
    class: ValueClass,
    register: u8,
}

struct Generator {
    output: MachineFunction,
    locations: HashMap<String, Location>,
}

fn is_complex(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { .. })
}

/// If `expression` is a multiplication, return its two operands.
fn as_multiplication(expression: &Expression) -> Option<(&Expression, &Expression)> {
    match expression {
        Expression::Binary { operator: BinaryOperator::Multiply, left, right } => Some((left, right)),
        _ => None,
    }
}

/// Whether `evaluate_*` can compute `expression` into `destination` using only
/// that register and the scratch register. `destination_is_scratch` means the
/// destination *is* the scratch register, so a node that itself needs the
/// scratch for a second operand cannot fit.
fn fits_single_scratch(expression: &Expression, destination_is_scratch: bool) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => {
            match (is_complex(left), is_complex(right)) {
                (false, false) => true,
                (true, false) => fits_single_scratch(left, true),
                (false, true) => fits_single_scratch(right, true),
                (true, true) => {
                    !destination_is_scratch
                        && fits_single_scratch(left, false)
                        && fits_single_scratch(right, true)
                }
            }
        }
        _ => true,
    }
}

impl Generator {
    fn assign_parameters(&mut self, function: &Function) -> Compilation<()> {
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::Int => {
                    self.locations.insert(parameter.name.clone(), Location { class: ValueClass::General, register: next_general });
                    next_general += 1;
                }
                Type::Float => {
                    self.locations.insert(parameter.name.clone(), Location { class: ValueClass::Float, register: next_float });
                    next_float += 1;
                }
                Type::Void => return Err(Diagnostic::error("a parameter cannot have type void")),
            }
        }
        Ok(())
    }

    /// Evaluate an integer expression into general register `destination`.
    fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(destination, *value);
                Ok(())
            }
            Expression::Variable(name) => {
                let source = self.general_register_of(name)?;
                if source != destination {
                    self.output.instructions.push(Instruction::move_register(destination, source));
                }
                Ok(())
            }
            Expression::Binary { operator, left, right } => {
                if !fits_single_scratch(expression, destination == GENERAL_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let (left_register, right_register) = self.place_general_operands(left, right, destination)?;
                self.output.instructions.push(general_combine(*operator, destination, left_register, right_register)?);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literal in integer context")),
        }
    }

    /// Compute the two operands of a binary node, returning the registers that
    /// hold the left and right values when the combining instruction runs.
    fn place_general_operands(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<(u8, u8)> {
        match (is_complex(left), is_complex(right)) {
            (false, false) => Ok((self.general_register_of_leaf(left)?, self.general_register_of_leaf(right)?)),
            (true, false) => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                Ok((GENERAL_SCRATCH, self.general_register_of_leaf(right)?))
            }
            (false, true) => {
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Ok((self.general_register_of_leaf(left)?, GENERAL_SCRATCH))
            }
            (true, true) => {
                self.evaluate_general(left, destination)?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Ok((destination, GENERAL_SCRATCH))
            }
        }
    }

    /// Evaluate a float expression into float register `destination`.
    fn evaluate_float(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::Variable(name) => {
                let source = self.float_register_of(name)?;
                if source != destination {
                    self.output.instructions.push(Instruction::FloatMove { d: destination, b: source });
                }
                Ok(())
            }
            Expression::Binary { operator, left, right } => {
                // With -fp_contract on, mwcc fuses multiply-add into fmadds/fmsubs.
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract)
                    && self.try_emit_float_fused(*operator, left, right, destination)?
                {
                    return Ok(());
                }
                if !fits_single_scratch(expression, destination == FLOAT_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let (left_register, right_register) = self.place_float_operands(left, right, destination)?;
                self.output.instructions.push(float_combine(*operator, destination, left_register, right_register)?);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literals need the constant pool (roadmap M3)")),
            Expression::IntegerLiteral(_) => Err(Diagnostic::error("integer literal in float context")),
        }
    }

    /// Try to fuse `left op right` into a multiply-add when one side is a
    /// multiplication. Returns whether an instruction was emitted. mwcc contracts
    /// these under -fp_contract on, so when a multiply sits under an add/subtract
    /// we either fuse it or stop honestly — never fall back to a separate
    /// multiply, which would not match.
    fn try_emit_float_fused(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        // x*y +/- addend
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
        // addend +/- x*y
        if let Some((x, y)) = as_multiplication(right) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(left)?;
            self.output.instructions.push(match operator {
                BinaryOperator::Add => Instruction::FloatMultiplyAddSingle { d: destination, a: multiplicand, c: multiplier, b: addend },
                // addend - x*y
                BinaryOperator::Subtract => Instruction::FloatNegativeMultiplySubtractSingle { d: destination, a: multiplicand, c: multiplier, b: addend },
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        Ok(false)
    }

    /// Place the addend of a fused multiply-add: a leaf stays in its register; a
    /// sub-expression is computed into the float scratch register first.
    fn place_float_addend(&mut self, expression: &Expression) -> Compilation<u8> {
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

    fn place_float_operands(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<(u8, u8)> {
        match (is_complex(left), is_complex(right)) {
            (false, false) => Ok((self.float_register_of_leaf(left)?, self.float_register_of_leaf(right)?)),
            (true, false) => {
                self.evaluate_float(left, FLOAT_SCRATCH)?;
                Ok((FLOAT_SCRATCH, self.float_register_of_leaf(right)?))
            }
            (false, true) => {
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                Ok((self.float_register_of_leaf(left)?, FLOAT_SCRATCH))
            }
            (true, true) => {
                self.evaluate_float(left, destination)?;
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                Ok((destination, FLOAT_SCRATCH))
            }
        }
    }

    fn general_register_of(&self, name: &str) -> Compilation<u8> {
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        if location.class != ValueClass::General {
            return Err(Diagnostic::error(format!("'{name}' is not an integer")));
        }
        Ok(location.register)
    }

    fn float_register_of(&self, name: &str) -> Compilation<u8> {
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        if location.class != ValueClass::Float {
            return Err(Diagnostic::error(format!("'{name}' is not a float")));
        }
        Ok(location.register)
    }

    fn general_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.general_register_of(name),
            _ => Err(Diagnostic::error("v0: a leaf operand must be a parameter (constants in trees: roadmap M3)")),
        }
    }

    fn float_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.float_register_of(name),
            _ => Err(Diagnostic::error("v0: a float leaf operand must be a parameter")),
        }
    }

    /// Load a 32-bit integer constant the way mwcc does: `li`, or `lis` + `addi`
    /// with a high-adjusted upper half to absorb `addi`'s sign extension.
    fn load_integer_constant(&mut self, destination: u8, value: i64) {
        let value = value as i32;
        if (-0x8000..=0x7fff).contains(&value) {
            self.output.instructions.push(Instruction::load_immediate(destination, value as i16));
        } else {
            let low = (value as u32 & 0xffff) as i16;
            let high_adjusted = ((value - low as i32) >> 16) as i16;
            self.output.instructions.push(Instruction::load_immediate_shifted(destination, high_adjusted));
            self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: low });
        }
    }
}

/// Build the instruction that combines two general operands into `destination`.
///
/// For the commutative operators mwcc places the scratch register (`r0`, which
/// holds a just-computed sub-expression) *second*; subtraction is ordered so
/// `subf` computes `left - right`.
fn general_combine(operator: BinaryOperator, destination: u8, left: u8, right: u8) -> Compilation<Instruction> {
    // Put the scratch register last for commutative operators.
    let (first, second) = if left == GENERAL_SCRATCH { (right, left) } else { (left, right) };
    Ok(match operator {
        BinaryOperator::Add => Instruction::Add { d: destination, a: first, b: second },
        BinaryOperator::Subtract => Instruction::SubtractFrom { d: destination, a: right, b: left },
        BinaryOperator::Multiply => Instruction::MultiplyLow { d: destination, a: first, b: second },
        BinaryOperator::Divide => return Err(Diagnostic::error("integer division not yet supported")),
    })
}

fn float_combine(operator: BinaryOperator, destination: u8, left: u8, right: u8) -> Compilation<Instruction> {
    // As with integers, the scratch register (holding a just-computed value) goes
    // second for the commutative operators.
    let (first, second) = if left == FLOAT_SCRATCH { (right, left) } else { (left, right) };
    Ok(match operator {
        BinaryOperator::Add => Instruction::FloatAddSingle { d: destination, a: first, b: second },
        BinaryOperator::Subtract => Instruction::FloatSubtractSingle { d: destination, a: left, b: right },
        BinaryOperator::Multiply => Instruction::FloatMultiplySingle { d: destination, a: first, c: second },
        BinaryOperator::Divide => return Err(Diagnostic::error("float division not yet supported")),
    })
}
