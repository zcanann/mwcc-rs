//! Parameterized and constant-return fixed-address word-register schedules.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts, peel_update_value};
#[allow(unused_imports)]
use super::*;
use mwcc_versions::FixedAddressParameterizedRmwStyle;

impl Generator {
    /// A direct word-register mask followed by a constant return. The same
    /// semantic leaf uses three observed schedules across 2.3.3, early 2.4.x,
    /// and later compilers.
    pub(crate) fn try_fixed_address_word_rmw_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let passthrough = matches!(function.parameters.as_slice(), [parameter]
            if matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt)
                && matches!(&function.return_expression, Some(Expression::Variable(name)) if name == &parameter.name)
                && self.locations.get(&parameter.name).is_some_and(|location| location.class == ValueClass::General && location.register == 3));
        if (!function.parameters.is_empty() && !passthrough)
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [Statement::Store { target, value }] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some((bank, index)) = fixed_slot(target) else {
            return Ok(false);
        };
        let Some(&(base_address, Type::UnsignedInt)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left,
            right,
        } = peel_update_value(value)
        else {
            return Ok(false);
        };
        let mask = if same_operand(target, left) {
            constant_value(right)
        } else if same_operand(target, right) {
            constant_value(left)
        } else {
            None
        };
        let Some(mask) = mask.and_then(|value| u16::try_from(value).ok()) else {
            return Ok(false);
        };
        let return_value = if passthrough {
            None
        } else {
            let Some(value) = function
                .return_expression
                .as_ref()
                .and_then(constant_value)
                .and_then(|value| i16::try_from(value).ok())
            else {
                return Ok(false);
            };
            Some(value)
        };
        let (high, low) = crate::expressions::split_address(base_address);
        let compound_update = matches!(peel_casts(value), Expression::IndexedUpdateValue { .. });
        let folded = i16::try_from(low as i64 + index * 4)
            .map_err(|_| Diagnostic::error("fixed-address word RMW is out of range"))?;
        let style = self.behavior.fixed_address_parameterized_rmw_style;
        if passthrough
            && style == FixedAddressParameterizedRmwStyle::Early24
            && mask > i16::MAX as u16
        {
            return Ok(false);
        }
        if passthrough && style == FixedAddressParameterizedRmwStyle::Legacy233 {
            let offset = i16::try_from(index * 4)
                .map_err(|_| Diagnostic::error("fixed-address word RMW is out of range"))?;
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(4, high),
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: low,
                },
                Instruction::LoadWord { d: 0, a: 4, offset },
                Instruction::AndImmediateRecord {
                    a: 0,
                    s: 0,
                    immediate: mask,
                },
                Instruction::StoreWord { s: 0, a: 4, offset },
            ]);
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        match style {
            FixedAddressParameterizedRmwStyle::Legacy233 => {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(3, high));
                let store_base = Instruction::AddImmediate {
                    d: 4,
                    a: 3,
                    immediate: low,
                };
                let load = Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: folded,
                };
                // Explicit assignment loads before completing the store base;
                // compound syntax completes that base first on 2.3.3.
                self.output.instructions.extend(if compound_update {
                    [store_base, load]
                } else {
                    [load, store_base]
                });
                if let Some(value) = return_value {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(3, value));
                }
                self.output
                    .instructions
                    .push(Instruction::AndImmediateRecord {
                        a: 0,
                        s: 0,
                        immediate: mask,
                    });
                let offset = i16::try_from(index * 4)
                    .map_err(|_| Diagnostic::error("fixed-address word RMW is out of range"))?;
                self.output
                    .instructions
                    .push(Instruction::StoreWord { s: 0, a: 4, offset });
            }
            FixedAddressParameterizedRmwStyle::Early24 if compound_update => {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(5, high));
                self.output.instructions.push(if mask <= i16::MAX as u16 {
                    Instruction::load_immediate(0, mask as i16)
                } else {
                    Instruction::load_immediate_shifted(3, 1)
                });
                self.output.instructions.push(Instruction::LoadWord {
                    d: 4,
                    a: 5,
                    offset: folded,
                });
                if mask > i16::MAX as u16 {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: 0,
                        a: 3,
                        immediate: mask as i16,
                    });
                }
                if let Some(value) = return_value {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(3, value));
                }
                self.output
                    .instructions
                    .push(Instruction::And { a: 0, s: 4, b: 0 });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 5,
                    offset: folded,
                });
            }
            FixedAddressParameterizedRmwStyle::Early24
            | FixedAddressParameterizedRmwStyle::Mainline24
            | FixedAddressParameterizedRmwStyle::Modern4x => {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(4, high));
                if let Some(value) = return_value {
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(3, value));
                }
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 4,
                    offset: folded,
                });
                self.output
                    .instructions
                    .push(Instruction::AndImmediateRecord {
                        a: 0,
                        s: 0,
                        immediate: mask,
                    });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 4,
                    offset: folded,
                });
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A word register update that preserves masked state, inserts a constant
    /// or shifted parameter field, and writes the same slot. This is
    /// the debugger EXI-select leaf; its schedule changes at both the 2.3.3 →
    /// 2.4.x and 2.4.x → 4.x optimizer boundaries.
    pub(crate) fn try_fixed_address_field_rmw(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty()
            || function.locals.len() != 1
            || !matches!(
                function.return_type,
                Type::Void | Type::Int | Type::UnsignedInt
            )
        {
            return Ok(false);
        }
        let [temporary] = function.locals.as_slice() else {
            return Ok(false);
        };
        if temporary.declared_type != Type::UnsignedInt
            || temporary.array_length.is_some()
            || temporary.is_static
            || temporary.is_volatile
        {
            return Ok(false);
        }
        let Some(initializer) = temporary.initializer.as_ref() else {
            return Ok(false);
        };
        let (updates, reset, forward) = match function.statements.as_slice() {
            [_, _, _] => (function.statements.as_slice(), None, None),
            [updates @ .., Statement::Store { target, value }] if updates.len() == 3 => {
                (updates, Some((target, value)), None)
            }
            [updates @ .., Statement::Expression(call @ Expression::Call { .. })]
                if updates.len() == 3 =>
            {
                (updates, None, Some(call))
            }
            _ => return Ok(false),
        };
        let [Statement::Assign {
            name: masked_name,
            value: masked_value,
        }, Statement::Assign {
            name: inserted_name,
            value: inserted_value,
        }, Statement::Store {
            target,
            value: stored_value,
        }] = updates
        else {
            return Ok(false);
        };
        if masked_name != &temporary.name
            || inserted_name != &temporary.name
            || !matches!(stored_value, Expression::Variable(name) if name == &temporary.name)
        {
            return Ok(false);
        }
        let Some((bank, index)) = fixed_slot(initializer) else {
            return Ok(false);
        };
        let Some((stored_bank, stored_index)) = fixed_slot(target) else {
            return Ok(false);
        };
        if bank != stored_bank || index != stored_index {
            return Ok(false);
        }
        let Some(&(base_address, Type::UnsignedInt)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: masked_left,
            right: masked_right,
        } = peel_casts(masked_value)
        else {
            return Ok(false);
        };
        if !matches!(masked_left.as_ref(), Expression::Variable(name) if name == &temporary.name) {
            return Ok(false);
        }
        let Some(mask) = constant_value(masked_right).and_then(|value| u16::try_from(value).ok())
        else {
            return Ok(false);
        };
        if let Some((target, value)) = reset {
            let Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            } = peel_update_value(value)
            else {
                return Ok(false);
            };
            if fixed_slot(target) != Some((bank, index))
                || !same_operand(target, left)
                || constant_value(right) != Some(i64::from(mask))
                || !matches!(peel_casts(value), Expression::IndexedUpdateValue { .. })
                || function.return_type == Type::Void
            {
                return Ok(false);
            }
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: inserted_left,
            right: inserted_right,
        } = peel_casts(inserted_value)
        else {
            return Ok(false);
        };
        let inserted_bits = if matches!(inserted_left.as_ref(), Expression::Variable(name) if name == &temporary.name)
        {
            inserted_right.as_ref()
        } else if matches!(inserted_right.as_ref(), Expression::Variable(name) if name == &temporary.name)
        {
            inserted_left.as_ref()
        } else {
            return Ok(false);
        };
        if let Some(set_bits) =
            constant_value(inserted_bits).and_then(|value| u16::try_from(value).ok())
        {
            return self.try_emit_constant_fixed_word_rmw(
                function,
                super::fixed_rmw_constant::ConstantRmw {
                    address: base_address,
                    index,
                    mask,
                    set_bits,
                    reset: reset.is_some(),
                },
                forward,
            );
        }
        if forward.is_some() {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::UnsignedInt {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: bits_left,
            right: bits_right,
        } = peel_casts(inserted_bits)
        else {
            return Ok(false);
        };
        let (set_bits, shifted) = if let Some(value) = constant_value(bits_left) {
            (value, bits_right.as_ref())
        } else if let Some(value) = constant_value(bits_right) {
            (value, bits_left.as_ref())
        } else {
            return Ok(false);
        };
        let Some(set_bits) = u16::try_from(set_bits).ok() else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::ShiftLeft,
            left: shift_value,
            right: shift_amount,
        } = peel_casts(shifted)
        else {
            return Ok(false);
        };
        if !matches!(shift_value.as_ref(), Expression::Variable(name) if name == &parameter.name) {
            return Ok(false);
        }
        let Some(shift) = constant_value(shift_amount)
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| *value < 32)
        else {
            return Ok(false);
        };
        let return_value = match (&function.return_type, &function.return_expression) {
            (Type::Void, None) => None,
            (Type::Int | Type::UnsignedInt, Some(value)) => {
                let Some(value) = constant_value(value).and_then(|value| i16::try_from(value).ok())
                else {
                    return Ok(false);
                };
                Some(value)
            }
            _ => return Ok(false),
        };

        let (high, low) = crate::expressions::split_address(base_address);
        let style = self.behavior.fixed_address_parameterized_rmw_style;
        if return_value.is_none() {
            self.emit_discarded_parameterized_rmw(high, low, index, mask, set_bits, shift)?;
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        if reset.is_some() && style == FixedAddressParameterizedRmwStyle::Legacy233 {
            self.emit_discarded_parameterized_rmw(high, low, index, mask, set_bits, shift)?;
            self.output.instructions.insert(
                self.output.instructions.len() - 2,
                Instruction::load_immediate(3, return_value.expect("status form")),
            );
            let offset = i16::try_from(index * 4).map_err(|_| {
                Diagnostic::error("fixed-address parameterized RMW is out of range")
            })?;
            self.emit_parameterized_rmw_reset(5, offset, mask, None);
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        // The early wide-mask lane consumes its mask home for the status.
        // Retaining that mask across a reset needs a separate allocation plan.
        if reset.is_some()
            && style == FixedAddressParameterizedRmwStyle::Early24
            && mask > i16::MAX as u16
        {
            return Ok(false);
        }
        let wide_early_mask =
            style == FixedAddressParameterizedRmwStyle::Early24 && mask > i16::MAX as u16;
        let (base, loaded) = match style {
            FixedAddressParameterizedRmwStyle::Modern4x => (5, 4),
            FixedAddressParameterizedRmwStyle::Early24 if wide_early_mask => (4, 5),
            FixedAddressParameterizedRmwStyle::Early24 => (5, 6),
            _ => (4, 5),
        };
        let displacement = i16::try_from(match style {
            FixedAddressParameterizedRmwStyle::Legacy233 => index * 4,
            _ => low as i64 + index * 4,
        })
        .map_err(|_| Diagnostic::error("fixed-address parameterized RMW is out of range"))?;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(base, high));
        if style == FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output.instructions.push(Instruction::AddImmediate {
                d: base,
                a: base,
                immediate: low,
            });
        }
        if style == FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output.instructions.push(Instruction::LoadWord {
                d: loaded,
                a: base,
                offset: displacement,
            });
        }
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate { a: 0, s: 3, shift });
        if wide_early_mask {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(3, 1));
        }
        if style != FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output.instructions.push(Instruction::LoadWord {
                d: loaded,
                a: base,
                offset: displacement,
            });
        }
        if style == FixedAddressParameterizedRmwStyle::Early24 {
            self.output.instructions.push(if wide_early_mask {
                Instruction::AddImmediate {
                    d: 3,
                    a: 3,
                    immediate: mask as i16,
                }
            } else {
                Instruction::load_immediate(4, mask as i16)
            });
        }
        if style != FixedAddressParameterizedRmwStyle::Modern4x {
            self.output.instructions.push(Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: set_bits,
            });
        }
        if style == FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output
                .instructions
                .push(Instruction::AndImmediateRecord {
                    a: loaded,
                    s: loaded,
                    immediate: mask,
                });
        } else if wide_early_mask {
            self.output.instructions.push(Instruction::And {
                a: loaded,
                s: loaded,
                b: 3,
            });
        }
        self.output.instructions.push(Instruction::load_immediate(
            3,
            return_value.expect("status form"),
        ));
        if style == FixedAddressParameterizedRmwStyle::Early24 && !wide_early_mask {
            self.output.instructions.push(Instruction::And {
                a: loaded,
                s: loaded,
                b: 4,
            });
        } else if style != FixedAddressParameterizedRmwStyle::Legacy233 && !wide_early_mask {
            self.output
                .instructions
                .push(Instruction::AndImmediateRecord {
                    a: loaded,
                    s: loaded,
                    immediate: mask,
                });
        }
        if style == FixedAddressParameterizedRmwStyle::Modern4x {
            self.output.instructions.push(Instruction::OrImmediate {
                a: loaded,
                s: loaded,
                immediate: set_bits,
            });
        }
        self.output.instructions.push(Instruction::Or {
            a: loaded,
            s: loaded,
            b: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: loaded,
            a: base,
            offset: displacement,
        });
        if reset.is_some() {
            let mask_register = (style == FixedAddressParameterizedRmwStyle::Early24).then_some(4);
            self.emit_parameterized_rmw_reset(base, displacement, mask, mask_register);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn emit_parameterized_rmw_reset(
        &mut self,
        base: u8,
        offset: i16,
        mask: u16,
        mask_register: Option<u8>,
    ) {
        // Preserve the second volatile read even though its address and mask
        // are shared with the preceding update.
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: base,
                offset,
            },
            match mask_register {
                Some(register) => Instruction::And {
                    a: 0,
                    s: 0,
                    b: register,
                },
                None => Instruction::AndImmediateRecord {
                    a: 0,
                    s: 0,
                    immediate: mask,
                },
            },
            Instruction::StoreWord {
                s: 0,
                a: base,
                offset,
            },
        ]);
    }

    /// Discarding the status frees r3 after the field shift. Legacy selection
    /// retains a separate materialized store base; later builds fold the bank
    /// displacement and reuse r3 for either the mask or loaded register value.
    fn emit_discarded_parameterized_rmw(
        &mut self,
        high: i16,
        low: i16,
        index: i64,
        mask: u16,
        set_bits: u16,
        shift: u8,
    ) -> Compilation<()> {
        let style = self.behavior.fixed_address_parameterized_rmw_style;
        let legacy = style == FixedAddressParameterizedRmwStyle::Legacy233;
        let early = style == FixedAddressParameterizedRmwStyle::Early24;
        let modern = style == FixedAddressParameterizedRmwStyle::Modern4x;
        let displacement = i16::try_from(i64::from(low) + index * 4)
            .map_err(|_| Diagnostic::error("fixed-address parameterized RMW is out of range"))?;
        let store_offset = if legacy {
            i16::try_from(index * 4)
                .map_err(|_| Diagnostic::error("fixed-address parameterized RMW is out of range"))?
        } else {
            displacement
        };
        if early && mask > i16::MAX as u16 {
            // The non-signed-immediate mask has its own live range; selection
            // prepares it before consuming the incoming field value in r3.
            self.output.instructions.extend([
                Instruction::load_immediate_shifted(5, high),
                Instruction::load_immediate_shifted(4, 1),
                Instruction::LoadWord {
                    d: 6,
                    a: 5,
                    offset: displacement,
                },
                Instruction::AddImmediate {
                    d: 4,
                    a: 4,
                    immediate: mask as i16,
                },
                Instruction::ShiftLeftImmediate { a: 0, s: 3, shift },
                Instruction::And { a: 6, s: 6, b: 4 },
                Instruction::OrImmediate {
                    a: 0,
                    s: 0,
                    immediate: set_bits,
                },
                Instruction::Or { a: 6, s: 6, b: 0 },
                Instruction::StoreWord {
                    s: 6,
                    a: 5,
                    offset: displacement,
                },
            ]);
            return Ok(());
        }
        let loaded = if legacy {
            4
        } else if early {
            5
        } else {
            3
        };
        let base = if legacy { 5 } else { 4 };
        let shift = Instruction::ShiftLeftImmediate { a: 0, s: 3, shift };
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, high));
        if legacy {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 5,
                a: 4,
                immediate: low,
            });
        } else {
            self.output.instructions.push(shift.clone());
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: loaded,
            a: 4,
            offset: displacement,
        });
        if legacy {
            self.output.instructions.push(shift);
        }
        if early {
            self.output
                .instructions
                .push(Instruction::load_immediate(3, mask as i16));
        }
        if !legacy && !modern {
            self.output.instructions.push(Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: set_bits,
            });
        }
        self.output.instructions.push(if early {
            Instruction::And {
                a: loaded,
                s: loaded,
                b: 3,
            }
        } else {
            Instruction::AndImmediateRecord {
                a: loaded,
                s: loaded,
                immediate: mask,
            }
        });
        if legacy || modern {
            let register = if modern { loaded } else { 0 };
            self.output.instructions.push(Instruction::OrImmediate {
                a: register,
                s: register,
                immediate: set_bits,
            });
        }
        self.output.instructions.push(Instruction::Or {
            a: loaded,
            s: loaded,
            b: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: loaded,
            a: base,
            offset: store_offset,
        });
        Ok(())
    }
}
