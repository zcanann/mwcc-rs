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
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, LocalDeclaration, Type, UnaryOperator};
use mwcc_target::Eabi;
use mwcc_versions::CompilerBuild;
use std::collections::HashMap;

/// The scratch register mwcc spills the secondary operand of a binary node into.
const GENERAL_SCRATCH: u8 = 0; // r0
const FLOAT_SCRATCH: u8 = 0; // f0

fn is_complex(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { .. } | Expression::Unary { .. })
}

fn is_zero_literal(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
}

/// A nonzero integer literal that fits a signed 16-bit immediate.
fn as_small_integer(expression: &Expression) -> Option<i16> {
    match expression {
        Expression::IntegerLiteral(value) if *value != 0 => i16::try_from(*value).ok(),
        _ => None,
    }
}

fn is_comparison(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
    )
}

/// If `expression` is a multiplication, return its two operands.
fn as_multiplication(expression: &Expression) -> Option<(&Expression, &Expression)> {
    match expression {
        Expression::Binary { operator: BinaryOperator::Multiply, left, right } => Some((left, right)),
        _ => None,
    }
}

fn is_commutative(operator: BinaryOperator) -> bool {
    matches!(
        operator,
        BinaryOperator::Add | BinaryOperator::Multiply | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor
    )
}

fn fits_signed_16(value: i64) -> bool {
    (-0x8000..=0x7fff).contains(&value)
}

fn fits_unsigned_16(value: i64) -> bool {
    (0..=0xffff).contains(&value)
}

/// If `value` is a mask of exactly the low `k` bits (`2^k - 1`, `1 <= k <= 31`),
/// return `k`.
fn low_bit_mask(value: i64) -> Option<u8> {
    if value >= 1 && (value as u64 & (value as u64 + 1)) == 0 {
        let bits = (value as u64 + 1).trailing_zeros();
        if (1..=31).contains(&bits) {
            return Some(bits as u8);
        }
    }
    None
}

