//! Compose a status/stream/mailbox retry protocol from verified bank helpers.

use super::*;
use mwcc_versions::{GlobalAddressing, OptimizationGoal, PlainLinkageEpilogueStyle};

mod legacy;
mod recognize;

impl Generator {
    pub(crate) fn try_bank_retry_transport(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.fixed_address_parameterized_rmw_style
            != FixedAddressParameterizedRmwStyle::Legacy233
            || self.behavior.optimization != Optimization::O4
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.behavior.power_pc_7400_scheduling_enabled()
            || !self.behavior.automatic_inlining_enabled
            || function.peephole_disabled
            || self.behavior.global_addressing != GlobalAddressing::SmallData
            || self.variadic_definition
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(caller) = recognize::transport(function) else {
            return Ok(false);
        };
        if self.globals.get(caller.counter) != Some(&Type::UnsignedChar)
            || self.volatile_globals.contains(caller.counter)
            || self.global_arrays.contains(caller.counter)
            || self.full_bss_globals.contains(caller.counter)
            || !self.retry_call_abi(caller.acquire, &[], true)
            || !self.retry_call_abi(caller.release, &[Type::Int], false)
            || !self.retry_call_abi(caller.status, &[Type::Pointer(Pointee::UnsignedInt)], true)
            || !self.retry_call_abi(caller.mailbox, &[Type::UnsignedInt], true)
            || !self.retry_call_abi(
                caller.stream,
                &[
                    Type::UnsignedInt,
                    Type::Pointer(Pointee::UnsignedInt),
                    Type::UnsignedInt,
                ],
                true,
            )
            || function
                .parameters
                .iter()
                .enumerate()
                .any(|(index, parameter)| {
                    !self.locations.get(&parameter.name).is_some_and(|location| {
                        location.class == ValueClass::General
                            && location.register == (3 + index as u8).into()
                            && location.width == 32
                    })
                })
        {
            return Ok(false);
        }
        let Some(status_body) =
            self.expanded_visible_bank_transaction(caller.status, &function.name)
        else {
            return Ok(false);
        };
        let Some(mailbox_body) =
            self.expanded_visible_bank_transaction(caller.mailbox, &function.name)
        else {
            return Ok(false);
        };
        let Some(status) = super::recognize::transaction(&status_body, &self.fixed_address_arrays)
        else {
            return Ok(false);
        };
        let Some(mailbox) =
            super::recognize::transaction(&mailbox_body, &self.fixed_address_arrays)
        else {
            return Ok(false);
        };
        let (Payload::Read { high: command }, Payload::Write { begin, end, high }) =
            (status.payload, mailbox.payload)
        else {
            return Ok(false);
        };
        if !same_bank_configuration(&status, &mailbox)
            || !self.bank_transfer_abi_is_compatible(status.transfer)
            || !self.bank_transfer_abi_is_compatible(mailbox.transfer)
        {
            return Ok(false);
        }
        self.emit_legacy_retry_transport(&caller, &status, &mailbox, command, (begin, end, high));
        Ok(true)
    }

    fn retry_call_abi(&self, name: &str, arguments: &[Type], result: bool) -> bool {
        !self.globals.contains_key(name)
            && !self.locations.contains_key(name)
            && !self.known_locals.contains(name)
            && !self.variadic_callees.contains(name)
            && self.inline_bodies.asm_fragment(name).is_none()
            && self
                .inline_bodies
                .parameterized_asm_fragment(name)
                .is_none()
            && crate::intrinsics::ordering_instruction(name, arguments.len()).is_none()
            && self.call_return_types.get(name).is_some_and(|ty| {
                matches!(ty, Type::Int | Type::UnsignedInt) || (!result && *ty == Type::Void)
            })
            && self.call_parameter_types.get(name).is_some_and(|types| {
                types.len() == arguments.len()
                    && types
                        .iter()
                        .zip(arguments)
                        .all(|(actual, expected)| match expected {
                            Type::Pointer(_) => matches!(actual, Type::Pointer(_)),
                            _ => matches!(actual, Type::Int | Type::UnsignedInt),
                        })
            })
    }
}
