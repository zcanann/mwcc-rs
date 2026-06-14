//! Pipeline: syntax trees -> machine code.
//!
//! For the v0 subset this fuses lowering, instruction selection, and register
//! assignment into one pass that reproduces mwcceppc's output for small leaf
//! functions. As the language grows these become distinct phases — a typed-tree
//! lowering, an instruction selector, and crucially a standalone **register
//! allocator** and **scheduler** (roadmap M1/M2), which is where exact
//! byte-matching is actually decided.
//!
//! ## Expression evaluation
//!
//! mwcc evaluates an expression tree keeping one value in a destination register
//! and spilling the other operand of a binary node into the scratch register
//! `r0` (`f0` for floats). This pass reproduces that for tree shapes where a
//! single scratch register suffices, and rejects (honestly) the shapes that need
//! the full allocator: a second scratch, commutative-chain re-association, or
//! reuse of a still-live operand register.
//!
//! Operand order in the emitted instruction is shape-driven: for the commutative
//! operators the operands keep source order, *except* when the left operand was a
//! sub-expression computed into the scratch register and the right is a leaf — then
//! mwcc puts the leaf first.
//!
//! ## Locals
//!
//! A single local whose value is the function result is computed straight into
//! the result register. Otherwise a single local is computed into the scratch
//! register and used as a leaf; shapes where that would collide with the scratch
//! the result expression also needs are rejected for now.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LocalDeclaration, Type};
use mwcc_target::Eabi;
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

/// The scratch register mwcc spills the secondary operand of a binary node into.
const GENERAL_SCRATCH: u8 = 0; // r0
const FLOAT_SCRATCH: u8 = 0; // f0

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

/// Whether evaluating `expression` uses the scratch register at all — true when
/// any binary node has a binary child.
fn needs_scratch(expression: &Expression) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => {
            is_complex(left) || is_complex(right) || needs_scratch(left) || needs_scratch(right)
        }
        _ => false,
    }
}

/// Whether `evaluate_*` can compute `expression` into `destination` using only
/// that register and the scratch register.
fn fits_single_scratch(expression: &Expression, destination_is_scratch: bool) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => match (is_complex(left), is_complex(right)) {
            (false, false) => true,
            (true, false) => fits_single_scratch(left, true),
            (false, true) => fits_single_scratch(right, true),
            (true, true) => {
                !destination_is_scratch && fits_single_scratch(left, false) && fits_single_scratch(right, true)
            }
        },
        _ => true,
    }
}

