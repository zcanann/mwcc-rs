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

use std::collections::{HashMap, HashSet};

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
    /// CONSUMER-TREE COLORING (the __va_arg policy spec, #1): the register this
    /// value's consumer wants it in — a return-sum leaf's in-place home (`a+b` ->
    /// a prefers r3, b prefers r0), taken when free at allocation, else the pool
    /// order proceeds. `None` (the default) is the plain lowest-free behavior.
    pub prefer: Option<u8>,
    /// Prefer an already allocated value's home when legal. Explicit physical
    /// preferences take precedence; missing or cross-class sources are ignored.
    pub prefer_virtual: Option<VirtualRegister>,
    /// Control-flow-aware live slots (`2*i` before instruction `i`, `2*i+1`
    /// after it). `None` preserves the interval-only behavior for hand-authored
    /// allocator inputs; liveness analysis always supplies these slots.
    pub live_slots: Option<Vec<usize>>,
}

impl LiveInterval {
    pub fn new(vreg: VirtualRegister, start: usize, end: usize) -> Self {
        debug_assert!(start <= end, "an interval ends no earlier than it starts");
        LiveInterval {
            vreg,
            start,
            end,
            avoid: Vec::new(),
            prefer: None,
            prefer_virtual: None,
            live_slots: None,
        }
    }

    /// The same interval with a set of registers it must avoid.
    pub fn avoiding(mut self, avoid: Vec<u8>) -> Self {
        self.avoid = avoid;
        self
    }

    /// The same interval with a consumer-tree preferred register.
    pub fn preferring(mut self, register: u8) -> Self {
        self.prefer = Some(register);
        self
    }
}

/// A physical register held by a pinned value (an ABI parameter or the return
/// slot) over `[start, end]`. A virtual register whose lifetime overlaps this
/// cannot be assigned `register`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedOccupancy {
    pub register: u8,
    pub class: Class,
    pub start: usize,
    pub end: usize,
    /// Control-flow-aware occupancy slots, when produced by liveness analysis.
    pub live_slots: Option<Vec<usize>>,
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

fn slots_interfere(left: &[usize], right: &[usize]) -> bool {
    let (mut left_index, mut right_index) = (0, 0);
    while left_index < left.len() && right_index < right.len() {
        match left[left_index].cmp(&right[right_index]) {
            std::cmp::Ordering::Less => left_index += 1,
            std::cmp::Ordering::Greater => right_index += 1,
            std::cmp::Ordering::Equal => return true,
        }
    }
    false
}

fn intervals_interfere(left: &LiveInterval, right: &LiveInterval) -> bool {
    match (&left.live_slots, &right.live_slots) {
        (Some(left), Some(right)) => slots_interfere(left, right),
        _ => interferes(left.start, left.end, right.start, right.end),
    }
}

fn pinned_interferes(occupancy: &PinnedOccupancy, interval: &LiveInterval) -> bool {
    match (&occupancy.live_slots, &interval.live_slots) {
        (Some(pinned), Some(virtual_slots)) => slots_interfere(pinned, virtual_slots),
        _ => interferes(occupancy.start, occupancy.end, interval.start, interval.end),
    }
}

fn crosses_call(interval: &LiveInterval, calls: &[usize]) -> bool {
    match &interval.live_slots {
        Some(slots) => calls.iter().any(|call| {
            slots.binary_search(&(2 * call)).is_ok()
                && slots.binary_search(&(2 * call + 1)).is_ok()
        }),
        None => calls
            .iter()
            .any(|call| interval.start < *call && *call < interval.end),
    }
}

/// The result of allocation: each virtual register's physical home.
#[derive(Debug, Clone, Default)]
pub struct Allocation {
    assignments: HashMap<VirtualRegister, u8>,
}

