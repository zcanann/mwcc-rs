//! Computed-index and computed-value read/modify/write transactions.
//!
//! Keeping this owner separate from leaf/constant indexed updates makes the
//! address lifetime and cross-expression scheduling explicit: the computed
//! index must survive value formation, the load, and the matching store.

use super::*;

impl Generator {
    pub(crate) fn emit_computed_indexed_rmw(
        &mut self,
        pointee: Pointee,
        base: u8,
        index: &Expression,
        operator: BinaryOperator,
        right: &Expression,
    ) -> Compilation<()> {
        if self.try_emit_shifted_byte_half_index_update(
            pointee,
            base,
            index,
            operator,
            right,
        )? {
            return Ok(());
        }

        let index = self.materialize_index_operand(index)?;
        let scaled = if pointee.size() == 1 {
            index
        } else {
            let scaled = self.fresh_virtual_general_avoiding(vec![index]);
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: index,
                    shift: pointee.size().trailing_zeros() as u8,
                });
            scaled
        };

        self.evaluate_general(right, GENERAL_SCRATCH)?;
        let loaded = self.fresh_virtual_general();
        self.output
            .instructions
            .push(indexed_load(pointee, loaded, base, scaled)?);
        if pointee == Pointee::Char {
            self.emit_widen(loaded, loaded, 8, true);
        }
        self.output.instructions.push(combine_computed_update(
            operator,
            loaded,
            GENERAL_SCRATCH,
        ));
        self.output
            .instructions
            .push(indexed_store(pointee, GENERAL_SCRATCH, base, scaled)?);
        Ok(())
    }

    /// `bytes[signed_index / 2] |= narrow << amount` is a compact packing
    /// transaction used by ADPCM encoders. All measured generations keep the
    /// shifted contribution in r0 and reuse one computed quotient for the load
    /// and store, but their independent-operation schedule differs.
    fn try_emit_shifted_byte_half_index_update(
        &mut self,
        pointee: Pointee,
        base: u8,
        index: &Expression,
        operator: BinaryOperator,
        right: &Expression,
    ) -> Compilation<bool> {
        if pointee != Pointee::UnsignedChar || operator != BinaryOperator::BitOr {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::Divide,
            left: dividend,
            right: divisor,
        } = index
        else {
            return Ok(false);
        };
        if constant_value(divisor) != Some(2) || !self.signedness_of(dividend)? {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: shifted,
            right: amount,
        } = right
        else {
            return Ok(false);
        };
        let (Ok((dividend, 32, true)), Ok((shifted_register, 8, false))) =
            (self.leaf_info(dividend), self.leaf_info(shifted))
        else {
            return Ok(false);
        };
        let Some(amount) = self.plain_integer_leaf_register(amount) else {
            return Ok(false);
        };

        use mwcc_versions::ComputedByteIndexedRmwStyle::*;
        let style = self.behavior.computed_byte_indexed_rmw_style;
        let quotient = match style {
            LegacyCarryCorrected => {
                let quotient = self.fresh_virtual_general_avoiding(vec![dividend]);
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: quotient,
                        s: dividend,
                        shift: 1,
                    });
                self.output
                    .instructions
                    .push(Instruction::AddToZeroExtended {
                        d: quotient,
                        a: quotient,
                    });
                self.emit_widen(GENERAL_SCRATCH, shifted_register, 8, false);
                let loaded = self.fresh_virtual_general_preferring(dividend);
                self.output.instructions.push(indexed_load(
                    pointee,
                    loaded,
                    base,
                    quotient,
                )?);
                self.output.instructions.push(Instruction::ShiftLeftWord {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                    b: amount,
                });
                self.finish_shifted_byte_update(base, quotient, loaded)?;
                return Ok(true);
            }
            MainlinePromotedShift | LaterDirectParameterShift => {
                let sign = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(Instruction::ShiftRightLogicalImmediate {
                        a: sign,
                        s: dividend,
                        shift: 31,
                    });

                let direct_parameter_shift = style == LaterDirectParameterShift
                    && matches!(shifted.as_ref(), Expression::Variable(name)
                        if self.parameter_names.contains(name));
                if direct_parameter_shift {
                    self.output.instructions.push(Instruction::ShiftLeftWord {
                        a: GENERAL_SCRATCH,
                        s: shifted_register,
                        b: amount,
                    });
                } else if style == MainlinePromotedShift {
                    self.emit_widen(GENERAL_SCRATCH, shifted_register, 8, false);
                }

                let adjusted = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::Add {
                    d: adjusted,
                    a: sign,
                    b: dividend,
                });
                let quotient = self.fresh_virtual_general_preferring(shifted_register);
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: quotient,
                        s: adjusted,
                        shift: 1,
                    });

                if style == LaterDirectParameterShift && !direct_parameter_shift {
                    self.emit_widen(GENERAL_SCRATCH, shifted_register, 8, false);
                }
                if !direct_parameter_shift {
                    if style == LaterDirectParameterShift {
                        let loaded = self.fresh_virtual_general_preferring(dividend);
                        self.output.instructions.push(indexed_load(
                            pointee,
                            loaded,
                            base,
                            quotient,
                        )?);
                        self.output.instructions.push(Instruction::ShiftLeftWord {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                            b: amount,
                        });
                        self.finish_shifted_byte_update(base, quotient, loaded)?;
                        return Ok(true);
                    }
                    self.output.instructions.push(Instruction::ShiftLeftWord {
                        a: GENERAL_SCRATCH,
                        s: GENERAL_SCRATCH,
                        b: amount,
                    });
                }
                quotient
            }
        };

        let loaded = self.fresh_virtual_general_preferring(dividend);
        self.output
            .instructions
            .push(indexed_load(pointee, loaded, base, quotient)?);
        self.finish_shifted_byte_update(base, quotient, loaded)?;
        Ok(true)
    }

    fn finish_shifted_byte_update(
        &mut self,
        base: u8,
        index: u8,
        loaded: u8,
    ) -> Compilation<()> {
        self.output.instructions.push(Instruction::Or {
            a: GENERAL_SCRATCH,
            s: loaded,
            b: GENERAL_SCRATCH,
        });
        self.output.instructions.push(indexed_store(
            Pointee::UnsignedChar,
            GENERAL_SCRATCH,
            base,
            index,
        )?);
        Ok(())
    }
}

fn combine_computed_update(
    operator: BinaryOperator,
    loaded: u8,
    right: u8,
) -> Instruction {
    use BinaryOperator::*;
    match operator {
        Add => Instruction::Add {
            d: right,
            a: loaded,
            b: right,
        },
        Subtract => Instruction::SubtractFrom {
            d: right,
            a: right,
            b: loaded,
        },
        Multiply => Instruction::MultiplyLow {
            d: right,
            a: loaded,
            b: right,
        },
        BitAnd => Instruction::And {
            a: right,
            s: loaded,
            b: right,
        },
        BitOr => Instruction::Or {
            a: right,
            s: loaded,
            b: right,
        },
        BitXor => Instruction::Xor {
            a: right,
            s: loaded,
            b: right,
        },
        _ => unreachable!("indexed RMW operator was validated by the caller"),
    }
}