/// Lower a parsed function to machine code for the given compiler build.
pub fn lower_function(function: &Function, _build: CompilerBuild) -> Compilation<MachineFunction> {
    let mut generator = Generator { output: MachineFunction::new(function.name.clone()), locations: HashMap::new() };
    generator.assign_parameters(function)?;
    generator.evaluate_body(function)?;
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

fn class_of(declared: Type) -> Compilation<ValueClass> {
    match declared {
        Type::Int => Ok(ValueClass::General),
        Type::Float => Ok(ValueClass::Float),
        Type::Void => Err(Diagnostic::error("a value cannot have type void")),
    }
}

impl Generator {
    fn assign_parameters(&mut self, function: &Function) -> Compilation<()> {
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            self.locations.insert(parameter.name.clone(), Location { class, register });
        }
        Ok(())
    }

    /// Evaluate the locals (if any) and the return expression into the result register.
    fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        let result = match function.return_type {
            Type::Int => Eabi::general_result().number,
            Type::Float => Eabi::float_result().number,
            Type::Void => return Ok(()),
        };

        match function.locals.as_slice() {
            [] => self.evaluate(&function.return_expression, function.return_type, result),
            [local] => self.evaluate_single_local(local, &function.return_expression, function.return_type, result),
            _ => Err(Diagnostic::error("multiple locals need the full register allocator (roadmap M1)")),
        }
    }

    fn evaluate_single_local(
        &mut self,
        local: &LocalDeclaration,
        return_expression: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let class = class_of(local.declared_type)?;

        // `return x;` — the local is the result, so compute its initializer
        // straight into the result register.
        if matches!(return_expression, Expression::Variable(name) if *name == local.name) {
            return self.evaluate(&local.initializer, local.declared_type, result);
        }

        // Otherwise the local lives in the scratch register and is used as a leaf.
        // That only works if the result expression does not itself need the scratch.
        if needs_scratch(return_expression) {
            return Err(Diagnostic::error("local reused inside a scratch-needing expression (roadmap M1)"));
        }
        let scratch = match class {
            ValueClass::General => GENERAL_SCRATCH,
            ValueClass::Float => FLOAT_SCRATCH,
        };
        self.evaluate(&local.initializer, local.declared_type, scratch)?;
        self.locations.insert(local.name.clone(), Location { class, register: scratch });
        self.evaluate(return_expression, return_type, result)
    }

    fn evaluate(&mut self, expression: &Expression, value_type: Type, destination: u8) -> Compilation<()> {
        match value_type {
            Type::Int => self.evaluate_general(expression, destination),
            Type::Float => self.evaluate_float(expression, destination),
            Type::Void => Err(Diagnostic::error("cannot evaluate a void expression")),
        }
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
                let operands = self.place_general_operands(left, right, destination)?;
                self.output.instructions.push(general_combine(*operator, destination, operands)?);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literal in integer context")),
        }
    }

    fn place_general_operands(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
        match (is_complex(left), is_complex(right)) {
            (false, false) => Operands::ordered(self.general_register_of_leaf(left)?, self.general_register_of_leaf(right)?),
            (true, false) => {
                self.evaluate_general(left, GENERAL_SCRATCH)?;
                // left computed into scratch, right is a leaf: mwcc puts the leaf first.
                Operands::reversed(GENERAL_SCRATCH, self.general_register_of_leaf(right)?)
            }
            (false, true) => {
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Operands::ordered(self.general_register_of_leaf(left)?, GENERAL_SCRATCH)
            }
            (true, true) => {
                self.evaluate_general(left, destination)?;
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                Operands::ordered(destination, GENERAL_SCRATCH)
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
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literals need the constant pool (roadmap M3)")),
            Expression::IntegerLiteral(_) => Err(Diagnostic::error("integer literal in float context")),
        }
    }

    /// Try to fuse `left op right` into a multiply-add when one side is a
    /// multiplication. mwcc contracts these under -fp_contract on, so we either
    /// fuse or stop honestly — never fall back to a separate multiply.
    fn try_emit_float_fused(
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

    fn place_float_operands(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<Operands> {
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
                self.evaluate_float(left, destination)?;
                self.evaluate_float(right, FLOAT_SCRATCH)?;
                Operands::ordered(destination, FLOAT_SCRATCH)
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
            _ => Err(Diagnostic::error("v0: a leaf operand must be a variable (constants in trees: roadmap M3)")),
        }
    }

    fn float_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.float_register_of(name),
            _ => Err(Diagnostic::error("v0: a float leaf operand must be a variable")),
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

/// The two operand registers of a binary node, with whether the commutative
/// emission order is reversed relative to source order.
struct Operands {
    left: u8,
    right: u8,
    reversed: bool,
}

impl Operands {
    fn ordered(left: u8, right: u8) -> Compilation<Operands> {
        Ok(Operands { left, right, reversed: false })
    }
    fn reversed(left: u8, right: u8) -> Compilation<Operands> {
        Ok(Operands { left, right, reversed: true })
    }
    /// The (first, second) operand registers for a commutative instruction.
    fn commutative(&self) -> (u8, u8) {
        if self.reversed {
            (self.right, self.left)
        } else {
            (self.left, self.right)
        }
    }
}

/// Build the instruction combining two general operands into `destination`.
/// Subtraction is ordered so `subf` computes `left - right`.
fn general_combine(operator: BinaryOperator, destination: u8, operands: Operands) -> Compilation<Instruction> {
    let (first, second) = operands.commutative();
    Ok(match operator {
        BinaryOperator::Add => Instruction::Add { d: destination, a: first, b: second },
        BinaryOperator::Subtract => Instruction::SubtractFrom { d: destination, a: operands.right, b: operands.left },
        BinaryOperator::Multiply => Instruction::MultiplyLow { d: destination, a: first, b: second },
        BinaryOperator::Divide => return Err(Diagnostic::error("integer division not yet supported")),
    })
}

fn float_combine(operator: BinaryOperator, destination: u8, operands: Operands) -> Compilation<Instruction> {
    let (first, second) = operands.commutative();
    Ok(match operator {
        BinaryOperator::Add => Instruction::FloatAddSingle { d: destination, a: first, b: second },
        BinaryOperator::Subtract => Instruction::FloatSubtractSingle { d: destination, a: operands.left, b: operands.right },
        BinaryOperator::Multiply => Instruction::FloatMultiplySingle { d: destination, a: first, c: second },
        BinaryOperator::Divide => return Err(Diagnostic::error("float division not yet supported")),
    })
}
