//! Core integer expression evaluation and operand placement.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Pointee, UnaryOperator};
use crate::analysis::*;
use crate::generator::*;
use crate::operands::*;

impl Generator {

    /// Evaluate an integer expression into general register `destination`.
    pub(crate) fn evaluate_general(&mut self, expression: &Expression, destination: u8) -> Compilation<()> {
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
            Expression::Dereference { pointer } => self.emit_load_from_pointer(pointer, destination),
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
        let name = leaf_name(pointer).ok_or_else(|| Diagnostic::error("dereference needs a pointer variable (roadmap)"))?;
        let location = self.locations.get(name).ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        let pointee = location.pointee.ok_or_else(|| Diagnostic::error(format!("'{name}' is not a pointer")))?;
        let address = location.register;
        let offset = 0;
        let instruction = match pointee {
            Pointee::Int | Pointee::UnsignedInt => Instruction::LoadWord { d: destination, a: address, offset },
            Pointee::Char | Pointee::UnsignedChar => Instruction::LoadByteZero { d: destination, a: address, offset },
            Pointee::Short => Instruction::LoadHalfwordAlgebraic { d: destination, a: address, offset },
            Pointee::UnsignedShort => Instruction::LoadHalfwordZero { d: destination, a: address, offset },
            Pointee::Float => Instruction::LoadFloatSingle { d: destination, a: address, offset },
        };
        self.output.instructions.push(instruction);
        Ok(())
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
