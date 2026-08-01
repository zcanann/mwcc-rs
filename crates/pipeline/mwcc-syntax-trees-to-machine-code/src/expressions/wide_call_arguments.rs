//! EABI marshaling for integer register-pair call arguments.
//!
//! A 64-bit integer begins in an odd-numbered GPR and occupies a big-endian
//! high:low pair. This module owns that placement independently from ordinary
//! one-word call argument evaluation.

use super::*;

fn fixed_clock_scale(expression: &Expression) -> Option<i16> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let Expression::IntegerLiteral(scale) = left.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: divided_clock,
        right: milliseconds,
    } = right.as_ref()
    else {
        return None;
    };
    let Expression::IntegerLiteral(1000) = milliseconds.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: clock,
        right: quarter,
    } = divided_clock.as_ref()
    else {
        return None;
    };
    let Expression::IntegerLiteral(4) = quarter.as_ref() else {
        return None;
    };
    let Expression::Dereference { pointer } = clock.as_ref() else {
        return None;
    };
    let Expression::Cast {
        target_type: Type::Pointer(Pointee::UnsignedInt),
        operand,
    } = pointer.as_ref()
    else {
        return None;
    };
    if !matches!(operand.as_ref(), Expression::IntegerLiteral(_)) {
        return None;
    }
    i16::try_from(*scale).ok()
}

impl Generator {
    /// Load one native 64-bit lvalue into an aligned EABI GPR pair.
    ///
    /// PowerPC is big-endian, so the high word occupies displacement zero and
    /// the low word displacement four. Materialize the address in the high
    /// destination, load the low word first, then replace the address with the
    /// high word. This needs no extra scratch register and keeps earlier call
    /// arguments reserved.
    fn try_emit_native_wide_call_argument(
        &mut self,
        argument: &Expression,
        high: u8,
        low: u8,
    ) -> Compilation<bool> {
        let Expression::Dereference { pointer } = argument else {
            return Ok(false);
        };
        if !matches!(
            pointer.as_ref(),
            Expression::Cast {
                target_type: Type::Pointer(Pointee::LongLong | Pointee::UnsignedLongLong),
                ..
            }
        ) {
            return Ok(false);
        }
        self.evaluate_general(pointer, high)?;
        self.output.instructions.push(Instruction::LoadWord {
            d: low,
            a: high,
            offset: 4,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: high,
            a: high,
            offset: 0,
        });
        Ok(true)
    }