/// Whether evaluating `expression` uses the scratch register at all — true when
/// any binary node has a binary child.
fn needs_scratch(expression: &Expression) -> bool {
    match expression {
        Expression::Binary { left, right, .. } => {
            is_complex(left) || is_complex(right) || needs_scratch(left) || needs_scratch(right)
        }
        Expression::Unary { operator, operand } => {
            matches!(operator, UnaryOperator::LogicalNot) || needs_scratch(operand)
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
        Expression::Unary { operator, operand } => match operator {
            UnaryOperator::LogicalNot => !destination_is_scratch && fits_single_scratch(operand, destination_is_scratch),
            _ => fits_single_scratch(operand, destination_is_scratch),
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
    signed: bool,
}

struct Generator {
    output: MachineFunction,
    locations: HashMap<String, Location>,
}

fn class_of(declared: Type) -> Compilation<ValueClass> {
    match declared {
        Type::Int | Type::UnsignedInt => Ok(ValueClass::General),
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
            self.locations.insert(
                parameter.name.clone(),
                Location { class, register, signed: parameter.parameter_type.is_signed() },
            );
        }
        Ok(())
    }

    /// Evaluate the locals (if any) and the return expression into the result register.
    fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        let result = match function.return_type {
            Type::Int | Type::UnsignedInt => Eabi::general_result().number,
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
        self.locations.insert(local.name.clone(), Location { class, register: scratch, signed: local.declared_type.is_signed() });
        self.evaluate(return_expression, return_type, result)
    }

    fn evaluate(&mut self, expression: &Expression, value_type: Type, destination: u8) -> Compilation<()> {
        match value_type {
            Type::Int | Type::UnsignedInt => self.evaluate_general(expression, destination),
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
            Expression::Unary { operator, operand } => self.emit_unary(*operator, operand, destination),
            Expression::Binary { operator, left, right } => {
                // Comparisons compile to branchless idioms.
                if is_comparison(*operator) {
                    return self.emit_comparison(*operator, left, right, destination);
                }
                // Right shift, divide, and modulo select instructions by signedness.
                if *operator == BinaryOperator::ShiftRight {
                    return self.emit_shift_right(left, right, destination);
                }
                if *operator == BinaryOperator::Divide {
                    return self.emit_divide(left, right, destination);
                }
                if *operator == BinaryOperator::Modulo {
                    return self.emit_modulo(left, right, destination);
                }
                // A 16-bit constant operand folds into an immediate instruction.
                if self.try_emit_general_with_constant(*operator, left, right, destination)? {
                    return Ok(());
                }
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

    /// Emit a division, choosing signed/unsigned and handling power-of-two
    /// constant divisors; non-power-of-two constants (magic-number lowering) and
    /// signed division by powers of two beyond 2 are deferred.
    fn emit_divide(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)? && self.signedness_of(right)?;
        let d = destination;

        if let Expression::IntegerLiteral(divisor) = right {
            let divisor = *divisor;
            if divisor >= 2 && (divisor as u64).is_power_of_two() {
                if !signed {
                    self.evaluate_general(left, d)?;
                    self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: d, shift: divisor.trailing_zeros() as u8 });
                    return Ok(());
                }
                if divisor == 2 {
                    // signed /2 rounds toward zero: add the sign bit, then arithmetic shift.
                    self.evaluate_general(left, d)?;
                    self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: d, shift: 31 });
                    self.output.instructions.push(Instruction::Add { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: d });
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: GENERAL_SCRATCH, shift: 1 });
                    return Ok(());
                }
            }
            return Err(Diagnostic::error("division by this constant needs magic-number lowering (roadmap)"));
        }

        // register divide
        self.evaluate_general(left, d)?;
        let divisor = if is_complex(right) {
            if !fits_single_scratch(right, true) {
                return Err(Diagnostic::error("divisor needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(right)?
        };
        self.output.instructions.push(if signed {
            Instruction::DivideWord { d, a: d, b: divisor }
        } else {
            Instruction::DivideWordUnsigned { d, a: d, b: divisor }
        });
        Ok(())
    }

    /// Emit a remainder as `left - (left / right) * right` (leaf operands only for now).
    fn emit_modulo(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)? && self.signedness_of(right)?;
        let left_register = self.general_register_of_leaf(left)?;
        let right_register = self.general_register_of_leaf(right)?;
        self.output.instructions.push(if signed {
            Instruction::DivideWord { d: GENERAL_SCRATCH, a: left_register, b: right_register }
        } else {
            Instruction::DivideWordUnsigned { d: GENERAL_SCRATCH, a: left_register, b: right_register }
        });
        self.output.instructions.push(Instruction::MultiplyLow { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: right_register });
        self.output.instructions.push(Instruction::SubtractFrom { d: destination, a: GENERAL_SCRATCH, b: left_register });
        Ok(())
    }

    /// Emit a prefix unary operator into `destination`.
    fn emit_unary(&mut self, operator: UnaryOperator, operand: &Expression, destination: u8) -> Compilation<()> {
        let d = destination;
        match operator {
            UnaryOperator::Negate => {
                self.evaluate_general(operand, d)?;
                self.output.instructions.push(Instruction::Negate { d, a: d });
            }
            UnaryOperator::BitNot => {
                self.evaluate_general(operand, d)?;
                self.output.instructions.push(Instruction::Nor { a: d, s: d, b: d });
            }
            UnaryOperator::LogicalNot => {
                // !x == (x == 0): cntlzw then srwi by 5.
                self.evaluate_general(operand, d)?;
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
            }
        }
        Ok(())
    }

    /// Emit a comparison as mwcc's branchless idiom. Currently handles `==` (and
    /// `== 0`) and signed `< 0`; the richer signed less/greater idioms are not
    /// implemented yet.
    fn emit_comparison(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let d = destination;
        let signed_left = self.signedness_of(left)?;
        match operator {
            BinaryOperator::Equal => {
                if is_zero_literal(right) || is_zero_literal(left) {
                    let value = if is_zero_literal(right) { left } else { right };
                    self.evaluate_general(value, d)?;
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: d });
                } else if let Some(constant) = as_small_integer(right) {
                    // a == c : (c - a) leading zeros
                    let value = self.general_register_of_leaf(left)?;
                    self.output.instructions.push(Instruction::SubtractFromImmediate { d: GENERAL_SCRATCH, a: value, immediate: constant });
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                } else {
                    let left_register = self.general_register_of_leaf(left)?;
                    let right_register = self.general_register_of_leaf(right)?;
                    self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: left_register, b: right_register });
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                }
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 5 });
                Ok(())
            }
            // x != 0 : sign bit of (-x | x)
            BinaryOperator::NotEqual if is_zero_literal(right) => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed x < 0 : the sign bit.
            BinaryOperator::Less if is_zero_literal(right) && signed_left => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: d, shift: 31 });
                Ok(())
            }
            // signed x > 0 : sign bit of (-x & ~x)
            BinaryOperator::Greater if is_zero_literal(right) && signed_left => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: d });
                self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed x >= 0 : !(x < 0)
            BinaryOperator::GreaterEqual if is_zero_literal(right) && signed_left => {
                self.evaluate_general(left, d)?;
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: d, shift: 31 });
                self.output.instructions.push(Instruction::XorImmediate { a: d, s: GENERAL_SCRATCH, immediate: 1 });
                Ok(())
            }
            _ => Err(Diagnostic::error("this comparison needs the branchless compare idioms (roadmap)")),
        }
    }

    /// Whether the value of `expression` is signed (for selecting `>>`). The
    /// usual arithmetic conversions make a binary expression unsigned if either
    /// operand is unsigned.
    fn signedness_of(&self, expression: &Expression) -> Compilation<bool> {
        match expression {
            Expression::IntegerLiteral(_) => Ok(true),
            Expression::FloatLiteral(_) => Ok(true),
            Expression::Variable(name) => {
                Ok(self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?.signed)
            }
            Expression::Binary { operator, left, right } => {
                if is_comparison(*operator) {
                    Ok(true) // a comparison yields an int (signed)
                } else {
                    Ok(self.signedness_of(left)? && self.signedness_of(right)?)
                }
            }
            Expression::Unary { operator, operand } => match operator {
                UnaryOperator::LogicalNot => Ok(true),
                _ => self.signedness_of(operand),
            },
        }
    }

    /// Emit a right shift, choosing arithmetic (signed) or logical (unsigned)
    /// from the type of the shifted value.
    fn emit_shift_right(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)?;
        let d = destination;

        if let Expression::IntegerLiteral(amount) = right {
            if (1..=31).contains(amount) {
                self.evaluate_general(left, d)?;
                let shift = *amount as u8;
                self.output.instructions.push(if signed {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: d, shift }
                } else {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: d, shift }
                });
                return Ok(());
            }
        }

        // Register form: value into the destination, shift amount into a register.
        self.evaluate_general(left, d)?;
        let amount = if is_complex(right) {
            if !fits_single_scratch(right, true) {
                return Err(Diagnostic::error("shift amount needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        } else {
            self.general_register_of_leaf(right)?
        };
        self.output.instructions.push(if signed {
            Instruction::ShiftRightAlgebraicWord { a: d, s: d, b: amount }
        } else {
            Instruction::ShiftRightWord { a: d, s: d, b: amount }
        });
        Ok(())
    }

    /// Fold a constant operand into an immediate instruction. Returns whether an
    /// instruction was emitted; if the constant does not qualify (out of range,
    /// non-mask), returns false so the caller can stop honestly.
    fn try_emit_general_with_constant(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        // variable op constant — subtraction becomes addition of the negation.
        if let Expression::IntegerLiteral(constant) = right {
            let (effective, value) = match operator {
                BinaryOperator::Subtract => (BinaryOperator::Add, -constant),
                other => (other, *constant),
            };
            if self.emit_constant_form(effective, left, value, destination)? {
                return Ok(true);
            }
        }
        // constant op variable — only the commutative operators.
        if is_commutative(operator) {
            if let Expression::IntegerLiteral(constant) = left {
                if self.emit_constant_form(operator, right, *constant, destination)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Evaluate `variable` into `destination` and apply `constant` via the
    /// matching immediate instruction, if the constant qualifies for one.
    fn emit_constant_form(&mut self, operator: BinaryOperator, variable: &Expression, constant: i64, destination: u8) -> Compilation<bool> {
        let d = destination;
        let instruction = match operator {
            BinaryOperator::Add if fits_signed_16(constant) => Instruction::AddImmediate { d, a: d, immediate: constant as i16 },
            BinaryOperator::Multiply if fits_signed_16(constant) => {
                if constant >= 2 && (constant as u64).is_power_of_two() {
                    Instruction::ShiftLeftImmediate { a: d, s: d, shift: constant.trailing_zeros() as u8 }
                } else {
                    Instruction::MultiplyImmediate { d, a: d, immediate: constant as i16 }
                }
            }
            BinaryOperator::BitOr if fits_unsigned_16(constant) => Instruction::OrImmediate { a: d, s: d, immediate: constant as u16 },
            BinaryOperator::BitXor if fits_unsigned_16(constant) => Instruction::XorImmediate { a: d, s: d, immediate: constant as u16 },
            BinaryOperator::BitAnd if low_bit_mask(constant).is_some() => {
                Instruction::ClearLeftImmediate { a: d, s: d, clear: 32 - low_bit_mask(constant).unwrap() }
            }
            BinaryOperator::ShiftLeft if (1..=31).contains(&constant) => Instruction::ShiftLeftImmediate { a: d, s: d, shift: constant as u8 },
            _ => return Ok(false),
        };
        self.evaluate_general(variable, destination)?;
        self.output.instructions.push(instruction);
        Ok(true)
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
            Expression::Unary { .. } => Err(Diagnostic::error("float unary operators not yet supported")),
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

fn float_combine(operator: BinaryOperator, destination: u8, operands: Operands) -> Compilation<Instruction> {
    let (first, second) = operands.commutative();
    Ok(match operator {
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
