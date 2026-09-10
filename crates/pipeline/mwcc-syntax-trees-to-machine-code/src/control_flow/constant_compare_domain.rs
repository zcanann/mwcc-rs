//! Compare constants at the signed/unsigned immediate boundary.
//!
//! Narrow operands are promoted before comparing. Unsigned narrow provenance
//! still selects cmplwi for nonnegative 16-bit constants, while a signed
//! negative constant uses the promoted signed domain. A negative word compared
//! in the unsigned domain must never be truncated to a 16-bit immediate.

use super::*;

impl Generator {
    pub(super) fn try_emit_boundary_constant_compare(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
    ) -> Compilation<Option<(u8, u8)>> {
        let Some(constant) = constant_value(right) else {
            return Ok(None);
        };
        if i32::try_from(constant).is_err() && u32::try_from(constant).is_err() {
            return Ok(None);
        }
        let Some(width @ (8 | 16 | 32)) = self.unpromoted_integer_width(left) else {
            return Ok(None);
        };
        let source_signed = self.signedness_of(left)?;
        let boundary = if width < 32 {
            i16::try_from(constant).is_err() || (constant < 0 && !source_signed)
        } else {
            !source_signed
                && ((constant < 0 && i16::try_from(constant).is_ok())
                    || (32768..=65535).contains(&constant))
        };
        if !boundary {
            return Ok(None);
        }

        let signed = if !source_signed && u16::try_from(constant).is_ok() {
            false
        } else {
            self.usual_integer_binary_signedness(left, right)?
        };
        let immediate = if signed {
            i16::try_from(constant).ok()
        } else {
            u16::try_from(constant).ok().map(|value| value as i16)
        };
        if let Some(immediate) = immediate {
            let mut source = self.condition_operand_register(left)?;
            if let Ok((home, narrow_width, narrow_signed)) = self.leaf_info(left) {
                if home == source && narrow_width < 32 {
                    self.emit_widen(GENERAL_SCRATCH, source, narrow_width, narrow_signed);
                    source = GENERAL_SCRATCH;
                }
            } else if width < 32
                && matches!(
                    left,
                    Expression::Call { .. } | Expression::VirtualCall { .. }
                )
            {
                self.emit_widen(GENERAL_SCRATCH, source, width, source_signed);
                source = GENERAL_SCRATCH;
            } else if self.is_signed_byte_load(left)? {
                self.emit_widen(source, source, 8, true);
            }
            self.output.instructions.push(if signed {
                Instruction::CompareWordImmediate {
                    a: source,
                    immediate,
                }
            } else {
                Instruction::CompareLogicalWordImmediate {
                    a: source,
                    immediate: immediate as u16,
                }
            });
        } else {
            // The constant or the biased equality result occupies r0. Keep the
            // promoted value in a real GPR, including memory/call operands.
            let source = if let Ok((home, narrow_width, narrow_signed)) = self.leaf_info(left) {
                if home == GENERAL_SCRATCH {
                    let register = self.fresh_virtual_general();
                    self.emit_widen(register, home, narrow_width, narrow_signed);
                    register
                } else {
                    if narrow_width < 32 {
                        self.emit_widen(home, home, narrow_width, narrow_signed);
                    }
                    home
                }
            } else {
                let register = self.fresh_virtual_general();
                self.evaluate_promoted_general_operand(left, register)?;
                if width < 32
                    && matches!(
                        left,
                        Expression::Call { .. } | Expression::VirtualCall { .. }
                    )
                {
                    self.emit_widen(register, register, width, source_signed);
                }
                register
            };
            if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
                self.output
                    .instructions
                    .push(Instruction::AddImmediateShifted {
                        d: GENERAL_SCRATCH,
                        a: source,
                        immediate: ((constant as u32 >> 16) as i16).wrapping_neg(),
                    });
                self.output
                    .instructions
                    .push(Instruction::CompareLogicalWordImmediate {
                        a: GENERAL_SCRATCH,
                        immediate: constant as u16,
                    });
            } else {
                self.load_integer_constant(GENERAL_SCRATCH, constant);
                self.output.instructions.push(if signed {
                    Instruction::CompareWord {
                        a: source,
                        b: GENERAL_SCRATCH,
                    }
                } else {
                    Instruction::CompareLogicalWord {
                        a: source,
                        b: GENERAL_SCRATCH,
                    }
                });
            }
        }
        Ok(Some(
            false_branch_bo_bi(operator).expect("comparison operator"),
        ))
    }
}
