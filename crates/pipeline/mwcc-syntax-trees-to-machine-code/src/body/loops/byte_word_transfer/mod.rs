//! Counted byte packing/unpacking around a fixed word-register transaction.
//!
//! Recognition proves the complete memory/control sequence. The version policy
//! and packet emitters separately own unrolling, register placement, and issue
//! order; no project names or device addresses participate in admission.

use super::*;
use mwcc_syntax_trees::SourceFundamentalType;
use mwcc_versions::{ByteWordTransferStyle, Optimization, OptimizationGoal};
mod legacy;
mod mainline;
mod mainline_packets;
mod packets;
mod recognize;
#[cfg(test)]
mod tests;

#[derive(Debug)]
struct Transfer<'a> {
    address: u32,
    data_offset: i16,
    control_offset: i16,
    start: u16,
    mode_shift: u8,
    count_shift: u8,
    poll_bit: u8,
    index: &'a str,
    count: &'a str,
}

impl Generator {
    pub(crate) fn try_byte_word_transfer(&mut self, function: &Function) -> Compilation<bool> {
        if self.behavior.byte_word_transfer_style == ByteWordTransferStyle::Structured
            || self.behavior.optimization != Optimization::O4
            || self.behavior.optimization_goal != OptimizationGoal::Performance
            || !self.behavior.scheduler_enabled
            || self.behavior.power_pc_7400_scheduling_enabled()
            || function.peephole_disabled
            || self.variadic_definition
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let Some(plan) = recognize::transfer(function, &self.fixed_address_arrays) else {
            return Ok(false);
        };
        if function
            .parameters
            .iter()
            .enumerate()
            .any(|(i, p)| self.lookup_general(&p.name) != Some((3 + i as u8).into()))
        {
            return Ok(false);
        }
        let Some(index_kind) = self.local_source_fundamentals.get(plan.index).copied() else {
            return Ok(false);
        };
        let Some(count_kind) = self.parameter_source_fundamentals.get(plan.count).copied() else {
            return Ok(false);
        };
        if ![index_kind, count_kind].iter().all(|kind| {
            matches!(
                kind,
                SourceFundamentalType::SignedInteger | SourceFundamentalType::SignedLong
            )
        }) {
            return Ok(false);
        }
        let mixed_count_types = index_kind != count_kind;
        match self.behavior.byte_word_transfer_style {
            ByteWordTransferStyle::LegacyDependencyFirst
            | ByteWordTransferStyle::LegacyInterleaved => {
                self.emit_legacy_byte_word_transfer(&plan, mixed_count_types);
            }
            ByteWordTransferStyle::Mainline
            | ByteWordTransferStyle::GuardedGameCube
            | ByteWordTransferStyle::GuardedWii => {
                // Mainline keeps only the address high live through polling.
                // Its direct control displacement must fit independently of data.
                let (_, low) = crate::expressions::split_address(plan.address);
                let Some(control_offset) = low.checked_add(plan.control_offset) else {
                    return Ok(false);
                };
                self.emit_mainline_byte_word_transfer(&plan, mixed_count_types, control_offset);
            }
            ByteWordTransferStyle::Structured => unreachable!(),
        }
        Ok(true)
    }
}
