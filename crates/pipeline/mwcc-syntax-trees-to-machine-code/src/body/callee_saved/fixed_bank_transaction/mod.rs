//! Select/transfer/poll/reset transactions over a fixed word-register bank.
//!
//! Recognition describes memory and call effects. Version schedules own the
//! distinct address lifetimes and frame layout, without naming SDK symbols.

use super::super::*;
use mwcc_versions::{FixedAddressParameterizedRmwStyle, Optimization};

mod legacy;
mod recognize;
mod stream;
mod stream_legacy;
mod stream_mainline;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
enum Payload {
    Write {
        begin: u8,
        end: u8,
        high: u16,
    },
    Read {
        high: u16,
    },
    Stream {
        command: stream::Command,
        writing: bool,
    },
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
        if self.behavior.optimization != Optimization::O4
            || self.variadic_definition
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let plan = (self.behavior.fixed_address_parameterized_rmw_style
            == FixedAddressParameterizedRmwStyle::Legacy233)
            .then(|| recognize::transaction(function, &self.fixed_address_arrays))
            .flatten()
            .or_else(|| {
                (self.behavior.fixed_bank_stream_style
                    != mwcc_versions::FixedBankStreamStyle::Structured
                    && self.behavior.optimization_goal
                        == mwcc_versions::OptimizationGoal::Performance
                    && self.behavior.scheduler_enabled
                    && !self.behavior.power_pc_7400_scheduling_enabled()
                    && !function.peephole_disabled)
                    .then(|| stream::transaction(function, &self.fixed_address_arrays))
                    .flatten()
            });
        let Some(plan) = plan else {
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
            || function
                .parameters
                .iter()
                .enumerate()
                .any(|(index, parameter)| {
                    !self.locations.get(&parameter.name).is_some_and(|location| {
                        location.class == ValueClass::General
                            && location.register == 3 + index as u8
                            && location.width == 32
                    })
                })
        {
            return Ok(false);
        }
        if let Payload::Stream { command, writing } = plan.payload {
            if matches!(
                self.behavior.fixed_bank_stream_style,
                mwcc_versions::FixedBankStreamStyle::MainlineRegisterMask
                    | mwcc_versions::FixedBankStreamStyle::MainlineImmediateMask
                    | mwcc_versions::FixedBankStreamStyle::RetainedPage
                    | mwcc_versions::FixedBankStreamStyle::RetainedPageEarlyStore
            ) {
                let (_, low) = crate::expressions::split_address(plan.address);
                let Some(poll_offset) = low.checked_add(plan.poll) else {
                    return Ok(false);
                };
                self.non_leaf = true;
                self.output.pre_scheduled = true;
                self.emit_mainline_bank_stream(&plan, command, writing, poll_offset);
                return Ok(true);
            }
        }
        self.emit_legacy_fixed_bank_transaction(&plan);
        Ok(true)
    }
}
