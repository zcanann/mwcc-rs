//! Reconcile source-variable provenance with physical register allocation.
//!
//! Instruction rewriting and debug provenance consume the same allocation.
//! Keeping this bridge separate prevents late debug lowering from interpreting
//! encoded virtual-register fields as physical register numbers.

use crate::generator::{Location, ValueClass};
use mwcc_vreg::{Allocation, Class, Reg};
use std::collections::HashMap;

pub(crate) fn reconcile_variable_locations(
    locations: &mut HashMap<String, Location>,
    allocation: &Allocation,
) {
    for location in locations.values_mut() {
        let class = match location.class {
            ValueClass::General => Class::General,
            ValueClass::Float => Class::Float,
        };
        let Reg::Virtual(register) = Reg::from_field(location.register, class) else {
            continue;
        };
        if let Some(physical) = allocation.physical(register) {
            location.register = physical;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_vreg::{Allocator, LinearScan, LiveInterval, RegisterConstraints, VirtualRegister};

    fn location(class: ValueClass, register: Reg) -> Location {
        Location {
            class,
            register: register.to_field(),
            signed: true,
            width: 32,
            pointee: None,
            stride: None,
        }
    }

    #[test]
    fn maps_live_virtual_locations_and_preserves_physical_or_dead_ones() {
        let live = VirtualRegister::new(4, Class::General);
        let dead = VirtualRegister::new(9, Class::General);
        let allocation = LinearScan
            .allocate(
                &[LiveInterval::new(live, 0, 2)],
                &[],
                &[],
                &RegisterConstraints::gekko(),
            )
            .expect("allocation");
        let mut locations = HashMap::from([
            ("live".into(), location(ValueClass::General, Reg::Virtual(live))),
            ("dead".into(), location(ValueClass::General, Reg::Virtual(dead))),
            ("pinned".into(), location(ValueClass::General, Reg::Physical(19))),
        ]);

        reconcile_variable_locations(&mut locations, &allocation);

        assert_eq!(locations["live"].register, 3);
        assert_eq!(locations["dead"].register, Reg::Virtual(dead).to_field());
        assert_eq!(locations["pinned"].register, 19);
    }
}