impl Allocation {
    /// The physical register assigned to a virtual register.
    pub fn physical(&self, vreg: VirtualRegister) -> Option<u8> {
        self.assignments.get(&vreg).copied()
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
            .iter()
            .filter(|(virtual_register, register)| {
                virtual_register.class == Class::General
                    && constraints.general_callee_saved.contains(register)
            })
            .map(|(_, register)| *register)
            .collect();
        used.sort_unstable_by(|left, right| right.cmp(left));
        used.dedup();
        used
    }

    /// Callee-saved FPRs selected by this allocation, highest first.
    pub fn assigned_float_callee_saved(
        &self,
        constraints: &RegisterConstraints,
    ) -> Vec<u8> {
        let mut used: Vec<u8> = self
            .assignments
            .iter()
            .filter(|(virtual_register, register)| {
                virtual_register.class == Class::Float
                    && constraints.float_callee_saved.contains(register)
            })
            .map(|(_, register)| *register)
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

        allocate_in_order(order, pinned, calls, constraints)
    }
}

/// Allocate each nominated expression group from its consumer toward its inputs.
/// Groups are visited at their earliest definition in the ordinary scan. Only
/// allocation order changes: full CFG live slots, pinned homes, call survival,
/// register preferences, and legal pools retain the same meaning as LinearScan.
/// Missing members are ignored, and the first group owns overlapping members.
pub struct ConsumerFirstScan<'a> {
    pub groups: &'a [Vec<VirtualRegister>],
}

impl Allocator for ConsumerFirstScan<'_> {
    fn allocate(
        &self,
        intervals: &[LiveInterval],
        pinned: &[PinnedOccupancy],
        calls: &[usize],
        constraints: &RegisterConstraints,
    ) -> Result<Allocation, AllocationError> {
        let mut linear: Vec<_> = intervals.iter().collect();
        linear.sort_by_key(|interval| (interval.start, interval.vreg.id));
        let by_register: HashMap<_, _> = intervals.iter().map(|i| (i.vreg, i)).collect();
        let mut owner = HashMap::new();
        for (index, group) in self.groups.iter().enumerate() {
            for register in group {
                if by_register.contains_key(register) {
                    owner.entry(*register).or_insert(index);
                }
            }
        }
        let mut order = Vec::with_capacity(intervals.len());
        let mut seen = HashSet::new();
        for interval in linear {
            if seen.contains(&interval.vreg) {
                continue;
            }
            if let Some(&index) = owner.get(&interval.vreg) {
                for register in &self.groups[index] {
                    if owner.get(register) == Some(&index) && seen.insert(*register) {
                        order.push(by_register[register]);
                    }
                }
            } else {
                seen.insert(interval.vreg);
                order.push(interval);
            }
        }
        allocate_in_order(order, pinned, calls, constraints)
    }
}

