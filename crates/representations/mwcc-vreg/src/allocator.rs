//! The register allocator: assign every virtual register a physical home.
//!
//! The allocator works on *live intervals* — each virtual register's lifetime as
//! a `[definition, last use]` index range over the selected instruction stream —
//! plus the *pinned occupancies* where a physical register is already held by an
//! ABI value (a parameter or the return slot). It produces an [`Allocation`]
//! mapping each virtual register to a physical register such that values live at
//! the same time never share one.
//!
//! [`Allocator`] is a trait so the *policy* is swappable: a build that allocated
//! differently is a different allocator, not a forked code generator — the same
//! design principle as the version registry. [`LinearScan`] is the first policy:
//! lowest free register at each definition, which reproduces the spirit of the
//! current inline model (favor the lowest free register, avoid live ones).

use std::collections::HashMap;

use crate::constraints::RegisterConstraints;
use crate::register::{Class, VirtualRegister};

/// A virtual register's lifetime, as inclusive instruction indices: defined at
/// `start`, last read at `end`. A value used only at its definition has
/// `start == end`. `avoid` lists physical registers the allocator must not use
/// for this value — a placement hint from selection (e.g. "not the destination",
/// so mwcc's coalescing of a result-path temp onto the destination is matched).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveInterval {
    pub vreg: VirtualRegister,
    pub start: usize,
    pub end: usize,
    pub avoid: Vec<u8>,
}

impl LiveInterval {
    pub fn new(vreg: VirtualRegister, start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "an interval ends no earlier than it starts");
        LiveInterval { vreg, start, end, avoid: Vec::new() }
    }

    /// The same interval with a set of registers it must avoid.
    pub fn avoiding(mut self, avoid: Vec<u8>) -> Self {
        self.avoid = avoid;
        self
    }
}

/// A physical register held by a pinned value (an ABI parameter or the return
/// slot) over `[start, end]`. A virtual register whose lifetime overlaps this
/// cannot be assigned `register`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinnedOccupancy {
    pub register: u8,
    pub class: Class,
    pub start: usize,
    pub end: usize,
}

/// Whether two live ranges interfere — i.e. cannot share a register. The test is
/// *half-open*: a value defined at index `i` does not interfere with one whose
/// last use is `i`. That models the hardware reality that an instruction reads
/// its sources before writing its destination, so a result may reuse a source
/// register whose value dies at that instruction (`add r3, r3, r4`). Without
/// this, the allocator would never reuse a just-consumed register, and would
/// diverge from mwcc, which does so aggressively.
fn interferes(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start < b_end && b_start < a_end
}

/// The result of allocation: each virtual register's physical home.
#[derive(Debug, Clone, Default)]
pub struct Allocation {
    assignments: HashMap<u32, u8>,
}

impl Allocation {
    /// The physical register assigned to a virtual register.
    pub fn physical(&self, vreg: VirtualRegister) -> Option<u8> {
        self.assignments.get(&vreg.id).copied()
    }

    /// The number of virtual registers placed.
    pub fn len(&self) -> usize {
        self.assignments.len()
    }

    /// The callee-saved physical registers this allocation used, highest first,
    /// deduplicated — the set the function's prologue must save and its unwind
    /// metadata must count.
    pub fn assigned_callee_saved(&self, constraints: &RegisterConstraints) -> Vec<u8> {
        let mut used: Vec<u8> = self
            .assignments
            .values()
            .copied()
            .filter(|register| constraints.general_callee_saved.contains(register))
            .collect();
        used.sort_unstable_by(|left, right| right.cmp(left));
        used.dedup();
        used
    }

    pub fn is_empty(&self) -> bool {
        self.assignments.is_empty()
    }
}

/// Why allocation could not complete. Out of registers means the function needs
/// spilling — a real subsystem we have not built; the caller defers honestly
/// rather than emit wrong bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllocationError {
    /// No free register of `class` remained at instruction `at` — spilling needed.
    OutOfRegisters { class: Class, at: usize },
}

/// A register-allocation policy.
pub trait Allocator {
    fn allocate(
        &self,
        intervals: &[LiveInterval],
        pinned: &[PinnedOccupancy],
        calls: &[usize],
        constraints: &RegisterConstraints,
    ) -> Result<Allocation, AllocationError>;
}

/// Lowest-free linear scan: process definitions in order, and assign each value
/// the lowest-preference-order register not held by a value live at that point
/// (another still-live interval, or an overlapping pinned occupancy). Registers
/// free as their intervals expire and are reused. This reproduces the spirit of
/// the current inline model; mwcc-specific tie-breaks are tuned against the
/// oracle as selection is migrated onto it.
pub struct LinearScan;

