//! Narrow-integer width extension, fused narrow shifts, and return truncation.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{Expression, Type};
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    /// The natural width of a value loaded by dereferencing `pointer` — through a
    /// leaf pointer variable or a pointer-typed struct member (`*p->cq`).
    fn dereferenced_width(&self, pointer: &Expression) -> Option<u8> {
        if let Some((_, _, Type::Pointer(pointee))) = as_member(pointer) {
            return Some(pointee.element().width());
        }
        self.pointee_of(pointer).ok().map(|pointee| pointee.element().width())
    }

    /// Whether the expression reads any narrow variable. A narrow return whose
    /// expression reads narrow operands relies on mwcc's optimization that elides
    /// operand extension because the result is truncated anyway — not yet modeled.
    pub(crate) fn contains_narrow_leaf(&self, expression: &Expression) -> bool {
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
    pub(crate) fn evaluate_narrow_return(&mut self, expression: &Expression, return_type: Type, result: u8) -> Compilation<()> {
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

        // A load already yields the value's natural width; when it matches the
        // return type no truncation is needed — load straight into the result.
        let load_width = match expression {
            Expression::Dereference { pointer } => self.dereferenced_width(pointer),
            Expression::Index { base, .. } => self.dereferenced_width(base),
            Expression::Member { member_type, .. } => Some(member_type.width()),
            _ => None,
        };
        if load_width == Some(width) {
            return self.evaluate_general(expression, result);
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

    /// Move/extend a value of `width` bits from `source` into `destination`,
    /// sign- or zero-extending narrow values to 32 bits.
    pub(crate) fn emit_widen(&mut self, destination: u8, source: u8, width: u8, signed: bool) {
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
    pub(crate) fn emit_narrow_unsigned_shift(&mut self, destination: u8, source: u8, width: u8, left: bool, amount: u8) -> bool {
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
}
