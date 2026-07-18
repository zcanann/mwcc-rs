//! Narrow-integer width extension, fused narrow shifts, and return truncation.

use crate::analysis::*;
use crate::generator::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};

impl Generator {
    /// The natural width of a value loaded by dereferencing `pointer` — through a
    /// leaf pointer variable or a pointer-typed struct member (`*p->cq`).
    pub(crate) fn dereferenced_width(&self, pointer: &Expression) -> Option<u8> {
        // `*(T*)p` — a pointer cast: the dereferenced width is the cast's target pointee width, so a
        // narrow cast-deref return loads at its natural width (`*(short*)p` -> lha) without re-extending.
        if let Expression::Cast {
            target_type: Type::Pointer(pointee),
            ..
        } = pointer
        {
            return Some(pointee.element().width());
        }
        if let Some((_, _, Type::Pointer(pointee))) = as_member(pointer) {
            return Some(pointee.element().width());
        }
        // A file-scope global pointer.
        if let Expression::Variable(name) = pointer {
            if !self.locations.contains_key(name) {
                if let Some(Type::Pointer(pointee)) = self.globals.get(name) {
                    return Some(pointee.element().width());
                }
                // A file-scope global ARRAY subscripts to its element type — `globals` stores
                // the element type itself (there is no `Type::Array`), and `global_array_sizes`
                // records the extent, so `char a[8]; a[2]` resolves to the 8-bit element.
                if self.global_array_sizes.contains_key(name) {
                    if let Some(element) = self.globals.get(name) {
                        return Some(element.width());
                    }
                }
            }
        }
        // The address of an array member subscripts to its element.
        if let Expression::MemberAddress { element, .. } = pointer {
            return Some(element.element().width());
        }
        // `*(p + i)`: the dereferenced width is the pointer operand's pointee width (the
        // integer offset does not change the element type). `+` commutes, and this resolves a
        // global pointer operand (via the global-pointer arm above) as well as a local one.
        if let Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } = pointer
        {
            if let Some(width) = self.dereferenced_width(left) {
                return Some(width);
            }
            if let Some(width) = self.dereferenced_width(right) {
                return Some(width);
            }
        }
        self.pointee_of(pointer)
            .ok()
            .map(|pointee| pointee.element().width())
    }

    /// Whether the expression reads any narrow variable. A narrow return whose
    /// expression reads narrow operands relies on mwcc's optimization that elides
    /// operand extension because the result is truncated anyway — not yet modeled.
    pub(crate) fn contains_narrow_leaf(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(_) => self.is_narrow_leaf(expression),
            Expression::Binary { left, right, .. } => {
                self.contains_narrow_leaf(left) || self.contains_narrow_leaf(right)
            }
            Expression::Unary { operand, .. } => self.contains_narrow_leaf(operand),
            Expression::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                self.contains_narrow_leaf(condition)
                    || self.contains_narrow_leaf(when_true)
                    || self.contains_narrow_leaf(when_false)
            }
            Expression::Cast { operand, .. } => self.contains_narrow_leaf(operand),
            // A narrow memory load (member or dereference of a sub-word type) is
            // also a narrow operand for this purpose — computing with it into the
            // scratch then truncating is the same unmodeled optimization.
            Expression::Member { member_type, .. } => member_type.width() < 32,
            Expression::Dereference { pointer } => self
                .dereferenced_width(pointer)
                .is_some_and(|width| width < 32),
            Expression::Index { base, .. } => self
                .dereferenced_width(base)
                .is_some_and(|width| width < 32),
            _ => false,
        }
    }

    /// Coerce a returned value to a narrow return type. mwcc truncates a bare wide
    /// variable in place (`extsb`/`extsh`/`clrlwi r3,r3`) and computes a wider
    /// expression into the scratch before truncating it into the result
    /// (`addi r0,r3,1; extsb r3,r0`). The optimization that elides operand
    /// extension when the result is truncated is not modeled, so a computation
    /// reading a narrow operand is deferred rather than mis-extended.
    pub(crate) fn evaluate_narrow_return(
        &mut self,
        expression: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let width = return_type.width();
        let signed = self.signed_of(return_type);

        // A CONSTANT narrow return is truncated to the return type at COMPILE time and loaded
        // directly — mwcc emits `li r3, (type)const`, not a runtime `li r0,const; extsb r3,r0`.
        if let Some(constant) = crate::analysis::constant_value(expression) {
            let truncated = match (width, signed) {
                (8, true) => constant as i8 as i64,
                (8, false) => constant as u8 as i64,
                (16, true) => constant as i16 as i64,
                (16, false) => constant as u16 as i64,
                _ => constant,
            };
            self.load_integer_constant(result, truncated);
            return Ok(());
        }

        if let Expression::Variable(_) = expression {
            let (register, variable_width, _) = self.leaf_info(expression)?;
            if register != result {
                return Err(Diagnostic::error(
                    "narrow return of a non-result variable (roadmap M1)",
                ));
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

        // A narrow result truncates, so for an operator whose low bits depend only on
        // its operands' low bits — add/sub/and/or/xor/mul/shift-left — a narrow leaf
        // operand is read raw and the result re-truncated (mwcc: `addi r0,r3,5; clrlwi
        // r3,r0,24`). Scoped to `variable OP constant`, which the constant-form path
        // reads through place_operand (honoring the raw flag); a two-variable narrow
        // op still extends both operands and is left to the full optimization. The
        // operator restriction keeps the raw read off div/mod/shift-right.
        if let Expression::Binary {
            operator,
            left,
            right,
        } = expression
        {
            // BitAnd and ShiftLeft are excluded: mwcc folds their constant into a single
            // `rlwinm`/`clrlwi` that also performs the return-width truncation, so the
            // separate trailing widen this path emits would be redundant. The remaining
            // ops compute into a register and truncate separately, matching mwcc.
            let truncation_safe = matches!(
                operator,
                BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor
                    | BinaryOperator::Multiply
            );
            let var_op_constant = matches!(left.as_ref(), Expression::Variable(_))
                && matches!(right.as_ref(), Expression::IntegerLiteral(_));
            if truncation_safe && var_op_constant {
                self.narrow_truncation_context = true;
                let evaluated = self.evaluate_general(expression, GENERAL_SCRATCH);
                self.narrow_truncation_context = false;
                evaluated?;
                self.emit_widen(result, GENERAL_SCRATCH, width, signed);
                return Ok(());
            }
        }
        // Other narrow-operand expressions still need the full truncation-propagation
        // optimization (un-extended reads through nested truncation-safe operators);
        // defer rather than emit redundant leading extensions.
        if self.contains_narrow_leaf(expression) {
            return Err(Diagnostic::error("narrow return of a narrow-operand expression needs the truncation-context optimization (roadmap)"));
        }
        if needs_scratch(expression) {
            return Err(Diagnostic::error(
                "narrow return of a scratch-needing expression (roadmap M1)",
            ));
        }
        self.evaluate_general(expression, GENERAL_SCRATCH)?;
        self.emit_widen(result, GENERAL_SCRATCH, width, signed);
        Ok(())
    }

    /// Move/extend a value of `width` bits from `source` into `destination`,
    /// sign- or zero-extending narrow values to 32 bits.
    pub(crate) fn emit_widen(&mut self, destination: u8, source: u8, width: u8, signed: bool) {
        match (width, signed) {
            (8, true) => self.output.instructions.push(Instruction::ExtendSignByte {
                a: destination,
                s: source,
            }),
            (16, true) => self
                .output
                .instructions
                .push(Instruction::ExtendSignHalfword {
                    a: destination,
                    s: source,
                }),
            (8, false) => self
                .output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: destination,
                    s: source,
                    clear: 24,
                }),
            (16, false) => self
                .output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: destination,
                    s: source,
                    clear: 16,
                }),
            _ if source != destination => self
                .output
                .instructions
                .push(Instruction::move_register(destination, source)),
            _ => {}
        }
    }

    /// Like emit_widen but with the record form (`extsh.`/`extsb.`/`clrlwi.`), so the
    /// extension also sets cr0 — mwcc's one-instruction test of a narrow value against
    /// zero (`if (s < 0)` -> `extsh. r0,rS; bge`).
    pub(crate) fn emit_widen_record(
        &mut self,
        destination: u8,
        source: u8,
        width: u8,
        signed: bool,
    ) {
        let instruction = match (width, signed) {
            (8, true) => Instruction::ExtendSignByteRecord {
                a: destination,
                s: source,
            },
            (16, true) => Instruction::ExtendSignHalfwordRecord {
                a: destination,
                s: source,
            },
            (8, false) => Instruction::ClearLeftImmediateRecord {
                a: destination,
                s: source,
                clear: 24,
            },
            _ => Instruction::ClearLeftImmediateRecord {
                a: destination,
                s: source,
                clear: 16,
            },
        };
        self.output.instructions.push(instruction);
    }

    /// Emit mwcc's fused `rlwinm` for an unsigned narrow value shifted by a
    /// constant. A `width`-bit value occupies the low `width` bits, starting at
    /// big-endian bit `32-width`. `<< n` rotates left n and keeps the shifted
    /// window; `>> n` rotates by `32-n`. Returns false when the shift would push
    /// significant bits out of the single-rlwinm range (deferred, not modeled).
    pub(crate) fn emit_narrow_unsigned_shift(
        &mut self,
        destination: u8,
        source: u8,
        width: u8,
        left: bool,
        amount: u8,
    ) -> bool {
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
