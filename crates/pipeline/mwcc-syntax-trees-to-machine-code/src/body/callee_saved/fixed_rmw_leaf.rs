//! Single-node leaf schedules for fixed-address hardware-register updates.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts};
#[allow(unused_imports)]
use super::*;
use mwcc_versions::FixedAddressRmwStyle;

impl Generator {
    /// A one-node fixed-register update: `bank[k] |= C` or `bank[k] &= C`.
    pub(crate) fn try_fixed_address_immediate_rmw(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.try_fixed_address_word_rmw_return(function)? {
            return Ok(true);
        }
        if self.try_fixed_address_direct_immediate_rmw(function)? {
            return Ok(true);
        }
        if self.try_fixed_address_parameterized_rmw(function)? {
            return Ok(true);
        }
        if self.try_fixed_address_split_word_rmw(function)? {
            return Ok(true);
        }
        self.try_fixed_address_local_rmw(function)
    }

    fn try_fixed_address_direct_immediate_rmw(&mut self, function: &Function) -> Compilation<bool> {
        if !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [Statement::Store { target, value }] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some((bank, index)) = fixed_slot(target) else {
            return Ok(false);
        };
        let Some(&(base_address, element_type)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        if element_type != Type::UnsignedShort {
            return Ok(false);
        }
        let Expression::Binary {
            operator,
            left,
            right,
        } = peel_casts(value)
        else {
            return Ok(false);
        };
        let constant = if same_operand(target, left) {
            constant_value(right)
        } else if *operator == BinaryOperator::BitOr && same_operand(target, right) {
            constant_value(left)
        } else {
            None
        };
        let Some(constant) = constant else {
            return Ok(false);
        };
        let update = match operator {
            BinaryOperator::BitOr if u16::try_from(constant).is_ok() => Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: constant as u16,
            },
            BinaryOperator::BitAnd => {
                // The store narrows to a halfword: `~0x8000` arrives as signed
                // -32769 but semantically masks the loaded u16 with 0x7fff.
                let mask_range = match self.behavior.fixed_address_rmw_style {
                    FixedAddressRmwStyle::FoldedDisplacementWithNarrowMask => {
                        rlwinm_mask(constant as u16 as i64).or_else(|| rlwinm_mask(constant))
                    }
                    FixedAddressRmwStyle::MaterializedPageWithPromotedMask => rlwinm_mask(constant),
                };
                let Some((begin, end)) = mask_range else {
                    return Ok(false);
                };
                if end == 31 {
                    Instruction::ClearLeftImmediate {
                        a: 0,
                        s: 0,
                        clear: begin,
                    }
                } else {
                    Instruction::RotateAndMask {
                        a: 0,
                        s: 0,
                        shift: 0,
                        begin,
                        end,
                    }
                }
            }
            _ => return Ok(false),
        };

        let address = u32::try_from(base_address)
            .map_err(|_| Diagnostic::error("fixed-address register bank is out of range"))?;
        let (high, low) = crate::expressions::split_address(address);
        let materialize_page = self.behavior.fixed_address_rmw_style
            == FixedAddressRmwStyle::MaterializedPageWithPromotedMask
            && index != 0;
        let offset = i16::try_from(if materialize_page {
            index * 2
        } else {
            low as i64 + index * 2
        })
        .map_err(|_| Diagnostic::error("fixed-address RMW displacement is out of range"))?;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, high));
        if materialize_page {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 3,
                immediate: low,
            });
        }
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                0,
                3,
                offset,
            )?);
        self.output.instructions.push(update);
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                0,
                3,
                offset,
            )?);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The explicit-local spelling of one fixed-register DAG:
    ///
    /// ```text
    /// temporary = bank[k];
    /// temporary = (temporary & MASK) | BITS;
    /// bank[k] = temporary;
    /// ```
    ///
    /// mwcc materializes the mask ahead of the halfword load and carries the
    /// updated value in r0. Keeping this beside the direct single-node form
    /// makes the family share ownership without teaching generic local value
    /// tracking about fixed-address base scheduling.
    fn try_fixed_address_local_rmw(&mut self, function: &Function) -> Compilation<bool> {
        if !function.parameters.is_empty()
            || !function.guards.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || function.locals.len() != 1
        {
            return Ok(false);
        }
        let [temporary] = function.locals.as_slice() else {
            return Ok(false);
        };
        if temporary.declared_type != Type::UnsignedShort
            || temporary.array_length.is_some()
            || temporary.is_static
            || temporary.initializer.is_some()
        {
            return Ok(false);
        }
        let [Statement::Assign {
            name: loaded_name,
            value: loaded_value,
        }, Statement::Assign {
            name: updated_name,
            value: updated_value,
        }, Statement::Store {
            target,
            value: stored_value,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if loaded_name != &temporary.name
            || updated_name != &temporary.name
            || !matches!(stored_value, Expression::Variable(name) if name == &temporary.name)
        {
            return Ok(false);
        }
        let Some((bank, index)) = fixed_slot(loaded_value) else {
            return Ok(false);
        };
        let Some((stored_bank, stored_index)) = fixed_slot(target) else {
            return Ok(false);
        };
        if bank != stored_bank || index != stored_index {
            return Ok(false);
        }
        let Some(&(base_address, element_type)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        if element_type != Type::UnsignedShort {
            return Ok(false);
        }
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: preserved,
            right: set_bits,
        } = peel_casts(updated_value)
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: preserved_value,
            right: preserve_mask,
        } = peel_casts(preserved)
        else {
            return Ok(false);
        };
        if !matches!(preserved_value.as_ref(), Expression::Variable(name) if name == &temporary.name)
        {
            return Ok(false);
        }
        let Some(preserve_mask) =
            constant_value(preserve_mask).and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let Some(set_bits) = constant_value(set_bits)
            .and_then(|value| u16::try_from(value).ok())
            .filter(|value| *value != 0)
        else {
            return Ok(false);
        };

        let (high, low) = crate::expressions::split_address(base_address);
        let materialize_page = self.behavior.fixed_address_rmw_style
            == FixedAddressRmwStyle::MaterializedPageWithPromotedMask
            && index != 0;
        let offset = i16::try_from(if materialize_page {
            index * 2
        } else {
            low as i64 + index * 2
        })
        .map_err(|_| Diagnostic::error("fixed-address local RMW displacement is out of range"))?;
        let (base_register, loaded_register) = if materialize_page { (3, 4) } else { (4, 3) };
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(base_register, high));
        if materialize_page {
            self.output.instructions.push(Instruction::AddImmediate {
                d: base_register,
                a: base_register,
                immediate: low,
            });
        } else {
            self.output
                .instructions
                .push(Instruction::load_immediate(0, preserve_mask));
        }
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                loaded_register,
                base_register,
                offset,
            )?);
        if materialize_page {
            self.output
                .instructions
                .push(Instruction::load_immediate(0, preserve_mask));
        }
        self.output.instructions.push(Instruction::And {
            a: 0,
            s: loaded_register,
            b: 0,
        });
        self.output.instructions.push(Instruction::OrImmediate {
            a: 0,
            s: 0,
            immediate: set_bits,
        });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                0,
                base_register,
                offset,
            )?);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
