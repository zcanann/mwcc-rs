//! Physical homes that remain semantically live across a structured store.
//!
//! ABI result registers can hold named locals without an emitted definition
//! visible to virtual-register liveness. An intervening memory update must not
//! allocate one of those homes before the local's later read.

use super::*;
use super::structured::value_read_before_redefinition;

impl Generator {
    pub(super) fn reserve_live_physical_homes(
        &mut self,
        function: &Function,
        remaining: &[Statement],
    ) -> Vec<u8> {
        function
            .locals
            .iter()
            .map(|local| local.name.as_str())
            .chain(
                function
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str()),
            )
            .filter(|name| value_read_before_redefinition(remaining, name))
            .filter_map(|name| self.locations.get(name))
            .filter_map(|location| {
                (!mwcc_vreg::Reg::is_virtual_field(location.register))
                    .then_some(location.register)
            })
            .filter(|register| self.reserved.insert(*register))
            .collect()
    }

    pub(super) fn release_reserved_physical_homes(&mut self, registers: Vec<u8>) {
        for register in registers {
            self.reserved.remove(&register);
        }
    }
}
