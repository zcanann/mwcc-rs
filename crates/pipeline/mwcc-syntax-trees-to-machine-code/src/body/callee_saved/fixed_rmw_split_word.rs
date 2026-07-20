//! Word-sized fixed registers updated through separate local assignments.

use super::fixed_rmw_recognize::{fixed_slot, peel_casts};
#[allow(unused_imports)]
use super::*;
use mwcc_versions::FixedAddressRmwStyle;

impl Generator {
    /// A fixed register updated as load, OR, AND, store. Legacy Dolphin helpers
    /// use this exact spelling; the explicit-address generation materializes the
    /// bank page once, carries the word in r0, and applies both immediates there.
    pub(super) fn try_fixed_address_split_word_rmw(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.fixed_address_rmw_style
            != FixedAddressRmwStyle::MaterializedPageWithPromotedMask
            || !function.parameters.is_empty()
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
        if temporary.declared_type != Type::UnsignedInt
            || temporary.array_length.is_some()
            || temporary.is_static
            || temporary.initializer.is_some()
        {
            return Ok(false);
        }
        let [
            Statement::Assign {
                name: loaded_name,
                value: loaded_value,
            },
            Statement::Assign {
                name: set_name,
                value: set_value,
            },
            Statement::Assign {
                name: masked_name,
                value: masked_value,
            },
            Statement::Store {
                target,
                value: stored_value,
            },
        ] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if loaded_name != &temporary.name
            || set_name != &temporary.name
            || masked_name != &temporary.name
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
        let Some(&(base_address, Type::UnsignedInt)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        let immediate_update =
            |expression: &Expression, operator: BinaryOperator| -> Option<i64> {
                let Expression::Binary {
                    operator: actual,
                    left,
                    right,
                } = peel_casts(expression)
                else {
                    return None;
                };
                if *actual != operator {
                    return None;
                }
                if matches!(left.as_ref(), Expression::Variable(name) if name == &temporary.name) {
                    constant_value(right)
                } else if matches!(right.as_ref(), Expression::Variable(name) if name == &temporary.name)
                {
                    constant_value(left)
                } else {
                    None
                }
            };
        let Some(set_bits) = immediate_update(set_value, BinaryOperator::BitOr) else {
            return Ok(false);
        };
        let Some(preserve_mask) = immediate_update(masked_value, BinaryOperator::BitAnd) else {
            return Ok(false);
        };
        let Some((begin, end)) = rlwinm_mask(preserve_mask) else {
            return Ok(false);
        };
        let bits = set_bits as u32;
        let set_instruction = match (bits >> 16, bits & 0xffff) {
            (high, 0) if high != 0 => Instruction::OrImmediateShifted {
                a: 0,
                s: 0,
                immediate: high as u16,
            },
            (0, low) if low != 0 => Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: low as u16,
            },
            _ => return Ok(false),
        };
        let (high, low) = crate::expressions::split_address(base_address);
        let offset = i16::try_from(index * 4)
            .map_err(|_| Diagnostic::error("fixed-address word RMW offset is out of range"))?;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, high));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 3,
            immediate: low,
        });
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedInt,
                0,
                3,
                offset,
            )?);
        self.output.instructions.push(set_instruction);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: 0,
            shift: 0,
            begin,
            end,
        });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedInt,
                0,
                3,
                offset,
            )?);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
