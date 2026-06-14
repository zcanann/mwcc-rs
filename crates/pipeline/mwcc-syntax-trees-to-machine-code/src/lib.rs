//! Pipeline: syntax trees -> machine code.
//!
//! For the v0 subset this fuses lowering, instruction selection, and register
//! assignment into one pass that reproduces mwcceppc's output for leaf functions
//! returning a single expression. As the language grows these become distinct
//! phases — a typed-tree lowering, an instruction selector, and crucially a
//! standalone **register allocator** and **scheduler** (roadmap M1/M2), which is
//! where exact byte-matching is actually decided.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, MachineFunction};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, Type};
use mwcc_target::Eabi;
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

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

    /// Evaluate an integer expression into general register `target`.
    fn evaluate_general(&mut self, expression: &Expression, target: u8) -> Compilation<()> {
        match expression {
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(target, *value);
                Ok(())
            }
            Expression::Variable(name) => {
                let source = self.general_register_of(name)?;
                if source != target {
                    self.output.instructions.push(Instruction::move_register(target, source));
                }
                Ok(())
            }
            Expression::Binary { operator, left, right } => {
                self.evaluate_general(left, target)?;
                let right_register = self.general_register_of_leaf(right)?;
                let instruction = match operator {
                    BinaryOperator::Add => Instruction::Add { d: target, a: target, b: right_register },
                    // `sub rD,rA,rB` = rA-rB = subf rD,rB,rA
                    BinaryOperator::Subtract => Instruction::SubtractFrom { d: target, a: right_register, b: target },
                    BinaryOperator::Multiply => Instruction::MultiplyLow { d: target, a: target, b: right_register },
                    BinaryOperator::Divide => return Err(Diagnostic::error("integer division not yet supported")),
                };
                self.output.instructions.push(instruction);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literal in integer context")),
        }
    }

    /// Evaluate a float expression into float register `target`.
    fn evaluate_float(&mut self, expression: &Expression, target: u8) -> Compilation<()> {
        match expression {
            Expression::Variable(name) => {
                let source = self.float_register_of(name)?;
                if source != target {
                    self.output.instructions.push(Instruction::FloatMove { d: target, b: source });
                }
                Ok(())
            }
            Expression::Binary { operator, left, right } => {
                self.evaluate_float(left, target)?;
                let right_register = self.float_register_of_leaf(right)?;
                let instruction = match operator {
                    BinaryOperator::Add => Instruction::FloatAddSingle { d: target, a: target, b: right_register },
                    BinaryOperator::Subtract => Instruction::FloatSubtractSingle { d: target, a: target, b: right_register },
                    BinaryOperator::Multiply => Instruction::FloatMultiplySingle { d: target, a: target, c: right_register },
                    BinaryOperator::Divide => return Err(Diagnostic::error("float division not yet supported")),
                };
                self.output.instructions.push(instruction);
                Ok(())
            }
            Expression::FloatLiteral(_) => Err(Diagnostic::error("float literals need the constant pool (roadmap M3)")),
            Expression::IntegerLiteral(_) => Err(Diagnostic::error("integer literal in float context")),
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
            _ => Err(Diagnostic::error("v0: the right operand of a binary expression must be a parameter")),
        }
    }

    fn float_register_of_leaf(&self, expression: &Expression) -> Compilation<u8> {
        match expression {
            Expression::Variable(name) => self.float_register_of(name),
            _ => Err(Diagnostic::error("v0: the right operand of a float binary expression must be a parameter")),
        }
    }

    /// Load a 32-bit integer constant the way mwcc does: `li`, or `lis` + `addi`
    /// with a high-adjusted upper half to absorb `addi`'s sign extension.
    fn load_integer_constant(&mut self, target: u8, value: i64) {
        let value = value as i32;
        if (-0x8000..=0x7fff).contains(&value) {
            self.output.instructions.push(Instruction::load_immediate(target, value as i16));
        } else {
            let low = (value as u32 & 0xffff) as i16;
            let high_adjusted = ((value - low as i32) >> 16) as i16;
            self.output.instructions.push(Instruction::load_immediate_shifted(target, high_adjusted));
            self.output.instructions.push(Instruction::AddImmediate { d: target, a: target, immediate: low });
        }
    }
}