    /// Schedule `(anchored address, widened fixed-clock ticks, callback)`.
    ///
    /// The first address is deliberately reloadable: MWCC borrows r3 while
    /// reducing the fixed clock, starts the callback address in the resulting
    /// load latency slot, then publishes the first argument immediately before
    /// zero-extending the 32-bit tick value into r5:r6.
    pub(crate) fn try_emit_fixed_clock_wide_callback_arguments(
        &mut self,
        arguments: &[Expression],
        name: &str,
    ) -> Compilation<bool> {
        let [first @ Expression::AddressOf { operand }, ticks, Expression::AddressOf {
            operand: callback_operand,
        }] = arguments
        else {
            return Ok(false);
        };
        let Expression::Variable(addressed) = operand.as_ref() else {
            return Ok(false);
        };
        let Expression::Variable(callback) = callback_operand.as_ref() else {
            return Ok(false);
        };
        let Some(parameter_types) = self.call_parameter_types.get(name) else {
            return Ok(false);
        };
        let anchored = self
            .data_section_anchor
            .as_ref()
            .is_some_and(|anchor| anchor.register.is_some() && anchor.symbols.contains(addressed));
        if !self.behavior.schedule_latency_slots
            || self.globals.contains_key(name)
            || self.locations.contains_key(name)
            || parameter_types.len() < 3
            || !matches!(parameter_types[1], Type::LongLong | Type::UnsignedLongLong)
            || !anchored
            || !self.call_return_types.contains_key(callback)
            || self.globals.contains_key(callback)
            || self.locations.contains_key(callback)
            || fixed_clock_scale(ticks).is_none()
            || self.signedness_of(ticks)?
        {
            return Ok(false);
        }

        let start = self.output.instructions.len();
        self.evaluate_general(ticks, Eabi::FIRST_GENERAL_ARGUMENT + 3)?;
        let wide_end = self.output.instructions.len();
        let raw_shape = matches!(
            self.output.instructions.get(start..wide_end),
            Some([
                Instruction::AddImmediateShifted {
                    d: clock_high,
                    a: 0,
                    ..
                },
                Instruction::LoadWord {
                    d: 0,
                    a: clock_base,
                    ..
                },
                Instruction::ShiftRightLogicalImmediate {
                    a: quarter,
                    s: 0,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: magic_high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: 0,
                    a: magic_base,
                    ..
                },
                Instruction::MultiplyHighWordUnsigned {
                    d: 0,
                    a: 0,
                    b: divided,
                },
                Instruction::ShiftRightLogicalImmediate { a: 0, s: 0, .. },
                Instruction::MultiplyImmediate { d: 6, a: 0, .. },
            ]) if wide_end == start + 8
                && clock_high == clock_base
                && magic_high == magic_base
                && quarter == divided
        );
        if !raw_shape {
            return Err(Diagnostic::error(format!(
                "the fixed-clock wide callback schedule changed shape: {:?}",
                &self.output.instructions[start..wide_end],
            )));
        }

        self.emit_address_high(Eabi::FIRST_GENERAL_ARGUMENT, callback);
        self.record_relocation(RelocationKind::Addr16Lo, callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: Eabi::FIRST_GENERAL_ARGUMENT + 4,
            a: Eabi::FIRST_GENERAL_ARGUMENT,
            immediate: 0,
        });
        self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT + 2, 0);

        // Alternate the constant high half with the fixed load, then use the
        // callback high half as the next independent instruction.
        crate::move_instruction_before_retargeting(self, start + 3, start + 2);
        crate::move_instruction_before_retargeting(self, start + 8, start + 3);

        self.output.instructions[start] = match self.output.instructions[start] {
            Instruction::AddImmediateShifted { immediate, .. } => {
                Instruction::AddImmediateShifted {
                    d: Eabi::FIRST_GENERAL_ARGUMENT,
                    a: 0,
                    immediate,
                }
            }
            _ => unreachable!("the fixed-clock address high half was verified"),
        };
        self.output.instructions[start + 1] = match self.output.instructions[start + 1] {
            Instruction::LoadWord { d, offset, .. } => Instruction::LoadWord {
                d,
                a: Eabi::FIRST_GENERAL_ARGUMENT,
                offset,
            },
            _ => unreachable!("the fixed-clock load was verified"),
        };
        self.output.instructions[start + 2] = match self.output.instructions[start + 2] {
            Instruction::AddImmediateShifted { immediate, .. } => {
                Instruction::AddImmediateShifted {
                    d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    a: 0,
                    immediate,
                }
            }
            _ => unreachable!("the fixed-clock high half was verified"),
        };
        self.output.instructions[start + 4] = match self.output.instructions[start + 4] {
            Instruction::ShiftRightLogicalImmediate { shift, .. } => {
                Instruction::ShiftRightLogicalImmediate {
                    a: GENERAL_SCRATCH,
                    s: GENERAL_SCRATCH,
                    shift,
                }
            }
            _ => unreachable!("the fixed-clock pre-shift was verified"),
        };
        self.output.instructions[start + 5] = match self.output.instructions[start + 5] {
            Instruction::AddImmediate { immediate, .. } => Instruction::AddImmediate {
                d: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                a: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                immediate,
            },
            _ => unreachable!("the fixed-clock magic low half was verified"),
        };
        self.output.instructions[start + 6] = Instruction::MultiplyHighWordUnsigned {
            d: GENERAL_SCRATCH,
            a: Eabi::FIRST_GENERAL_ARGUMENT + 1,
            b: GENERAL_SCRATCH,
        };
        Ok(true)
    }

    /// Widen a scalar integer actual into a 64-bit formal's aligned GPR pair.
    ///
    /// Full 64-bit expression evaluation and pair overflow to the outgoing
    /// parameter area remain separate features. Reject those shapes rather than
    /// silently passing only one word.
    pub(crate) fn emit_widened_general_call_argument(
        &mut self,
        argument: &Expression,
        parameter_type: Type,
        mut next_general: u8,
    ) -> Compilation<u8> {
        debug_assert!(matches!(
            parameter_type,
            Type::LongLong | Type::UnsignedLongLong
        ));
        if self.is_float_value(argument) {
            return Err(Diagnostic::error(
                "floating-to-64-bit call argument conversion is not supported yet (roadmap)",
            ));
        }
        let native_wide = self
            .unpromoted_integer_width(argument)
            .is_some_and(|width| width > 32);
        if next_general % 2 == 0 {
            next_general += 1;
        }
        let high = next_general;
        let low = high
            .checked_add(1)
            .ok_or_else(|| Diagnostic::error("a wide call argument register pair overflowed"))?;
        if low > Eabi::LAST_GENERAL_ARGUMENT {
            return Err(Diagnostic::error(
                "a wide call argument needs an outgoing stack pair (roadmap)",
            ));
        }

        // Earlier ABI arguments remain live while the low word is evaluated.
        // Reserve them so an expression temporary cannot silently overwrite
        // one before the call.
        let newly_reserved: Vec<_> = (Eabi::FIRST_GENERAL_ARGUMENT..high)
            .filter(|register| self.reserved.insert(*register))
            .collect();
        if native_wide {
            let emitted = self.try_emit_native_wide_call_argument(argument, high, low);
            for register in newly_reserved {
                self.reserved.remove(&register);
            }
            if emitted? {
                return low
                    .checked_add(1)
                    .ok_or_else(|| Diagnostic::error("a wide call argument pair overflowed"));
            }
            return Err(Diagnostic::error(
                "a native 64-bit call argument needs pair evaluation (roadmap)",
            ));
        }
        let evaluated = self.evaluate_general(argument, low);
        for register in newly_reserved {
            self.reserved.remove(&register);
        }
        evaluated?;

        if self.signedness_of(argument)? {
            self.output
                .instructions
                .push(Instruction::ShiftRightAlgebraicImmediate {
                    a: high,
                    s: low,
                    shift: 31,
                });
        } else {
            self.load_integer_constant(high, 0);
        }
        Ok(low + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_clock_ticks(scale: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(Expression::IntegerLiteral(scale)),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::Divide,
                left: Box::new(Expression::Binary {
                    operator: BinaryOperator::Divide,
                    left: Box::new(Expression::Dereference {
                        pointer: Box::new(Expression::Cast {
                            target_type: Type::Pointer(Pointee::UnsignedInt),
                            operand: Box::new(Expression::IntegerLiteral(0x8000_00f8)),
                        }),
                    }),
                    right: Box::new(Expression::IntegerLiteral(4)),
                }),
                right: Box::new(Expression::IntegerLiteral(1000)),
            }),
        }
    }

    #[test]
    fn recognizes_scaled_millisecond_ticks_from_a_fixed_unsigned_clock() {
        assert_eq!(fixed_clock_scale(&fixed_clock_ticks(1150)), Some(1150));
    }

    #[test]
    fn rejects_a_different_clock_reduction_shape() {
        let mut ticks = fixed_clock_ticks(1150);
        let Expression::Binary { right, .. } = &mut ticks else {
            unreachable!()
        };
        let Expression::Binary {
            right: milliseconds,
            ..
        } = right.as_mut()
        else {
            unreachable!()
        };
        *milliseconds = Box::new(Expression::IntegerLiteral(1024));
        assert_eq!(fixed_clock_scale(&ticks), None);
    }
}
