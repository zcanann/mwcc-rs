//! Core integer expression evaluation and operand placement.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, Type, UnaryOperator};
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

/// The displacement load for a pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`).
fn displacement_load(pointee: Pointee, d: u8, a: u8, offset: i16) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::LoadWord { d, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZero { d, a, offset },
        Pointee::Short => Instruction::LoadHalfwordAlgebraic { d, a, offset },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZero { d, a, offset },
        Pointee::Float => Instruction::LoadFloatSingle { d, a, offset },
    }
}

/// The indexed load for a pointee type (`lwzx`/`lbzx`/`lhax`/`lhzx`/`lfsx`).
fn indexed_load(pointee: Pointee, d: u8, a: u8, b: u8) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::LoadWordIndexed { d, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZeroIndexed { d, a, b },
        Pointee::Short => Instruction::LoadHalfwordAlgebraicIndexed { d, a, b },
        Pointee::UnsignedShort => Instruction::LoadHalfwordZeroIndexed { d, a, b },
        Pointee::Float => Instruction::LoadFloatSingleIndexed { d, a, b },
    }
}

/// The displacement store for a pointee type (`stw`/`stb`/`sth`/`stfs`).
fn displacement_store(pointee: Pointee, s: u8, a: u8, offset: i16) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::StoreWord { s, a, offset },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByte { s, a, offset },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfword { s, a, offset },
        Pointee::Float => Instruction::StoreFloatSingle { s, a, offset },
    }
}

/// The indexed store for a pointee type (`stwx`/`stbx`/`sthx`/`stfsx`).
fn indexed_store(pointee: Pointee, s: u8, a: u8, b: u8) -> Instruction {
    match pointee {
        Pointee::Int | Pointee::UnsignedInt => Instruction::StoreWordIndexed { s, a, b },
        Pointee::Char | Pointee::UnsignedChar => Instruction::StoreByteIndexed { s, a, b },
        Pointee::Short | Pointee::UnsignedShort => Instruction::StoreHalfwordIndexed { s, a, b },
        Pointee::Float => Instruction::StoreFloatSingleIndexed { s, a, b },
    }
}

impl Generator {