fn allocate_in_order(
    order: Vec<&LiveInterval>,
    pinned: &[PinnedOccupancy],
    calls: &[usize],
    constraints: &RegisterConstraints,
) -> Result<Allocation, AllocationError> {
    let mut allocation = Allocation::default();
    // Currently-live assigned intervals: (last-use index, physical register, class).
    let mut active: Vec<(&LiveInterval, u8)> = Vec::new();

    // A group can visit a later definition before an earlier one. Expire only
    // values that end before *every* remaining definition, not this visit.
    let mut remaining_start = vec![usize::MAX; order.len()];
    let mut earliest = usize::MAX;
    for (index, interval) in order.iter().enumerate().rev() {
        earliest = earliest.min(interval.start);
        remaining_start[index] = earliest;
    }
    for (index, interval) in order.into_iter().enumerate() {
        let class = interval.vreg.class;
        // Keep all assignments that can interfere with any future visit.
        active.retain(|(active_interval, _)| active_interval.end > remaining_start[index]);

        let mut busy: Vec<u8> = active
            .iter()
            .filter(|(active_interval, _)| {
                active_interval.vreg.class == class
                    && intervals_interfere(active_interval, interval)
            })
            .map(|(_, register)| *register)
            .collect();
        for occupancy in pinned {
            if occupancy.class == class && pinned_interferes(occupancy, interval) {
                busy.push(occupancy.register);
            }
        }

        // A value live ACROSS a call (strictly inside its range — a result defined
        // at the call or an argument last used at it needs no saving) must survive
        // the callee, so it draws only from the callee-saved pool. Other values
        // prefer the volatile pool but may spill into callee-saved registers when
        // pressure exceeds it; the frame reconciler adds the required saves.
        let crosses_call = crosses_call(interval, calls);
        let pool: Vec<u8> = match (crosses_call, class) {
            (true, Class::General) => constraints.general_callee_saved.clone(),
            (true, Class::Float) => constraints.float_callee_saved.clone(),
            (false, Class::General) => constraints
                .general_pool
                .iter()
                .chain(&constraints.general_callee_saved)
                .copied()
                .collect(),
            (false, Class::Float) => constraints
                .float_pool
                .iter()
                .chain(&constraints.float_callee_saved)
                .copied()
                .collect(),
        };
        // The consumer-tree preference wins when free (policy #1); the pool
        // order is the fallback. The class SCRATCH (r0) is a legal preference —
        // the measured double-duty home (policy #3) — though it is outside the
        // default pool; it is never honored across a call (the scratch dies
        // there). Any other out-of-pool wish is ignored — correctness first.
        let preferred = interval
            .prefer
            .or_else(|| {
                interval
                    .prefer_virtual
                    .filter(|source| source.class == class)
                    .and_then(|source| allocation.physical(source))
            })
            .filter(|register| {
                let scratch_home = *register == constraints.scratch(class) && !crosses_call;
                (pool.contains(register) || scratch_home)
                    && !busy.contains(register)
                    && !interval.avoid.contains(register)
            });
        let choice = match preferred {
            Some(register) => register,
            None => pool
                .iter()
                .copied()
                .find(|register| !busy.contains(register) && !interval.avoid.contains(register))
                .ok_or(AllocationError::OutOfRegisters {
                    class,
                    at: interval.start,
                })?,
        };

        allocation.assignments.insert(interval.vreg, choice);
        active.push((interval, choice));
    }

    Ok(allocation)
}

/// The DESCENDING store-fill policy (measured, fires 851-856): mwcc allocates a
/// literal-fill's materializations from the TOP of a window down — for an N-store
/// aggregate fill, v0 = r(N+2), the base at r(N+1), later values descending with
/// r3 recycled after the base migrates off it, and the LAST value always r0 (the
/// scratch, legal because it dies at its store). Expressed generally: assign each
/// value the HIGHEST free pool register below `top`; when nothing in the window is
/// free the scratch is used for a value that does not cross a call. LinearScan's
/// interference/pinning/call rules are shared; only the pool direction differs.
pub struct DescendingScan {
    /// The window's top register (inclusive) — r(N+2) for an N-store fill.
    pub top: u8,
}

