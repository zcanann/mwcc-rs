//! Select/transfer/poll/reset transactions over a fixed word-register bank.
//!
//! Recognition describes memory and call effects. Version schedules own the
//! distinct address lifetimes and frame layout, without naming SDK symbols.

use super::super::*;
use mwcc_versions::{FixedAddressParameterizedRmwStyle, Optimization};

mod legacy;
mod recognize;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
enum Payload {
    Write { begin: u8, end: u8, high: u16 },
    Read { high: u16 },
}

#[derive(Debug)]
struct Transaction<'a> {
    address: u32,
    selected: i16,
    poll: i16,
    preserve: u16,
    insert: u16,
    poll_begin: u8,
    poll_end: u8,
    transfer: &'a str,
    payload: Payload,
}

impl Generator {
    pub(crate) fn try_fixed_bank_transaction(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.fixed_address_parameterized_rmw_style
            != FixedAddressParameterizedRmwStyle::Legacy233
            || self.behavior.optimization != Optimization::O4
            || self.variadic_definition
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(plan) = recognize::transaction(function, &self.fixed_address_arrays) else {
            return Ok(false);
        };
        let target = plan.transfer;
        if self.globals.contains_key(target)
            || self.locations.contains_key(target)
            || self.known_locals.contains(target)
            || self.variadic_callees.contains(target)
            || !matches!(
                self.call_return_types.get(target),
                Some(Type::Int | Type::UnsignedInt)
            )
            || !self.call_parameter_types.get(target).is_some_and(|types| {
                matches!(
                    types.as_slice(),
                    [
                        Type::Pointer(_),
                        Type::Int | Type::UnsignedInt,
                        Type::Int | Type::UnsignedInt
                    ]
                )
            })
            || self.inline_bodies.asm_fragment(target).is_some()
            || self
                .inline_bodies
                .parameterized_asm_fragment(target)
                .is_some()
            || crate::intrinsics::ordering_instruction(target, 3).is_some()
            || !self
                .locations
                .get(&function.parameters[0].name)
                .is_some_and(|location| {
                    location.class == ValueClass::General
                        && location.register == 3
                        && location.width == 32
                })
        {
            return Ok(false);
        }
        self.emit_legacy_fixed_bank_transaction(&plan);
        Ok(true)
    }
}