    /// Evaluate an integer expression into general register `destination`.
    pub(crate) fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
        match expression {
            Expression::IntegerLiteral(value) => {
                self.load_integer_constant(destination, *value);
                Ok(())
            }
            Expression::Variable(name) => {
                if let Some(location) = self.locations.get(name) {
                    if location.class != ValueClass::General {
                        return Err(Diagnostic::error(format!("'{name}' is not an integer")));
                    }
                    let (source, width, signed) = (location.register, location.width, location.signed);
                    self.emit_widen(destination, source, width, signed);
                    Ok(())
                } else {
                    self.emit_global_load(name, destination)
                }
            }
            Expression::Unary { operator, operand } => self.emit_unary(*operator, operand, destination),
            Expression::Conditional { condition, when_true, when_false } => {
                self.emit_conditional(condition, when_true, when_false, destination, false)
            }
            Expression::Cast { target_type, operand } => self.emit_cast_to_integer(*target_type, operand, destination),
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
            Expression::Index { base, index } => self.emit_subscript(base, index, destination),
            Expression::Call { name, arguments } => self.emit_call(name, arguments, Some(destination), false),
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

    /// Place an operand and return the register holding it. A leaf stays in its
    /// own register. A sub-expression is computed into the destination when the
    /// consumer can fold it there (`addi`), otherwise into the scratch register —
    /// mwcc keeps `addi` operands in place but routes `rlwinm`/logical operands
    /// through `r0`. Returns `None` when a scratch operand does not fit.
    /// Emit `*pointer` — load the pointed-to value into `destination`, choosing
    /// the load by the pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`). The pointer
    /// must be a leaf variable holding the address; richer addressing is on the
    /// roadmap.
    pub(crate) fn emit_load_from_pointer(&mut self, pointer: &Expression, destination: u8) -> Compilation<()> {
        let (pointee, address) = self.pointer_leaf(pointer)?;
        self.output.instructions.push(displacement_load(pointee, destination, address, 0));
        Ok(())
    }

    /// Emit `base[index]` into `destination`. A constant index folds into the load
    /// displacement (`lwz r3,8(r3)`); a variable index is scaled by the element
    /// size and uses an indexed load (`slwi r0,rI,2; lwzx r3,rBase,r0`).
    pub(crate) fn emit_subscript(&mut self, base: &Expression, index: &Expression, destination: u8) -> Compilation<()> {
        let (pointee, address) = self.pointer_leaf(base)?;
        if let Some(constant) = constant_value(index) {
            let offset = constant * pointee.size() as i64;
            let offset = i16::try_from(offset).map_err(|_| Diagnostic::error("subscript offset out of range (roadmap)"))?;
            self.output.instructions.push(displacement_load(pointee, destination, address, offset));
            return Ok(());
        }
        let index_register = self.general_register_of_leaf(index)?;
        let size = pointee.size();
        let scaled = if size == 1 {
            index_register
        } else {
            self.output.instructions.push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: index_register,
                shift: size.trailing_zeros() as u8,
            });
            GENERAL_SCRATCH
        };
        self.output.instructions.push(indexed_load(pointee, destination, address, scaled));
        Ok(())
    }

    /// Emit a store: `*p = v;` or `p[i] = v;`. The value goes to memory at the
    /// place addressed by the pointer (with a folded displacement for a constant
    /// index, or a scaled indexed store for a variable one).
    pub(crate) fn emit_store(&mut self, target: &Expression, value: &Expression) -> Compilation<()> {
        let (base, index) = match target {
            Expression::Dereference { pointer } => (pointer.as_ref(), None),
            Expression::Index { base, index } => (base.as_ref(), Some(index.as_ref())),
            _ => return Err(Diagnostic::error("store target must be `*p` or `p[i]`")),
        };
        let (pointee, address) = self.pointer_leaf(base)?;
        match index {
            None => {
                let source = self.place_store_value(value, pointee)?;
                self.output.instructions.push(displacement_store(pointee, source, address, 0));
            }
            Some(index) if constant_value(index).is_some() => {
                let offset = i16::try_from(constant_value(index).unwrap() * pointee.size() as i64)
                    .map_err(|_| Diagnostic::error("store offset out of range (roadmap)"))?;
                let source = self.place_store_value(value, pointee)?;
                self.output.instructions.push(displacement_store(pointee, source, address, offset));
            }
            Some(index) => {
                // A variable index uses the scratch for scaling, so the value must
                // be a leaf (it stays in its own register).
                if !matches!(value, Expression::Variable(_)) {
                    return Err(Diagnostic::error("store with a variable index needs a simple value (roadmap)"));
                }
                let source = self.place_store_value(value, pointee)?;
                let index_register = self.general_register_of_leaf(index)?;
                let size = pointee.size();
                let scaled = if size == 1 {
                    index_register
                } else {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: index_register,
                        shift: size.trailing_zeros() as u8,
                    });
                    GENERAL_SCRATCH
                };
                self.output.instructions.push(indexed_store(pointee, source, address, scaled));
            }
        }
        Ok(())
    }

    /// The register holding the value to store: a leaf stays in its own register,
    /// anything else is computed into the scratch (`li r0,0; stw r0,…`,
    /// `add r0,…; stw r0,…`) ahead of the store.
    fn place_store_value(&mut self, value: &Expression, pointee: Pointee) -> Compilation<u8> {
        if pointee == Pointee::Float {
            if matches!(value, Expression::Variable(_)) {
                return self.float_register_of_leaf(value);
            }
            self.evaluate_float(value, FLOAT_SCRATCH)?;
            return Ok(FLOAT_SCRATCH);
        }
        if matches!(value, Expression::Variable(_)) {
            return self.general_register_of_leaf(value);
        }
        self.evaluate_general(value, GENERAL_SCRATCH)?;
        Ok(GENERAL_SCRATCH)
    }

    /// Emit a direct call. Arguments are placed in the EABI argument registers,
    /// then `bl name`; the result (in r3 / f1) is moved to `destination` when one
    /// is wanted (a discarded call statement passes `None`).
    pub(crate) fn emit_call(&mut self, name: &str, arguments: &[Expression], destination: Option<u8>, float_result: bool) -> Compilation<()> {
        self.emit_arguments(arguments)?;
        self.output.instructions.push(Instruction::BranchAndLink { target: name.to_string() });
        if let Some(destination) = destination {
            let result = if float_result { Eabi::float_result().number } else { Eabi::general_result().number };
            if destination != result {
                self.output.instructions.push(if float_result {
                    Instruction::FloatMove { d: destination, b: result }
                } else {
                    Instruction::move_register(destination, result)
                });
            }
        }
        Ok(())
    }

    /// Place call arguments in the EABI argument registers (r3.. / f1..). Each is
    /// evaluated into its positional register; passthrough parameters are already
    /// in place, so this is a no-op for them.
    fn emit_arguments(&mut self, arguments: &[Expression]) -> Compilation<()> {
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for argument in arguments {
            if self.is_float_value(argument) {
                self.evaluate_float(argument, next_float)?;
                next_float += 1;
            } else {
                self.evaluate_general(argument, next_general)?;
                next_general += 1;
            }
        }
        Ok(())
    }

    /// Whether an expression yields a float (a float leaf, literal, or load).
    fn is_float_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::FloatLiteral(_) => true,
            Expression::Variable(_) => self.is_float_leaf(expression),
            Expression::Dereference { pointer } => matches!(self.pointee_of(pointer), Ok(Pointee::Float)),
            Expression::Index { base, .. } => matches!(self.pointee_of(base), Ok(Pointee::Float)),
            _ => false,
        }
    }

    /// Load a file-scope global into `destination`. The instruction carries the
    /// `0(r0)` placeholder that an `R_PPC_EMB_SDA21` relocation fills (r13 + the
    /// small-data offset); the load is chosen by the global's type.
    pub(crate) fn emit_global_load(&mut self, name: &str, destination: u8) -> Compilation<()> {
        let global_type = *self.globals.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        let instruction = match global_type {
            Type::Int | Type::UnsignedInt => Instruction::LoadWord { d: destination, a: 0, offset: 0 },
            Type::Char | Type::UnsignedChar => Instruction::LoadByteZero { d: destination, a: 0, offset: 0 },
            Type::Short => Instruction::LoadHalfwordAlgebraic { d: destination, a: 0, offset: 0 },
            Type::UnsignedShort => Instruction::LoadHalfwordZero { d: destination, a: 0, offset: 0 },
            Type::Float => Instruction::LoadFloatSingle { d: destination, a: 0, offset: 0 },
            other => return Err(Diagnostic::error(format!("global of type {other:?} is not supported yet"))),
        };
        self.output.instructions.push(instruction);
        Ok(())
    }

    /// `(pointee, address register)` for a pointer leaf variable.
    fn pointer_leaf(&self, base: &Expression) -> Compilation<(Pointee, u8)> {
        let name = leaf_name(base).ok_or_else(|| Diagnostic::error("pointer access needs a pointer variable (roadmap)"))?;
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        let pointee = location.pointee.ok_or_else(|| Diagnostic::error(format!("'{name}' is not a pointer")))?;
        Ok((pointee, location.register))
    }

    pub(crate) fn place_operand(&mut self, operand: &Expression, destination: u8, prefer_destination: bool) -> Compilation<Option<u8>> {
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
    pub(crate) fn emit_unary(&mut self, operator: UnaryOperator, operand: &Expression, destination: u8) -> Compilation<()> {
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
}