impl Allocator for LinearScan {
    fn allocate(
        &self,
        intervals: &[LiveInterval],
        pinned: &[PinnedOccupancy],
        calls: &[usize],
        constraints: &RegisterConstraints,
    ) -> Result<Allocation, AllocationError> {
        let mut order: Vec<&LiveInterval> = intervals.iter().collect();
        // Stable lowest-first: by definition point, then by id for determinism.
        order.sort_by_key(|interval| (interval.start, interval.vreg.id));

        let mut allocation = Allocation::default();
        // Currently-live assigned intervals: (last-use index, physical register, class).
        let mut active: Vec<(usize, u8, Class)> = Vec::new();

        for interval in order {
            let class = interval.vreg.class;
            // Expire intervals whose last use is at or before this definition (a
            // register freed exactly here may be reused — half-open, see `interferes`).
            active.retain(|(end, _, _)| *end > interval.start);

            let mut busy: Vec<u8> = active
                .iter()
                .filter(|(_, _, active_class)| *active_class == class)
                .map(|(_, register, _)| *register)
                .collect();
            for occupancy in pinned {
                if occupancy.class == class
                    && interferes(occupancy.start, occupancy.end, interval.start, interval.end)
                {
                    busy.push(occupancy.register);
                }
            }

            // A value live ACROSS a call (strictly inside its range — a result defined
            // at the call or an argument last used at it needs no saving) must survive
            // the callee: it draws from the callee-saved pool, highest first (r31, r30,
            // …), exactly mwcc's assignment order. Floats keep the volatile pool until
            // an FPR callee-saved case is captured.
            let crosses_call = calls.iter().any(|call| interval.start < *call && *call < interval.end);
            let pool: &[u8] = if crosses_call && class == Class::General {
                &constraints.general_callee_saved
            } else {
                constraints.pool(class)
            };
            let choice = pool
                .iter()
                .copied()
                .find(|register| !busy.contains(register) && !interval.avoid.contains(register))
                .ok_or(AllocationError::OutOfRegisters { class, at: interval.start })?;

            allocation.assignments.insert(interval.vreg.id, choice);
            active.push((interval.end, choice, class));
        }

        Ok(allocation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register::Reg;

    fn gpr(id: u32, start: usize, end: usize) -> LiveInterval {
        LiveInterval::new(Reg::general(id).virtual_register().unwrap(), start, end)
    }

    #[test]
    fn assigned_callee_saved_reports_the_used_registers_highest_first() {
        let intervals = [gpr(0, 0, 4), gpr(1, 1, 4), gpr(2, 3, 4)];
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan.allocate(&intervals, &[], &[2], &constraints).unwrap();
        // v0/v1 cross the call (r31, r30); v2 does not (r3, volatile).
        assert_eq!(allocation.assigned_callee_saved(&constraints), vec![31, 30]);
    }

    #[test]
    fn a_call_crossing_interval_draws_from_the_callee_saved_pool_descending() {
        // v0 [0,4] and v1 [1,4] both cross the call at 2 -> r31 then r30; v2 [3,4]
        // is defined after it -> the volatile pool (r3).
        let intervals = [gpr(0, 0, 4), gpr(1, 1, 4), gpr(2, 3, 4)];
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan.allocate(&intervals, &[], &[2], &constraints).unwrap();
        assert_eq!(allocation.physical(Reg::general(0).virtual_register().unwrap()), Some(31));
        assert_eq!(allocation.physical(Reg::general(1).virtual_register().unwrap()), Some(30));
        assert_eq!(allocation.physical(Reg::general(2).virtual_register().unwrap()), Some(3));
    }
    fn fpr(id: u32, start: usize, end: usize) -> LiveInterval {
        LiveInterval::new(Reg::float(id).virtual_register().unwrap(), start, end)
    }
    fn phys(id: u32) -> VirtualRegister {
        Reg::general(id).virtual_register().unwrap()
    }

    #[test]
    fn non_overlapping_intervals_reuse_the_lowest_register() {
        let constraints = RegisterConstraints::gekko();
        let intervals = [gpr(0, 0, 2), gpr(1, 3, 5)];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(3));
        assert_eq!(allocation.physical(phys(1)), Some(3)); // r3 freed and reused
    }

    #[test]
    fn an_avoid_hint_pushes_a_value_off_a_register_it_would_otherwise_take() {
        let constraints = RegisterConstraints::gekko();
        // Without a hint this lone value takes r3; the hint forces it to r4.
        let intervals = [gpr(0, 0, 2).avoiding(vec![3])];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(4));
    }

    #[test]
    fn overlapping_intervals_get_distinct_registers() {
        let constraints = RegisterConstraints::gekko();
        let intervals = [gpr(0, 0, 4), gpr(1, 2, 6)];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(3));
        assert_eq!(allocation.physical(phys(1)), Some(4)); // must avoid r3
    }

    #[test]
    fn general_and_float_draw_from_separate_pools() {
        let constraints = RegisterConstraints::gekko();
        let intervals = [gpr(0, 0, 4), fpr(1, 0, 4)];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(3)); // r3
        let float_one = Reg::float(1).virtual_register().unwrap();
        assert_eq!(allocation.physical(float_one), Some(1)); // f1, not blocked by r3
    }

    #[test]
    fn a_virtual_avoids_a_pinned_abi_register_it_outlives() {
        let constraints = RegisterConstraints::gekko();
        // A parameter pinned to r3 over [0, 5]; a virtual live across it.
        let pinned = [PinnedOccupancy { register: 3, class: Class::General, start: 0, end: 5 }];
        let intervals = [gpr(0, 1, 4)];
        let allocation = LinearScan.allocate(&intervals, &pinned, &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(4)); // r3 is taken by the parameter
    }

    #[test]
    fn a_virtual_may_reuse_a_pinned_register_once_it_is_free() {
        let constraints = RegisterConstraints::gekko();
        let pinned = [PinnedOccupancy { register: 3, class: Class::General, start: 0, end: 2 }];
        let intervals = [gpr(0, 3, 5)]; // starts after the parameter's last use
        let allocation = LinearScan.allocate(&intervals, &pinned, &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(3));
    }

    #[test]
    fn running_out_of_registers_is_an_honest_error() {
        // A pool of one register, two simultaneously-live values.
        let constraints = RegisterConstraints { general_pool: vec![3], ..RegisterConstraints::gekko() };
        let intervals = [gpr(0, 0, 4), gpr(1, 1, 5)];
        let error = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap_err();
        assert_eq!(error, AllocationError::OutOfRegisters { class: Class::General, at: 1 });
    }
}
