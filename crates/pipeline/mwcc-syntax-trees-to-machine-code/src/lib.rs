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
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, Type, UnaryOperator};
use mwcc_target::Eabi;
use mwcc_versions::CompilerBuild;
use std::collections::{HashMap, HashSet};

/// The scratch register mwcc spills the secondary operand of a binary node into.
const GENERAL_SCRATCH: u8 = 0; // r0
const FLOAT_SCRATCH: u8 = 0; // f0

fn is_complex(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Binary { .. } | Expression::Unary { .. } | Expression::Conditional { .. } | Expression::Cast { .. }
    )
}

fn is_zero_literal(expression: &Expression) -> bool {
    matches!(expression, Expression::IntegerLiteral(0))
}

/// The integer value if `expression` is a literal or a negated literal.
fn constant_value(expression: &Expression) -> Option<i64> {
    match expression {
        Expression::IntegerLiteral(value) => Some(*value),
        Expression::Unary { operator: UnaryOperator::Negate, operand } => match operand.as_ref() {
            Expression::IntegerLiteral(value) => Some(-*value),
            _ => None,
        },
        _ => None,
    }
}

/// The variable name if `expression` is a plain variable reference.
fn leaf_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

/// The variable name if `expression` is `~variable`.
fn complemented_leaf_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Unary { operator: UnaryOperator::BitNot, operand } => leaf_name(operand),
        _ => None,
    }
}


/// A nonzero integer literal that fits a signed 16-bit immediate.
fn as_small_integer(expression: &Expression) -> Option<i16> {
    match expression {
        Expression::IntegerLiteral(value) if *value != 0 => i16::try_from(*value).ok(),
        _ => None,
    }
}

/// The `(BO, BI)` of the branch that fires when `operator` is **true** (cr0 bits:
/// 0=LT, 1=GT, 2=EQ; BO 12 = if-true, 4 = if-false). The negated branch is
/// `(BO ^ 8, BI)`.
fn positive_branch(operator: BinaryOperator) -> (u8, u8) {
    match operator {
        BinaryOperator::Greater => (12, 1),
        BinaryOperator::Less => (12, 0),
        BinaryOperator::GreaterEqual => (4, 0),
        BinaryOperator::LessEqual => (4, 1),
        BinaryOperator::Equal => (12, 2),
        BinaryOperator::NotEqual => (4, 2),
        _ => (12, 2),
    }
}

/// The logical negation of a comparison operator (`==`↔`!=`, `<`↔`>=`, `>`↔`<=`).
fn flip_comparison(operator: BinaryOperator) -> Option<BinaryOperator> {
    Some(match operator {
        BinaryOperator::Equal => BinaryOperator::NotEqual,
        BinaryOperator::NotEqual => BinaryOperator::Equal,
        BinaryOperator::Less => BinaryOperator::GreaterEqual,
        BinaryOperator::GreaterEqual => BinaryOperator::Less,
        BinaryOperator::Greater => BinaryOperator::LessEqual,
        BinaryOperator::LessEqual => BinaryOperator::Greater,
        _ => return None,
    })
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

/// If `value` is a single contiguous run of set bits, return the PowerPC
/// `(mask_begin, mask_end)` for `rlwinm rA,rS,0,begin,end`.
fn contiguous_mask(value: i64) -> Option<(u8, u8)> {
    let mask = value as u32;
    if mask == 0 {
        return None;
    }
    let lowest = mask.trailing_zeros();
    let highest = 31 - mask.leading_zeros();
    let shifted = mask >> lowest;
    if shifted & shifted.wrapping_add(1) != 0 {
        return None; // not a single contiguous run
    }
    Some(((31 - highest) as u8, (31 - lowest) as u8))
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
        Expression::Conditional { .. } => true,
        Expression::Cast { .. } => true,
        _ => false,
    }
}

/// Whether a type is a narrow integer (sub-32-bit), whose values are extended
/// when read and truncated when produced as a result.
fn is_narrow_int(value_type: Type) -> bool {
    matches!(value_type, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
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
        // conditionals and casts are only handled at the top of an evaluation,
        // not nested inside the single-scratch tree model
        Expression::Conditional { .. } | Expression::Cast { .. } => false,
        _ => true,
    }
}

/// Lower a parsed function to machine code for the given compiler build.
pub fn lower_function(function: &Function, build: CompilerBuild) -> Compilation<MachineFunction> {
    let mut generator = Generator {
        output: MachineFunction::new(function.name.clone()),
        locations: HashMap::new(),
        reserved: HashSet::new(),
        frame_size: 0,
        build,
    };
    generator.assign_parameters(function)?;
    generator.evaluate_body(function)?;
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
    /// Integer width in bits (8/16/32); narrow values are extended when read.
    width: u8,
}

struct Generator {
    output: MachineFunction,
    locations: HashMap<String, Location>,
    /// Registers holding live values that must not be clobbered while a sibling
    /// sub-expression is being evaluated. The allocator draws temporaries from
    /// the registers outside this set.
    reserved: HashSet<u8>,
    /// Stack frame size in bytes (0 = leaf function, no frame). Set when an
    /// operation needs scratch stack space (e.g. an int/float conversion).
    frame_size: i16,
    /// The build we are reproducing. Its only codegen-affecting knob today is
    /// the default signedness of plain `char` (see [`Generator::signed_of`]).
    build: CompilerBuild,
}

fn class_of(declared: Type) -> Compilation<ValueClass> {
    match declared {
        Type::Float => Ok(ValueClass::Float),
        Type::Void => Err(Diagnostic::error("a value cannot have type void")),
        _ => Ok(ValueClass::General),
    }
}