impl Allocator for DescendingScan {
    fn allocate(
        &self,
        intervals: &[LiveInterval],
        pinned: &[PinnedOccupancy],
        calls: &[usize],
        constraints: &RegisterConstraints,
    ) -> Result<Allocation, AllocationError> {
        let mut order: Vec<&LiveInterval> = intervals.iter().collect();
        order.sort_by_key(|interval| (interval.start, interval.vreg.id));

        let mut allocation = Allocation::default();
        let mut active: Vec<(&LiveInterval, u8)> = Vec::new();

        for interval in order {
            let class = interval.vreg.class;
            active.retain(|(active_interval, _)| active_interval.end > interval.start);

            let mut busy: Vec<u8> = active
                .iter()
                .filter(|(active_interval, _)| {
                    active_interval.vreg.class == class
                        && intervals_interfere(active_interval, interval)
                })
                .map(|(_, register)| *register)
                .collect();
            for occupancy in pinned {
                if occupancy.class == class && pinned_interferes(occupancy, interval) {
                    busy.push(occupancy.register);
                }
            }

            let crosses_call = crosses_call(interval, calls);
            let pool: Vec<u8> = if crosses_call && class == Class::General {
                constraints.general_callee_saved.clone()
            } else {
                // The descending window: pool registers <= top, HIGHEST first, then
                // the scratch as the final fallback for a non-call-crossing value.
                let mut window: Vec<u8> = constraints
                    .pool(class)
                    .iter()
                    .copied()
                    .filter(|register| *register <= self.top)
                    .collect();
                window.sort_unstable_by(|left, right| right.cmp(left));
                if !crosses_call {
                    window.push(constraints.scratch(class));
                }
                window
            };
            let choice = pool
                .iter()
                .copied()
                .find(|register| !busy.contains(register) && !interval.avoid.contains(register))
                .ok_or(AllocationError::OutOfRegisters { class, at: interval.start })?;

            allocation.assignments.insert(interval.vreg, choice);
            active.push((interval, choice));
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
    fn copy_affinity_reuses_a_dead_source_home_when_legal() {
        let source = gpr(0, 0, 2).preferring(6);
        let mut copy = gpr(1, 2, 4);
        copy.prefer_virtual = Some(source.vreg);
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan
            .allocate(&[source.clone(), copy.clone()], &[], &[], &constraints)
            .unwrap();
        assert_eq!(allocation.physical(copy.vreg), Some(6));
        copy.avoid.push(6);
        let allocation = LinearScan
            .allocate(&[source, copy.clone()], &[], &[], &constraints)
            .unwrap();
        assert_eq!(allocation.physical(copy.vreg), Some(3));
    }

    #[test]
    fn copy_affinity_never_overrides_interference_or_call_survival() {
        let source = gpr(0, 0, 4).preferring(6);
        let mut copy = gpr(1, 2, 5);
        copy.prefer_virtual = Some(source.vreg);
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan
            .allocate(&[source.clone(), copy.clone()], &[], &[], &constraints)
            .unwrap();
        assert_eq!(allocation.physical(copy.vreg), Some(3));
        let source = gpr(0, 0, 2).preferring(6);
        let allocation = LinearScan
            .allocate(&[source, copy.clone()], &[], &[3], &constraints)
            .unwrap();
        assert_eq!(allocation.physical(copy.vreg), Some(31));
    }

    #[test]
    fn unknown_copy_sources_fall_back_and_explicit_homes_take_precedence() {
        let mut copy = gpr(1, 0, 2);
        copy.prefer_virtual = Some(gpr(0, 0, 1).vreg);
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan
            .allocate(&[copy.clone()], &[], &[], &constraints)
            .unwrap();
        assert_eq!(allocation.physical(copy.vreg), Some(3));
        let source = gpr(0, 0, 2).preferring(6);
        copy.start = 2;
        copy.end = 4;
        copy.prefer = Some(7);
        let allocation = LinearScan
            .allocate(&[source, copy.clone()], &[], &[], &constraints)
            .unwrap();
        assert_eq!(allocation.physical(copy.vreg), Some(7));
    }

    #[test]
    fn consumer_groups_reuse_dead_inputs_without_inventing_interference() {
        let intervals = [gpr(0, 2, 5), gpr(1, 3, 5), gpr(2, 5, 6)];
        let groups = vec![vec![
            intervals[2].vreg,
            intervals[1].vreg,
            intervals[0].vreg,
        ]];
        let constraints = RegisterConstraints::gekko();
        for (delta_end, expected) in [(2, [5, 4, 4]), (4, [6, 5, 4])] {
            let pinned = [
                PinnedOccupancy {
                    register: 3,
                    class: Class::General,
                    start: 0,
                    end: 7,
                    live_slots: None,
                },
                PinnedOccupancy {
                    register: 4,
                    class: Class::General,
                    start: 0,
                    end: delta_end,
                    live_slots: None,
                },
            ];
            let allocation = ConsumerFirstScan { groups: &groups }
                .allocate(&intervals, &pinned, &[], &constraints)
                .unwrap();
            for (interval, expected) in intervals.iter().zip(expected) {
                assert_eq!(allocation.physical(interval.vreg), Some(expected));
            }
        }
    }

    #[test]
    fn a_later_visit_cannot_expire_values_needed_by_an_earlier_definition() {
        let intervals = [gpr(0, 0, 3), gpr(1, 10, 11).preferring(4), gpr(2, 1, 2)];
        let groups = vec![vec![intervals[1].vreg, intervals[2].vreg]];
        let allocation = ConsumerFirstScan { groups: &groups }
            .allocate(&intervals, &[], &[], &RegisterConstraints::gekko())
            .unwrap();
        assert_eq!(allocation.physical(intervals[0].vreg), Some(3));
        assert_eq!(allocation.physical(intervals[1].vreg), Some(4));
        // v0 is still live at v2 despite the intervening visit to position 10.
        assert_eq!(allocation.physical(intervals[2].vreg), Some(4));
    }

    #[test]
    fn consumer_order_keeps_cfg_slots_and_call_survival_authoritative() {
        let mut intervals = [gpr(0, 0, 5), gpr(1, 0, 5), gpr(2, 2, 5).preferring(0)];
        intervals[0].live_slots = Some(vec![0, 1]);
        intervals[1].live_slots = Some(vec![8, 9]);
        let groups = vec![vec![
            intervals[2].vreg,
            intervals[1].vreg,
            intervals[0].vreg,
        ]];
        let allocation = ConsumerFirstScan { groups: &groups }
            .allocate(&intervals, &[], &[3], &RegisterConstraints::gekko())
            .unwrap();
        assert_eq!(allocation.physical(intervals[0].vreg), Some(3));
        assert_eq!(allocation.physical(intervals[1].vreg), Some(3));
        assert_eq!(allocation.physical(intervals[2].vreg), Some(31));
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
    fn a_preferred_register_wins_when_free_and_is_ignored_when_busy() {
        // b prefers r0 (the consumer-tree scratch home) over the default r3.
        let constraints = RegisterConstraints::gekko();
        let intervals = vec![gpr(1, 0, 4), gpr(2, 1, 4).preferring(0)];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        assert_eq!(allocation.physical(Reg::general(2).virtual_register().unwrap()), Some(0));
        // The same preference is IGNORED when another live value holds it.
        let intervals = vec![gpr(1, 0, 4).preferring(0), gpr(2, 1, 4).preferring(0)];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        let first = allocation.physical(Reg::general(1).virtual_register().unwrap()).unwrap();
        let second = allocation.physical(Reg::general(2).virtual_register().unwrap()).unwrap();
        assert_eq!(first, 0);
        assert_ne!(second, 0);
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

    #[test]
    fn a_float_crossing_a_call_draws_from_the_fpr_callee_saved_pool() {
        let intervals = [fpr(0, 0, 4), fpr(1, 1, 4), fpr(2, 3, 4)];
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan.allocate(&intervals, &[], &[2], &constraints).unwrap();
        assert_eq!(
            allocation.physical(Reg::float(0).virtual_register().unwrap()),
            Some(31)
        );
        assert_eq!(
            allocation.physical(Reg::float(1).virtual_register().unwrap()),
            Some(30)
        );
        assert_eq!(
            allocation.physical(Reg::float(2).virtual_register().unwrap()),
            Some(1)
        );
        assert_eq!(
            allocation.assigned_float_callee_saved(&constraints),
            vec![31, 30]
        );
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
        let pinned = [PinnedOccupancy {
            register: 3,
            class: Class::General,
            start: 0,
            end: 5,
            live_slots: None,
        }];
        let intervals = [gpr(0, 1, 4)];
        let allocation = LinearScan.allocate(&intervals, &pinned, &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(4)); // r3 is taken by the parameter
    }

    #[test]
    fn a_virtual_may_reuse_a_pinned_register_once_it_is_free() {
        let constraints = RegisterConstraints::gekko();
        let pinned = [PinnedOccupancy {
            register: 3,
            class: Class::General,
            start: 0,
            end: 2,
            live_slots: None,
        }];
        let intervals = [gpr(0, 3, 5)]; // starts after the parameter's last use
        let allocation = LinearScan.allocate(&intervals, &pinned, &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(3));
    }

    #[test]
    fn descending_scan_reproduces_the_three_store_fill_formula() {
        // The fire-854 aggregate fill, as pure intervals over the measured emission
        // order [lis, li v0, addi base, li v1, li v2, stw0, stw1, stw2]:
        let constraints = RegisterConstraints::gekko();
        let intervals = [
            gpr(0, 1, 5), // v0: defined at its li, dies at store 0
            gpr(1, 2, 7), // base: defined at the addi, live through the last store
            gpr(2, 3, 6), // v1
            gpr(3, 4, 7), // v2
        ];
        let allocation = DescendingScan { top: 5 }.allocate(&intervals, &[], &[], &constraints).unwrap();
        // Measured: v0 = r5, base = r4, v1 = r3 (recycled), v2 = r0 (the scratch).
        assert_eq!(allocation.physical(phys(0)), Some(5));
        assert_eq!(allocation.physical(phys(1)), Some(4));
        assert_eq!(allocation.physical(phys(2)), Some(3));
        assert_eq!(allocation.physical(phys(3)), Some(0));
    }

    #[test]
    fn descending_scan_reproduces_the_four_store_fill_formula() {
        // Fire 855's N=4: [lis, li v0, addi base, li v1, li v2, li v3, stw x4].
        let constraints = RegisterConstraints::gekko();
        let intervals = [
            gpr(0, 1, 6),  // v0
            gpr(1, 2, 9),  // base
            gpr(2, 3, 7),  // v1
            gpr(3, 4, 8),  // v2
            gpr(4, 5, 9),  // v3
        ];
        let allocation = DescendingScan { top: 6 }.allocate(&intervals, &[], &[], &constraints).unwrap();
        // Measured: v0 = r6, base = r5, v1 = r4, v2 = r3, v3 = r0.
        assert_eq!(allocation.physical(phys(0)), Some(6));
        assert_eq!(allocation.physical(phys(1)), Some(5));
        assert_eq!(allocation.physical(phys(2)), Some(4));
        assert_eq!(allocation.physical(phys(3)), Some(3));
        assert_eq!(allocation.physical(phys(4)), Some(0));
    }

    #[test]
    fn running_out_of_registers_is_an_honest_error() {
        // A pool of one register, two simultaneously-live values.
        let constraints = RegisterConstraints {
            general_pool: vec![3],
            general_callee_saved: Vec::new(),
            ..RegisterConstraints::gekko()
        };
        let intervals = [gpr(0, 0, 4), gpr(1, 1, 5)];
        let error = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap_err();
        assert_eq!(error, AllocationError::OutOfRegisters { class: Class::General, at: 1 });
    }

    #[test]
    fn pressure_may_use_callee_saved_registers_without_crossing_a_call() {
        let constraints = RegisterConstraints {
            general_pool: vec![3],
            general_callee_saved: vec![31],
            ..RegisterConstraints::gekko()
        };
        let intervals = [gpr(0, 0, 4), gpr(1, 1, 5)];
        let allocation = LinearScan.allocate(&intervals, &[], &[], &constraints).unwrap();
        assert_eq!(allocation.physical(phys(0)), Some(3));
        assert_eq!(allocation.physical(phys(1)), Some(31));
    }
}
