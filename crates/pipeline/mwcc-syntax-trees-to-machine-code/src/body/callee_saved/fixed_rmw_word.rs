//! Parameterized and constant-return fixed-address word-register schedules.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts};
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
        if !function.parameters.is_empty()
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
        } = peel_casts(value)
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
        let Some(return_value) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let (high, low) = crate::expressions::split_address(base_address);
        let folded = i16::try_from(low as i64 + index * 4)
            .map_err(|_| Diagnostic::error("fixed-address word RMW is out of range"))?;
        let style = self.behavior.fixed_address_parameterized_rmw_style;
        match style {
            FixedAddressParameterizedRmwStyle::Legacy233 => {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(3, high));
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 4,
                    a: 3,
                    immediate: low,
                });
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 3,
                    offset: folded,
                });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, return_value));
                self.output.instructions.push(Instruction::AndImmediateRecord {
                    a: 0,
                    s: 0,
                    immediate: mask,
                });
                let offset = i16::try_from(index * 4)
                    .map_err(|_| Diagnostic::error("fixed-address word RMW is out of range"))?;
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 4,
                    offset,
                });
            }
            FixedAddressParameterizedRmwStyle::Early24 => {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(5, high));
                self.output
                    .instructions
                    .push(Instruction::load_immediate(0, mask as i16));
                self.output.instructions.push(Instruction::LoadWord {
                    d: 4,
                    a: 5,
                    offset: folded,
                });
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, return_value));
                self.output.instructions.push(Instruction::And {
                    a: 0,
                    s: 4,
                    b: 0,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 5,
                    offset: folded,
                });
            }
            FixedAddressParameterizedRmwStyle::Mainline24
            | FixedAddressParameterizedRmwStyle::Modern4x => {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(4, high));
                self.output
                    .instructions
                    .push(Instruction::load_immediate(3, return_value));
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 4,
                    offset: folded,
                });
                self.output.instructions.push(Instruction::AndImmediateRecord {
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

    /// A word register update that preserves masked state, inserts a shifted
    /// parameter field, writes the same slot, and returns a constant. This is
    /// the debugger EXI-select leaf; its schedule changes at both the 2.3.3 →
    /// 2.4.x and 2.4.x → 4.x optimizer boundaries.
    pub(crate) fn try_fixed_address_parameterized_rmw(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || function.parameters.len() != 1
            || function.locals.len() != 1
            || function.return_type != Type::UnsignedInt
        {
            return Ok(false);
        }
        let [parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        let [temporary] = function.locals.as_slice() else {
            return Ok(false);
        };
        if parameter.parameter_type != Type::UnsignedInt
            || temporary.declared_type != Type::UnsignedInt
            || temporary.array_length.is_some()
            || temporary.is_static
        {
            return Ok(false);
        }
        let Some(initializer) = temporary.initializer.as_ref() else {
            return Ok(false);
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
        }] = function.statements.as_slice()
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
        if !matches!(masked_left.as_ref(), Expression::Variable(name) if name == &temporary.name)
        {
            return Ok(false);
        }
        let Some(mask) = constant_value(masked_right).and_then(|value| u16::try_from(value).ok())
        else {
            return Ok(false);
        };
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
        if !matches!(shift_value.as_ref(), Expression::Variable(name) if name == &parameter.name)
        {
            return Ok(false);
        }
        let Some(shift) = constant_value(shift_amount)
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| *value < 32)
        else {
            return Ok(false);
        };
        let Some(return_value) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };

        let (high, low) = crate::expressions::split_address(base_address);
        let style = self.behavior.fixed_address_parameterized_rmw_style;
        let (base, loaded) = match style {
            FixedAddressParameterizedRmwStyle::Modern4x => (5, 4),
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
        self.output.instructions.push(Instruction::ShiftLeftImmediate {
            a: 0,
            s: 3,
            shift,
        });
        if style != FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output.instructions.push(Instruction::LoadWord {
                d: loaded,
                a: base,
                offset: displacement,
            });
        }
        if style == FixedAddressParameterizedRmwStyle::Early24 {
            self.output
                .instructions
                .push(Instruction::load_immediate(4, mask as i16));
        }
        if style != FixedAddressParameterizedRmwStyle::Modern4x {
            self.output.instructions.push(Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: set_bits,
            });
        }
        if style == FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output.instructions.push(Instruction::AndImmediateRecord {
                a: loaded,
                s: loaded,
                immediate: mask,
            });
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(3, return_value));
        if style == FixedAddressParameterizedRmwStyle::Early24 {
            self.output.instructions.push(Instruction::And {
                a: loaded,
                s: loaded,
                b: 4,
            });
        } else if style != FixedAddressParameterizedRmwStyle::Legacy233 {
            self.output.instructions.push(Instruction::AndImmediateRecord {
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
        self.emit_epilogue_and_return();
        Ok(true)
    }

}