impl Generator {
    /// Signedness of a source-level type for the target build. Plain `char` is
    /// the one type whose signedness is build-dependent (unsigned in GC/1.3
    /// build 53, signed from build 81 on); every other type is fixed. Routing
    /// all type-signedness queries through here makes the whole cascade — read
    /// extension, `>>`/`/`/`%` strength reduction, comparison folding, and the
    /// int->float bias — follow the build with no scattered version checks.
    fn signed_of(&self, declared: Type) -> bool {
        match declared {
            Type::Char => self.build.char_is_signed,
            other => other.is_signed(),
        }
    }

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
            let signed = self.signed_of(parameter.parameter_type);
            self.locations.insert(
                parameter.name.clone(),
                Location { class, register, signed, width: parameter.parameter_type.width() },
            );
        }
        Ok(())
    }

    /// Emit the whole function body, including its `blr`(s).
    fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        let result = match function.return_type {
            Type::Float => Eabi::float_result().number,
            Type::Void => {
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }
            _ => Eabi::general_result().number,
        };

        if !function.guards.is_empty() {
            if !function.locals.is_empty() {
                return Err(Diagnostic::error("locals combined with guards not yet supported"));
            }
            // mwcc lowers a single guard as a select (working-register form) but a
            // chain of guards as separate return blocks.
            if let [guard] = function.guards.as_slice() {
                let select = Expression::Conditional {
                    condition: Box::new(guard.condition.clone()),
                    when_true: Box::new(guard.value.clone()),
                    when_false: Box::new(function.return_expression.clone()),
                };
                self.evaluate_tail(&select, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }
            return self.emit_guard_sequence(&function.guards, &function.return_expression, function.return_type, result);
        }

        match function.locals.as_slice() {
            [] => self.evaluate_tail(&function.return_expression, function.return_type, result)?,
            [local] => self.evaluate_single_local(local, &function.return_expression, function.return_type, result)?,
            _ => return Err(Diagnostic::error("multiple locals need the full register allocator (roadmap M1)")),
        }
        // Tear down the stack frame, if one was allocated.
        if self.frame_size != 0 {
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }

    /// Emit a sequence of `if (c) return v;` guards followed by the final return.
    /// Each guard is its own block ending in `blr`; the last guard collapses the
    /// final return into a conditional return when the final value already sits in
    /// the result register.
    fn emit_guard_sequence(
        &mut self,
        guards: &[GuardedReturn],
        final_return: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let final_in_result = match final_return {
            Expression::Variable(name) => self.locations.get(name).map(|location| location.register) == Some(result),
            _ => false,
        };

        for (index, guard) in guards.iter().enumerate() {
            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            let value_register = self.general_register_of_leaf(&guard.value)?;
            let is_last = index + 1 == guards.len();

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                if result != value_register {
                    self.output.instructions.push(Instruction::move_register(result, value_register));
                }
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            if result != value_register {
                self.output.instructions.push(Instruction::move_register(result, value_register));
            }
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let next = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = next;
            }
        }

        // Final fall-through return.
        self.evaluate_tail(final_return, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }

    /// Evaluate the function result. A conditional in this tail position can use a
    /// conditional return when one of its values already sits in the result register.
    fn evaluate_tail(&mut self, expression: &Expression, value_type: Type, result: u8) -> Compilation<()> {
        match expression {
            Expression::Conditional { condition, when_true, when_false } => match value_type {
                Type::Float => self.emit_float_conditional(condition, when_true, when_false, result, true),
                _ => self.emit_conditional(condition, when_true, when_false, result, true),
            },
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), left, right } => {
                self.emit_short_circuit(*operator, left, right, result)
            }
            // A narrow return type truncates the returned value. A `(type)` cast
            // expression already yields the narrow type, so it falls through to the
            // normal path; everything else is coerced here.
            other if is_narrow_int(value_type) && !matches!(other, Expression::Cast { .. }) => {
                self.evaluate_narrow_return(other, value_type, result)
            }
            other => self.evaluate(other, value_type, result),
        }
    }

    /// Whether the expression reads any narrow variable. A narrow return whose
    /// expression reads narrow operands relies on mwcc's optimization that elides
    /// operand extension because the result is truncated anyway — not yet modeled.
    fn contains_narrow_leaf(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(_) => self.is_narrow_leaf(expression),
            Expression::Binary { left, right, .. } => self.contains_narrow_leaf(left) || self.contains_narrow_leaf(right),
            Expression::Unary { operand, .. } => self.contains_narrow_leaf(operand),
            Expression::Conditional { condition, when_true, when_false } => {
                self.contains_narrow_leaf(condition) || self.contains_narrow_leaf(when_true) || self.contains_narrow_leaf(when_false)
            }
            Expression::Cast { operand, .. } => self.contains_narrow_leaf(operand),
            _ => false,
        }
    }

    /// Coerce a returned value to a narrow return type. mwcc truncates a bare wide
    /// variable in place (`extsb`/`extsh`/`clrlwi r3,r3`) and computes a wider
    /// expression into the scratch before truncating it into the result
    /// (`addi r0,r3,1; extsb r3,r0`). The optimization that elides operand
    /// extension when the result is truncated is not modeled, so a computation
    /// reading a narrow operand is deferred rather than mis-extended.
    fn evaluate_narrow_return(&mut self, expression: &Expression, return_type: Type, result: u8) -> Compilation<()> {
        let width = return_type.width();
        let signed = self.signed_of(return_type);

        if let Expression::Variable(_) = expression {
            let (register, variable_width, _) = self.leaf_info(expression)?;
            if register != result {
                return Err(Diagnostic::error("narrow return of a non-result variable (roadmap M1)"));
            }
            // A wider variable is truncated; one already this narrow needs nothing.
            if variable_width > width {
                self.emit_widen(result, register, width, signed);
            }
            return Ok(());
        }

        if self.contains_narrow_leaf(expression) {
            return Err(Diagnostic::error("narrow return of a narrow-operand expression needs the truncation-context optimization (roadmap)"));
        }
        if needs_scratch(expression) {
            return Err(Diagnostic::error("narrow return of a scratch-needing expression (roadmap M1)"));
        }
        self.evaluate_general(expression, GENERAL_SCRATCH)?;
        self.emit_widen(result, GENERAL_SCRATCH, width, signed);
        Ok(())
    }

    /// Emit a short-circuit `&&`/`||` in tail position as mwcc does: each operand
    /// is tested (a leaf against zero, a comparison directly) with an early
    /// conditional return. Each operand may be a leaf or a comparison.
    fn emit_short_circuit(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, result: u8) -> Compilation<()> {
        // If the right operand still reads the result register, the running result
        // cannot live there; mwcc computes it in r0 and copies it out at the end.
        if self.registers_used_by(right).contains(&result) {
            return self.emit_short_circuit_via_scratch(operator, left, right, result);
        }
        match operator {
            BinaryOperator::LogicalAnd => {
                // test left; result 0; return 0 if left false; test right; return 0 if right false; result 1.
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(result, 0));
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: left_skip, condition_bit: left_bit });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: right_skip, condition_bit: right_bit });
                self.output.instructions.push(Instruction::load_immediate(result, 1));
            }
            BinaryOperator::LogicalOr => {
                // test left; result 0; if left true skip to result 1; test right; return 0 if right false; result 1.
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(result, 0));
                let branch_index = self.output.instructions.len();
                // the branch taken when left is TRUE is the negation of the skip-when-false branch.
                self.output.instructions.push(Instruction::BranchConditionalForward { options: left_skip ^ 8, condition_bit: left_bit, target: 0 });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: right_skip, condition_bit: right_bit });
                let set_one = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = set_one;
                }
                self.output.instructions.push(Instruction::load_immediate(result, 1));
            }
            _ => unreachable!("caller restricts to logical and/or"),
        }
        Ok(())
    }

    /// Short-circuit `&&`/`||` whose result is built in the scratch register and
    /// copied to the destination at a common exit — used when the destination
    /// register is still needed by the right operand.
    fn emit_short_circuit_via_scratch(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, result: u8) -> Compilation<()> {
        let scratch = GENERAL_SCRATCH;
        match operator {
            BinaryOperator::LogicalAnd => {
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(scratch, 0));
                let exit_a = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: left_skip, condition_bit: left_bit, target: 0 });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                let exit_b = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: right_skip, condition_bit: right_bit, target: 0 });
                self.output.instructions.push(Instruction::load_immediate(scratch, 1));
                let exit = self.output.instructions.len();
                self.patch_forward(exit_a, exit);
                self.patch_forward(exit_b, exit);
                self.output.instructions.push(Instruction::move_register(result, scratch));
            }
            BinaryOperator::LogicalOr => {
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(scratch, 0));
                let to_set_one = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: left_skip ^ 8, condition_bit: left_bit, target: 0 });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                let to_exit = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: right_skip, condition_bit: right_bit, target: 0 });
                let set_one = self.output.instructions.len();
                self.output.instructions.push(Instruction::load_immediate(scratch, 1));
                let exit = self.output.instructions.len();
                self.patch_forward(to_set_one, set_one);
                self.patch_forward(to_exit, exit);
                self.output.instructions.push(Instruction::move_register(result, scratch));
            }
            _ => unreachable!("caller restricts to logical and/or"),
        }
        Ok(())
    }

    fn patch_forward(&mut self, branch_index: usize, target: usize) {
        if let Instruction::BranchConditionalForward { target: slot, .. } = &mut self.output.instructions[branch_index] {
            *slot = target;
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
        let signed = self.signed_of(local.declared_type);
        self.locations.insert(local.name.clone(), Location { class, register: scratch, signed, width: local.declared_type.width() });
        self.evaluate(return_expression, return_type, result)
    }

    fn evaluate(&mut self, expression: &Expression, value_type: Type, destination: u8) -> Compilation<()> {
        match value_type {
            Type::Float => self.evaluate_float(expression, destination),
            Type::Void => Err(Diagnostic::error("cannot evaluate a void expression")),
            _ => self.evaluate_general(expression, destination),
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
                let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
                if location.class != ValueClass::General {
                    return Err(Diagnostic::error(format!("'{name}' is not an integer")));
                }
                let (source, width, signed) = (location.register, location.width, location.signed);
                self.emit_widen(destination, source, width, signed);
                Ok(())
            }
            Expression::Unary { operator, operand } => self.emit_unary(*operator, operand, destination),
            Expression::Conditional { condition, when_true, when_false } => {
                self.emit_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { target_type, operand } => self.emit_cast_to_integer(*target_type, operand, destination),
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
                // `x & ~y` / `x | ~y` fuse into andc/orc.
                if matches!(operator, BinaryOperator::BitAnd | BinaryOperator::BitOr)
                    && self.try_emit_complement_logical(*operator, left, right, destination)
                {
                    return Ok(());
                }
                // A 16-bit constant operand folds into an immediate instruction.
                if self.try_emit_general_with_constant(*operator, left, right, destination)? {
                    return Ok(());
                }
                if !fits_single_scratch(expression, destination == GENERAL_SCRATCH) {
                    return Err(Diagnostic::error("expression needs the full register allocator (roadmap M1)"));
                }
                let operands = self.place_general_operands(*operator, left, right, destination)?;
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
                    let shift = divisor.trailing_zeros() as u8;
                    // Unsigned `/2^k` is a logical right shift; a narrow operand
                    // fuses the extension and shift into one rlwinm like `>>`.
                    if let Ok((register, width, _)) = self.leaf_info(left) {
                        if width < 32 {
                            if self.emit_narrow_unsigned_shift(d, register, width, false, shift) {
                                return Ok(());
                            }
                            return Err(Diagnostic::error("narrow unsigned divide out of the single-rlwinm range (roadmap)"));
                        }
                    }
                    self.evaluate_general(left, d)?;
                    self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: d, shift });
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

        // register divide: dividend (leaf stays, sub-expr -> scratch), then divisor.
        let Some(dividend) = self.place_operand(left, d, false)? else {
            return Err(Diagnostic::error("dividend needs the full register allocator (roadmap M1)"));
        };
        let divisor = if let Some(register) = leaf_name(right).and_then(|name| self.lookup_general(name)) {
            register
        } else {
            // a sub-expression divisor needs the scratch, which the dividend may occupy.
            if dividend == GENERAL_SCRATCH {
                return Err(Diagnostic::error("divisor and dividend both need scratch (roadmap M1)"));
            }
            if !fits_single_scratch(right, true) {
                return Err(Diagnostic::error("divisor needs the full register allocator (roadmap M1)"));
            }
            self.evaluate_general(right, GENERAL_SCRATCH)?;
            GENERAL_SCRATCH
        };
        self.output.instructions.push(if signed {
            Instruction::DivideWord { d, a: dividend, b: divisor }
        } else {
            Instruction::DivideWordUnsigned { d, a: dividend, b: divisor }
        });
        Ok(())
    }

    /// Emit a remainder as `left - (left / right) * right` (leaf operands only for now).
    fn emit_modulo(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)? && self.signedness_of(right)?;

        // Unsigned modulo by a power of two is a low-bit mask: a % 2^k == a & (2^k - 1).
        if !signed {
            if let Expression::IntegerLiteral(divisor) = right {
                if *divisor >= 2 && (*divisor as u64).is_power_of_two() {
                    let Some(source) = self.place_operand(left, destination, false)? else {
                        return Err(Diagnostic::error("modulo value needs the full register allocator (roadmap M1)"));
                    };
                    let clear = 32 - divisor.trailing_zeros() as u8;
                    self.output.instructions.push(Instruction::ClearLeftImmediate { a: destination, s: source, clear });
                    return Ok(());
                }
            }
        }

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

    /// Emit a ternary `condition ? when_true : when_false` into `destination`,
    /// matching mwcc's shape: the false value's register is the working register,
    /// conditionally overwritten with the true value, then moved to the result.
    /// Leaf operands only for now.
    fn emit_conditional(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<()> {
        // `comparison ? 1 : 0` is the comparison; `comparison ? 0 : 1` is its negation.
        if let Expression::Binary { operator, left, right } = condition {
            if is_comparison(*operator) {
                match (constant_value(when_true), constant_value(when_false)) {
                    (Some(1), Some(0)) => return self.evaluate_general(condition, destination),
                    (Some(0), Some(1)) => {
                        let flipped = flip_comparison(*operator).unwrap();
                        return self.emit_comparison(flipped, left, right, destination);
                    }
                    _ => {}
                }
            }
        }

        // `cond ? x : 0` with a plain truth condition is branchless: AND x with a
        // mask that is all-ones when cond != 0.
        if is_zero_literal(when_false) {
            if let (Some(condition_name), Some(value_name)) = (leaf_name(condition), leaf_name(when_true)) {
                if let (Some(condition_register), Some(value_register)) =
                    (self.lookup_general(condition_name), self.lookup_general(value_name))
                {
                    self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: condition_register });
                    self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: condition_register });
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
                    self.output.instructions.push(Instruction::And { a: destination, s: value_register, b: GENERAL_SCRATCH });
                    return Ok(());
                }
            }
        }

        let true_register = self.general_register_of_leaf(when_true)?;
        let false_register = self.general_register_of_leaf(when_false)?;

        // Emit the condition test and the branch that skips the true path when it fails.
        let (options, condition_bit) = self.emit_condition_test(condition)?;

        // In tail position, when the false value already sits in the result
        // register, return early on the false path instead of branching forward.
        if tail && false_register == destination {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            if destination != true_register {
                self.output.instructions.push(Instruction::move_register(destination, true_register));
            }
            return Ok(());
        }

        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.output.instructions.push(Instruction::move_register(false_register, true_register));

        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        if destination != false_register {
            self.output.instructions.push(Instruction::move_register(destination, false_register));
        }
        Ok(())
    }

    /// Emit a float `condition ? when_true : when_false`. The condition must be a
    /// float comparison; in tail position, when one branch value already sits in
    /// the result register, return early on that branch (fcmpo + bclr).
    fn emit_float_conditional(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<()> {
        let Expression::Binary { operator, left, right } = condition else {
            return Err(Diagnostic::error("float conditional needs a comparison condition"));
        };
        if !is_comparison(*operator) {
            return Err(Diagnostic::error("float conditional needs a comparison condition"));
        }
        let left_register = self.float_register_of_leaf(left)?;
        let right_register = self.float_register_of_leaf(right)?;
        let true_register = self.float_register_of_leaf(when_true)?;
        let false_register = self.float_register_of_leaf(when_false)?;

        self.output.instructions.push(Instruction::FloatCompareOrdered { a: left_register, b: right_register });
        let (positive_options, condition_bit) = positive_branch(*operator);

        if tail && true_register == destination {
            // true value already in the result: return on the true branch.
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: positive_options, condition_bit });
            if destination != false_register {
                self.output.instructions.push(Instruction::FloatMove { d: destination, b: false_register });
            }
            return Ok(());
        }
        if tail && false_register == destination {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: positive_options ^ 8, condition_bit });
            if destination != true_register {
                self.output.instructions.push(Instruction::FloatMove { d: destination, b: true_register });
            }
            return Ok(());
        }
        Err(Diagnostic::error("non-tail float select not yet supported"))
    }

    /// Emit the test for a branch condition and return the `(BO, BI)` of the
    /// branch that skips the guarded code when the condition is **false**. A
    /// comparison condition uses `cmpw`/`cmpwi` with the negated relation; any
    /// other expression is tested against zero (`!= 0`).
    fn emit_condition_test(&mut self, condition: &Expression) -> Compilation<(u8, u8)> {
        // `!x` as a condition is `x == 0`: skip the guarded code when x != 0.
        if let Expression::Unary { operator: UnaryOperator::LogicalNot, operand } = condition {
            let register = self.general_register_of_leaf(operand)?;
            self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
            return Ok((4, 2)); // bne — skip when x != 0
        }
        if let Expression::Binary { operator, left, right } = condition {
            if is_comparison(*operator) {
                let signed = self.signedness_of(left)? && self.signedness_of(right)?;
                let left_register = self.general_register_of_leaf(left)?;
                match (as_small_integer(right), is_zero_literal(right)) {
                    (Some(constant), _) if signed => {
                        self.output.instructions.push(Instruction::CompareWordImmediate { a: left_register, immediate: constant });
                    }
                    (Some(constant), _) => {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: left_register, immediate: constant as u16 });
                    }
                    (None, true) if signed => {
                        self.output.instructions.push(Instruction::CompareWordImmediate { a: left_register, immediate: 0 });
                    }
                    (None, true) => {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: left_register, immediate: 0 });
                    }
                    (None, false) => {
                        let right_register = self.general_register_of_leaf(right)?;
                        if signed {
                            self.output.instructions.push(Instruction::CompareWord { a: left_register, b: right_register });
                        } else {
                            self.output.instructions.push(Instruction::CompareLogicalWord { a: left_register, b: right_register });
                        }
                    }
                }
                // Branch when the comparison is false. BO: 4 = if-false, 12 = if-true. BI: 0=LT,1=GT,2=EQ.
                return Ok(match operator {
                    BinaryOperator::Greater => (4, 1),      // ble
                    BinaryOperator::Less => (4, 0),         // bge
                    BinaryOperator::GreaterEqual => (12, 0), // blt
                    BinaryOperator::LessEqual => (12, 1),    // bgt
                    BinaryOperator::Equal => (4, 2),         // bne
                    BinaryOperator::NotEqual => (12, 2),     // beq
                    _ => unreachable!("is_comparison restricts the operator"),
                });
            }
        }
        // Plain truth test: compare against zero, skip when equal.
        let register = self.general_register_of_leaf(condition)?;
        self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
        Ok((12, 2)) // beq — skip when condition == 0
    }

    /// Emit a cast of an integer operand to a float in `destination` — mwcc's
    /// magic-constant conversion: bias the integer (flip its sign bit), assemble
    /// the double `0x43300000_<biased int>` on the stack, and subtract the bias
    /// `0x4330000000000000`. The bias double lives in `.sdata2`; the `lfd dest,0(0)`
    /// is byte-correct here, but its `R_PPC_EMB_SDA21` relocation and the constant
    /// pool are the next M3 step. Leaf integer operands only.
    fn emit_cast_to_float(&mut self, operand: &Expression, destination: u8) -> Compilation<()> {
        let source = self.general_register_of_leaf(operand)?;
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::XorImmediateShifted { a: source, s: source, immediate: 0x8000 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, 17200)); // lis r0, 0x4330
        // The bias load and the value store are independent; builds schedule them
        // in opposite orders (GC/2.0p1 stores first, every other build loads first).
        if self.build.float_cast_value_store_first {
            self.output.instructions.push(Instruction::StoreWord { s: source, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::LoadFloatDouble { d: destination, a: 0, offset: 0 }); // bias (needs reloc)
        } else {
            self.output.instructions.push(Instruction::LoadFloatDouble { d: destination, a: 0, offset: 0 }); // bias (needs reloc)
            self.output.instructions.push(Instruction::StoreWord { s: source, a: 1, offset: 12 });
        }
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: FLOAT_SCRATCH, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatSubtractSingle { d: destination, a: FLOAT_SCRATCH, b: destination });
        Ok(())
    }

    /// Move/extend a value of `width` bits from `source` into `destination`,
    /// sign- or zero-extending narrow values to 32 bits.
    fn emit_widen(&mut self, destination: u8, source: u8, width: u8, signed: bool) {
        match (width, signed) {
            (8, true) => self.output.instructions.push(Instruction::ExtendSignByte { a: destination, s: source }),
            (16, true) => self.output.instructions.push(Instruction::ExtendSignHalfword { a: destination, s: source }),
            (8, false) => self.output.instructions.push(Instruction::ClearLeftImmediate { a: destination, s: source, clear: 24 }),
            (16, false) => self.output.instructions.push(Instruction::ClearLeftImmediate { a: destination, s: source, clear: 16 }),
            _ if source != destination => self.output.instructions.push(Instruction::move_register(destination, source)),
            _ => {}
        }
    }

    /// Emit mwcc's fused `rlwinm` for an unsigned narrow value shifted by a
    /// constant. A `width`-bit value occupies the low `width` bits, starting at
    /// big-endian bit `32-width`. `<< n` rotates left n and keeps the shifted
    /// window; `>> n` rotates by `32-n`. Returns false when the shift would push
    /// significant bits out of the single-rlwinm range (deferred, not modeled).
    fn emit_narrow_unsigned_shift(&mut self, destination: u8, source: u8, width: u8, left: bool, amount: u8) -> bool {
        let start = 32 - width as u32; // first significant big-endian bit: uchar=24, ushort=16
        let n = amount as u32;
        let (shift, begin, end) = if left {
            if n == 0 || n > start {
                return false;
            }
            (n, start - n, 31 - n)
        } else {
            if n == 0 || n >= width as u32 {
                return false;
            }
            (32 - n, start + n, 31)
        };
        self.output.instructions.push(Instruction::RotateAndMask {
            a: destination,
            s: source,
            shift: shift as u8,
            begin: begin as u8,
            end: end as u8,
        });
        true
    }

    /// Whether `expression` is a float-valued leaf.
    fn is_float_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name) if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::Float))
    }

    /// Emit a cast of a float operand to an integer in `destination`. mwcc
    /// converts with `fctiwz`, then bounces the value through the stack frame.
    /// Leaf float operands only for now; int->float (the constant-pool direction)
    /// is handled separately once .sdata2 lands.
    fn emit_cast_to_integer(&mut self, target_type: Type, operand: &Expression, destination: u8) -> Compilation<()> {
        if self.is_float_leaf(operand) {
            // float -> int: convert, bounce through the frame, then narrow if needed.
            let source = self.float_register_of_leaf(operand)?;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::ConvertToIntegerWordZero { d: FLOAT_SCRATCH, b: source });
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::StoreFloatDouble { s: FLOAT_SCRATCH, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::LoadWord { d: destination, a: 1, offset: 12 });
            if target_type.width() < 32 {
                self.emit_widen(destination, destination, target_type.width(), self.signed_of(target_type));
            }
            return Ok(());
        }
        // int -> int narrowing: place the operand (sub-expression -> scratch),
        // then extend/truncate to the target width into the destination.
        if target_type.width() < 32 {
            let Some(source) = self.place_operand(operand, destination, false)? else {
                return Err(Diagnostic::error("cast operand needs the full register allocator (roadmap M1)"));
            };
            self.emit_widen(destination, source, target_type.width(), self.signed_of(target_type));
        } else {
            self.evaluate_general(operand, destination)?;
        }
        Ok(())
    }

    /// If one operand is `~leaf` and the other is a leaf, emit `andc`/`orc`.
    fn try_emit_complement_logical(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, destination: u8) -> bool {
        let (kept_expression, complemented_name) = if let Some(name) = complemented_leaf_name(right) {
            (left, name)
        } else if let Some(name) = complemented_leaf_name(left) {
            (right, name)
        } else {
            return false;
        };
        let (Some(kept_name), Some(complemented_register)) = (leaf_name(kept_expression), self.lookup_general(complemented_name)) else {
            return false;
        };
        let Some(kept_register) = self.lookup_general(kept_name) else {
            return false;
        };
        self.output.instructions.push(match operator {
            BinaryOperator::BitAnd => Instruction::AndComplement { a: destination, s: kept_register, b: complemented_register },
            _ => Instruction::OrComplement { a: destination, s: kept_register, b: complemented_register },
        });
        true
    }

    fn lookup_general(&self, name: &str) -> Option<u8> {
        self.locations.get(name).filter(|location| location.class == ValueClass::General).map(|location| location.register)
    }

    /// Whether `expression` is a narrow (sub-32-bit) integer variable. Such an
    /// operand needs width extension before use, and a few consumers (left shift
    /// and pow2 multiply) fuse extension and shift into a single `rlwinm` on the
    /// builds that treat `char` as unsigned — a peephole we do not model yet, so
    /// those callers defer narrow operands rather than emit non-matching bytes.
    fn is_narrow_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name)
            if self.locations.get(name.as_str()).is_some_and(|l| l.class == ValueClass::General && l.width < 32))
    }

    /// Place an operand and return the register holding it. A leaf stays in its
    /// own register. A sub-expression is computed into the destination when the
    /// consumer can fold it there (`addi`), otherwise into the scratch register —
    /// mwcc keeps `addi` operands in place but routes `rlwinm`/logical operands
    /// through `r0`. Returns `None` when a scratch operand does not fit.
    fn place_operand(&mut self, operand: &Expression, destination: u8, prefer_destination: bool) -> Compilation<Option<u8>> {
        if let Expression::Variable(name) = operand {
            let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
            let (register, width, signed) = (location.register, location.width, location.signed);
            if width == 32 {
                return Ok(Some(register));
            }
            // A narrow operand is width-extended to 32 bits before use. The
            // extension lands in the consumer's working register: the destination
            // for addi-family consumers that keep their operand in place, otherwise
            // the scratch (mwcc routes `extsb r0,rX` ahead of an `rlwinm`/`mulli`).
            let target = if prefer_destination { destination } else { GENERAL_SCRATCH };
            self.emit_widen(target, register, width, signed);
            return Ok(Some(target));
        }
        if prefer_destination {
            self.evaluate_general(operand, destination)?;
            Ok(Some(destination))
        } else {
            if !fits_single_scratch(operand, true) {
                return Ok(None);
            }
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            Ok(Some(GENERAL_SCRATCH))
        }
    }

    /// Emit a prefix unary operator into `destination`.
    fn emit_unary(&mut self, operator: UnaryOperator, operand: &Expression, destination: u8) -> Compilation<()> {
        let d = destination;
        match operator {
            UnaryOperator::Negate => {
                // Negating a literal folds to loading the negated constant.
                if let Expression::IntegerLiteral(value) = operand {
                    self.load_integer_constant(d, -*value);
                    return Ok(());
                }
                // -(-x) == x
                if let Expression::Unary { operator: UnaryOperator::Negate, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                let Some(source) = self.place_operand(operand, d, false)? else {
                    return Err(Diagnostic::error("negation operand needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::Negate { d, a: source });
            }
            UnaryOperator::BitNot => {
                // ~(~x) == x
                if let Expression::Unary { operator: UnaryOperator::BitNot, operand: inner } = operand {
                    return self.evaluate_general(inner, d);
                }
                let Some(source) = self.place_operand(operand, d, false)? else {
                    return Err(Diagnostic::error("complement operand needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::Nor { a: d, s: source, b: source });
            }
            UnaryOperator::LogicalNot => {
                // !(comparison) is the flipped comparison.
                if let Expression::Binary { operator, left, right } = operand {
                    if let Some(flipped) = (is_comparison(*operator)).then(|| flip_comparison(*operator)).flatten() {
                        return self.emit_comparison(flipped, left, right, d);
                    }
                }
                // !x == (x == 0): cntlzw then srwi by 5.
                let Some(source) = self.place_operand(operand, d, false)? else {
                    return Err(Diagnostic::error("logical-not operand needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
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
                    let Some(source) = self.place_operand(value, d, false)? else {
                        return Err(Diagnostic::error("comparison value needs the full register allocator (roadmap M1)"));
                    };
                    self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: source });
                } else if let Some(constant) = as_small_integer(right) {
                    // a == c : (c - a) leading zeros. A narrow operand is extended
                    // into the scratch first (extsb/clrlwi), then consumed there.
                    let value = match self.leaf_info(left) {
                        Ok((register, width, signed)) if width < 32 => {
                            self.emit_widen(GENERAL_SCRATCH, register, width, signed);
                            GENERAL_SCRATCH
                        }
                        _ => self.general_register_of_leaf(left)?,
                    };
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
                let Some(source) = self.place_operand(left, d, false)? else {
                    return Err(Diagnostic::error("comparison value needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: source, shift: 31 });
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
                let Some(source) = self.place_operand(left, d, false)? else {
                    return Err(Diagnostic::error("comparison value needs the full register allocator (roadmap M1)"));
                };
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: source, shift: 31 });
                self.output.instructions.push(Instruction::XorImmediate { a: d, s: GENERAL_SCRATCH, immediate: 1 });
                Ok(())
            }
            // general signed branchless comparisons (leaf operands)
            BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::NotEqual
                if signed_left && leaf_name(left).is_some() && leaf_name(right).is_some() =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                let scratch = GENERAL_SCRATCH;
                match operator {
                    // a < b : sign bit of (((a^b)>>1) - ((a^b)&b))
                    BinaryOperator::Less => {
                        self.output.instructions.push(Instruction::Xor { a: scratch, s: right_register, b: left_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: d, s: scratch, shift: 1 });
                        self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: right_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: d });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                    // a > b : sign bit of (((a^b)>>1) - ((a^b)&a)), reusing rB as a temp
                    BinaryOperator::Greater => {
                        self.output.instructions.push(Instruction::Xor { a: scratch, s: left_register, b: right_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: right_register, s: scratch, shift: 1 });
                        self.output.instructions.push(Instruction::And { a: scratch, s: scratch, b: left_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: scratch, b: right_register });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                    // a != b : sign bit of ((b - a) | (a - b)), with a second temp
                    _ => {
                        let temp = (3u8..=12).find(|r| ![left_register, right_register, scratch].contains(r)).ok_or_else(|| Diagnostic::error("out of registers"))?;
                        self.output.instructions.push(Instruction::SubtractFrom { d: temp, a: left_register, b: right_register });
                        self.output.instructions.push(Instruction::SubtractFrom { d: scratch, a: right_register, b: left_register });
                        self.output.instructions.push(Instruction::Or { a: scratch, s: temp, b: scratch });
                        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: scratch, shift: 31 });
                    }
                }
                Ok(())
            }
            // unsigned a < b / a > b : xor/cntlzw/slw/srwi.
            BinaryOperator::Less | BinaryOperator::Greater
                if !signed_left && leaf_name(left).is_some() && leaf_name(right).is_some() =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                // a < b uses b as the high side; a > b is b < a.
                let high = if matches!(operator, BinaryOperator::Less) { right_register } else { left_register };
                let low = if matches!(operator, BinaryOperator::Less) { left_register } else { right_register };
                self.output.instructions.push(Instruction::Xor { a: GENERAL_SCRATCH, s: high, b: low });
                self.output.instructions.push(Instruction::CountLeadingZeros { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftLeftWord { a: GENERAL_SCRATCH, s: high, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // unsigned a <= b / a >= b : orc-based, dest + scratch.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if !signed_left && leaf_name(left).is_some() && leaf_name(right).is_some() =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                // a<=b uses (low,high)=(a,b); a>=b is b<=a.
                let (low, high) = match operator {
                    BinaryOperator::LessEqual => (left_register, right_register),
                    _ => (right_register, left_register),
                };
                self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: low, b: high });
                self.output.instructions.push(Instruction::OrComplement { a: d, s: high, b: low });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 1 });
                self.output.instructions.push(Instruction::SubtractFrom { d: GENERAL_SCRATCH, a: GENERAL_SCRATCH, b: d });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: d, s: GENERAL_SCRATCH, shift: 31 });
                Ok(())
            }
            // signed a <= b / a >= b : carry-based, with two temporaries.
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual
                if signed_left && leaf_name(left).is_some() && leaf_name(right).is_some() =>
            {
                let left_register = self.general_register_of_leaf(left)?;
                let right_register = self.general_register_of_leaf(right)?;
                let mut free = (3u8..=12).filter(|r| ![left_register, right_register, GENERAL_SCRATCH].contains(r));
                let (Some(lower), Some(higher)) = (free.next(), free.next()) else {
                    return Err(Diagnostic::error("out of registers for comparison"));
                };
                // For a<=b: high = sign(b), low = sign(a), carry from (b - a).
                // For a>=b the operands swap.
                let (sign_high, sign_low, subtrahend, minuend) = match operator {
                    BinaryOperator::LessEqual => (right_register, left_register, left_register, right_register),
                    _ => (left_register, right_register, right_register, left_register),
                };
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: higher, s: sign_high, shift: 31 });
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: lower, s: sign_low, shift: 31 });
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: GENERAL_SCRATCH, a: subtrahend, b: minuend });
                self.output.instructions.push(Instruction::AddExtended { d, a: higher, b: lower });
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
            Expression::Conditional { when_true, when_false, .. } => {
                Ok(self.signedness_of(when_true)? && self.signedness_of(when_false)?)
            }
            Expression::Cast { target_type, .. } => Ok(self.signed_of(*target_type)),
        }
    }

    /// Emit a right shift, choosing arithmetic (signed) or logical (unsigned)
    /// from the type of the shifted value.
    fn emit_shift_right(&mut self, left: &Expression, right: &Expression, destination: u8) -> Compilation<()> {
        let signed = self.signedness_of(left)?;
        let d = destination;

        if let Expression::IntegerLiteral(amount) = right {
            if (1..=31).contains(amount) {
                // An unsigned narrow value fuses extension and shift into one
                // rlwinm; a signed narrow value extends (extsb/extsh) then shifts.
                if let Ok((register, width, leaf_signed)) = self.leaf_info(left) {
                    if width < 32 && !leaf_signed {
                        if self.emit_narrow_unsigned_shift(d, register, width, false, *amount as u8) {
                            return Ok(());
                        }
                        return Err(Diagnostic::error("narrow unsigned shift out of the single-rlwinm range (roadmap)"));
                    }
                }
                // The shifted value: a leaf stays put, a sub-expression goes to scratch.
                let Some(source) = self.place_operand(left, d, false)? else {
                    return Err(Diagnostic::error("shift value needs the full register allocator (roadmap M1)"));
                };
                let shift = *amount as u8;
                self.output.instructions.push(if signed {
                    Instruction::ShiftRightAlgebraicImmediate { a: d, s: source, shift }
                } else {
                    Instruction::ShiftRightLogicalImmediate { a: d, s: source, shift }
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
        if let Some(constant) = constant_value(right) {
            let (effective, value) = match operator {
                BinaryOperator::Subtract => (BinaryOperator::Add, -constant),
                other => (other, constant),
            };
            if self.emit_constant_form(effective, left, value, destination)? {
                return Ok(true);
            }
        }
        // constant op variable — only the commutative operators.
        if is_commutative(operator) {
            if let Some(constant) = constant_value(left) {
                if self.emit_constant_form(operator, right, constant, destination)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Apply `constant` to `variable` via the matching immediate instruction, if
    /// the constant qualifies. The operand is read from its own register (a leaf)
    /// or computed into `destination` (a sub-expression); the immediate then reads
    /// that source directly — `addi` must not take `r0` as its source, which would
    /// silently mean `li`.
    fn emit_constant_form(&mut self, operator: BinaryOperator, variable: &Expression, constant: i64, destination: u8) -> Compilation<bool> {
        // Identity and strength-reduction folds.
        match (operator, constant) {
            (BinaryOperator::Add, 0) => {
                self.evaluate_general(variable, destination)?;
                return Ok(true);
            }
            (BinaryOperator::Multiply, 0) => {
                self.load_integer_constant(destination, 0);
                return Ok(true);
            }
            (BinaryOperator::Multiply, 1) => {
                self.evaluate_general(variable, destination)?;
                return Ok(true);
            }
            (BinaryOperator::Multiply, -1) => {
                let Some(source) = self.place_operand(variable, destination, false)? else {
                    return Ok(false);
                };
                self.output.instructions.push(Instruction::Negate { d: destination, a: source });
                return Ok(true);
            }
            _ => {}
        }

        enum Immediate {
            Add,
            ShiftLeft(u8),
            Multiply,
            Or,
            Xor,
            Mask(u8, u8),
        }
        let kind = match operator {
            BinaryOperator::Add if fits_signed_16(constant) => Immediate::Add,
            BinaryOperator::Multiply if fits_signed_16(constant) => {
                if constant >= 2 && (constant as u64).is_power_of_two() {
                    Immediate::ShiftLeft(constant.trailing_zeros() as u8)
                } else {
                    Immediate::Multiply
                }
            }
            BinaryOperator::BitOr if fits_unsigned_16(constant) => Immediate::Or,
            BinaryOperator::BitXor if fits_unsigned_16(constant) => Immediate::Xor,
            BinaryOperator::BitAnd if contiguous_mask(constant).is_some() => {
                let (begin, end) = contiguous_mask(constant).unwrap();
                Immediate::Mask(begin, end)
            }
            BinaryOperator::ShiftLeft if (1..=31).contains(&constant) => Immediate::ShiftLeft(constant as u8),
            _ => return Ok(false),
        };

        // A narrow value times a power of two (or `<< n`): an unsigned narrow
        // operand fuses extension and shift into one rlwinm; a signed one extends
        // (extsb/extsh) then shifts via the normal path below.
        if let &Immediate::ShiftLeft(shift) = &kind {
            if let Ok((register, width, leaf_signed)) = self.leaf_info(variable) {
                if width < 32 && !leaf_signed {
                    return Ok(self.emit_narrow_unsigned_shift(destination, register, width, true, shift));
                }
            }
        }
        let prefer_destination = matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract);
        let Some(source) = self.place_operand(variable, destination, prefer_destination)? else {
            return Ok(false);
        };
        let d = destination;
        let instruction = match kind {
            Immediate::Add => Instruction::AddImmediate { d, a: source, immediate: constant as i16 },
            Immediate::ShiftLeft(shift) => Instruction::ShiftLeftImmediate { a: d, s: source, shift },
            Immediate::Multiply => Instruction::MultiplyImmediate { d, a: source, immediate: constant as i16 },
            Immediate::Or => Instruction::OrImmediate { a: d, s: source, immediate: constant as u16 },
            Immediate::Xor => Instruction::XorImmediate { a: d, s: source, immediate: constant as u16 },
            Immediate::Mask(begin, end) => Instruction::AndContiguousMask { a: d, s: source, begin, end },
        };
        self.output.instructions.push(instruction);
        Ok(true)
    }

    /// (register, width-bits, signed) for a general-register leaf variable.
    fn leaf_info(&self, expression: &Expression) -> Compilation<(u8, u8, bool)> {
        if let Expression::Variable(name) = expression {
            if let Some(location) = self.locations.get(name.as_str()) {
                if location.class == ValueClass::General {
                    return Ok((location.register, location.width, location.signed));
                }
            }
        }
        Err(Diagnostic::error("expected a general-register leaf"))
    }

    /// Place two leaf operands when at least one is narrow, emitting the width
    /// extensions mwcc inserts. A wide leaf stays in its home register; a narrow
    /// leaf is extended — into the scratch when it is the only operand needing
    /// materialization or the non-anchor of a two-narrow pair, in place (its home)
    /// when it is the pair's anchor. The anchor is the left operand for commutative
    /// operators and the right for subtraction, matching mwcc's evaluation order.
    fn place_narrow_leaves(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression) -> Compilation<Operands> {
        let (left_register, left_width, left_signed) = self.leaf_info(left)?;
        let (right_register, right_width, right_signed) = self.leaf_info(right)?;
        let left_narrow = left_width < 32;
        let right_narrow = right_width < 32;
        let subtract = operator == BinaryOperator::Subtract;

        // Where each operand ends up.
        let (left_target, right_target) = if left_narrow && right_narrow {
            if subtract {
                (GENERAL_SCRATCH, right_register) // right is the anchor, kept in place
            } else {
                (left_register, GENERAL_SCRATCH) // left is the anchor, kept in place
            }
        } else if left_narrow {
            (GENERAL_SCRATCH, right_register)
        } else {
            (left_register, GENERAL_SCRATCH)
        };

        // Emit extensions in mwcc's order: the anchor first, then the scratch operand.
        if subtract {
            if right_narrow { self.emit_widen(right_target, right_register, right_width, right_signed); }
            if left_narrow { self.emit_widen(left_target, left_register, left_width, left_signed); }
        } else {
            if left_narrow { self.emit_widen(left_target, left_register, left_width, left_signed); }
            if right_narrow { self.emit_widen(right_target, right_register, right_width, right_signed); }
        }
        Operands::ordered(left_target, right_target)
    }

    fn place_general_operands(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, _destination: u8) -> Compilation<Operands> {
        match (is_complex(left), is_complex(right)) {
            (false, false) => {
                if self.is_narrow_leaf(left) || self.is_narrow_leaf(right) {
                    return self.place_narrow_leaves(operator, left, right);
                }
                Operands::ordered(self.general_register_of_leaf(left)?, self.general_register_of_leaf(right)?)
            }
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
                // Compute the left side into a free temporary, keeping the right
                // side's inputs live; then the right side into the scratch.
                let temp = self.with_reserved_inputs(right, |generator| {
                    let temp = generator.lowest_free_general()?;
                    generator.evaluate_general(left, temp)?;
                    Ok(temp)
                })?;
                // The temporary holds the left result; keep it live while the right runs.
                let temp_added = self.reserved.insert(temp);
                self.evaluate_general(right, GENERAL_SCRATCH)?;
                if temp_added {
                    self.reserved.remove(&temp);
                }
                Operands::ordered(temp, GENERAL_SCRATCH)
            }
        }
    }

    /// Run `body` with the registers read by `expression` reserved, restoring the
    /// reservation set afterward.
    fn with_reserved_inputs<T>(&mut self, expression: &Expression, body: impl FnOnce(&mut Self) -> Compilation<T>) -> Compilation<T> {
        let registers = self.registers_used_by(expression);
        let newly_reserved: Vec<u8> = registers.iter().copied().filter(|register| self.reserved.insert(*register)).collect();
        let result = body(self);
        for register in &newly_reserved {
            self.reserved.remove(register);
        }
        result
    }

    /// The general registers read by variables in `expression`.
    fn registers_used_by(&self, expression: &Expression) -> HashSet<u8> {
        let mut registers = HashSet::new();
        self.collect_registers(expression, &mut registers);
        registers
    }
    fn collect_registers(&self, expression: &Expression, registers: &mut HashSet<u8>) {
        // Within a single expression all variables share a class, so we record
        // register numbers without filtering by class.
        match expression {
            Expression::Variable(name) => {
                if let Some(location) = self.locations.get(name) {
                    registers.insert(location.register);
                }
            }
            Expression::Binary { left, right, .. } => {
                self.collect_registers(left, registers);
                self.collect_registers(right, registers);
            }
            Expression::Unary { operand, .. } => self.collect_registers(operand, registers),
            Expression::Conditional { condition, when_true, when_false } => {
                self.collect_registers(condition, registers);
                self.collect_registers(when_true, registers);
                self.collect_registers(when_false, registers);
            }
            Expression::Cast { operand, .. } => self.collect_registers(operand, registers),
            _ => {}
        }
    }

    /// The lowest general register (r3..=r12) that is neither reserved nor the scratch.
    fn lowest_free_general(&self) -> Compilation<u8> {
        (3..=12)
            .find(|register| *register != GENERAL_SCRATCH && !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free registers (roadmap M1: spilling)"))
    }

    /// The lowest float register (f1..=f13) that is neither reserved nor the scratch.
    fn lowest_free_float(&self) -> Compilation<u8> {
        (1..=13)
            .find(|register| *register != FLOAT_SCRATCH && !self.reserved.contains(register))
            .ok_or_else(|| Diagnostic::error("out of free float registers (roadmap M1: spilling)"))
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

    fn place_float_operands(&mut self, left: &Expression, right: &Expression, _destination: u8) -> Compilation<Operands> {
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
