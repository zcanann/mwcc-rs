//! Liveness over a selected instruction stream, and applying an allocation back
//! to it.
//!
//! [`analyze`] turns a stream into the [`LiveInterval`]s and [`PinnedOccupancy`]s
//! the allocator consumes: each register the [machine description] reports is
//! tracked from its first appearance (its definition, in well-formed selection)
//! to its last use. Values at or above [`VIRTUAL_BASE`] are virtual registers to
//! place; below are physical registers already fixed (ABI/scratch), whose ranges
//! virtuals must avoid. [`apply`] does the reverse: once the allocator has chosen
//! a physical home for each virtual, it rewrites the fields in place.
//!
//! Together — selection → `analyze` → [`Allocator::allocate`] → `apply` — this is
//! the register-allocation pass. Its current precision limit: a physical register
//! reused for unrelated values (the classic `r3` parameter-then-result) is
//! treated as one occupancy spanning both, so virtuals avoid it more than
//! strictly necessary. That is conservative (never wrong), and tightened with
//! proper live-range splitting as the migration needs it.
//!
//! [machine description]: crate::for_each_register
//! [`Allocator::allocate`]: crate::Allocator::allocate

use std::collections::HashMap;

use mwcc_machine_code::Instruction;

use crate::allocator::{Allocation, LiveInterval, PinnedOccupancy};
use crate::description::{for_each_register, register_operands, RegisterRole};
use crate::register::{Class, Reg, VirtualRegister, VIRTUAL_BASE};

/// The live ranges in a stream: one interval per virtual register, one occupancy
/// per physical register that appears.
#[derive(Debug, Default, Clone)]
pub struct Liveness {
    pub intervals: Vec<LiveInterval>,
    pub pinned: Vec<PinnedOccupancy>,
    /// Instruction indices of CALLS (`bl` / `bctrl`): an interval crossing one
    /// must live in a callee-saved register.
    pub calls: Vec<usize>,
}

/// Compute liveness from a selected instruction stream.
///
/// Each register gets a separate live range per *definition* — the classic `r3`
/// that is a parameter, then a temporary, then the result has three. Within an
/// instruction reads precede the write, so a register both used and defined there
/// closes its old range at the use and opens a new one at the definition. A
/// register first seen as a use is a parameter, live from entry (index 0).
pub fn analyze(instructions: &[Instruction]) -> Liveness {
    // The currently-open range per register key, as (start, last-touched).
    let mut open: HashMap<(Class, u8), (usize, usize)> = HashMap::new();
    let mut ranges: Vec<((Class, u8), usize, usize)> = Vec::new();

    let mut calls: Vec<usize> = Vec::new();
    for (index, instruction) in instructions.iter().enumerate() {
        if matches!(instruction, Instruction::BranchAndLink { .. } | Instruction::BranchToCountRegisterAndLink) {
            calls.push(index);
        }
        let operands = register_operands(instruction);
        // Uses first: extend the open range (opening one from entry if this is a
        // parameter's first appearance).
        for operand in operands.iter().filter(|operand| operand.role == RegisterRole::Use) {
            let entry = open.entry((operand.class, operand.register)).or_insert((0, index));
            entry.1 = index;
        }
        // Then definitions: a PHYSICAL register's old value ends and a new range
        // begins (the classic r3 parameter/temporary/result reuse). A VIRTUAL
        // register redefined mid-stream instead EXTENDS its one range: the vreg is a
        // single value home — mwcc keeps the same physical register across a
        // redefinition (a base register reused for the effective address) — so the
        // allocator must too; a split would let the linear scan hand the two halves
        // different homes.
        for operand in operands.iter().filter(|operand| operand.role == RegisterRole::Define) {
            let key = (operand.class, operand.register);
            if Reg::is_virtual_field(operand.register) {
                let entry = open.entry(key).or_insert((index, index));
                entry.1 = index;
            } else {
                if let Some((start, end)) = open.remove(&key) {
                    ranges.push((key, start, end));
                }
                open.insert(key, (index, index));
            }
        }
    }
    for (key, (start, end)) in open {
        ranges.push((key, start, end));
    }
    // Deterministic order: by start, then register, then class.
    ranges.sort_by_key(|((class, register), start, _)| (*start, *register, *class as u8));

    let mut liveness = Liveness::default();
    liveness.calls = calls;
    for ((class, value), start, end) in ranges {
        if Reg::is_virtual_field(value) {
            let vreg = VirtualRegister::new((value - VIRTUAL_BASE) as u32, class);
            liveness.intervals.push(LiveInterval::new(vreg, start, end));
        } else {
            liveness.pinned.push(PinnedOccupancy { register: value, class, start, end });
        }
    }
    liveness
}

/// Rewrite every virtual register in `instructions` to its allocated physical
/// register, in place. Fields the allocation does not cover (physical registers,
/// or virtuals it could not place) are left untouched.
pub fn apply(instructions: &mut [Instruction], allocation: &Allocation) {
    for instruction in instructions.iter_mut() {
        for_each_register(instruction, |_role, class, field| {
            if Reg::is_virtual_field(*field) {
                let vreg = VirtualRegister::new((*field - VIRTUAL_BASE) as u32, class);
                if let Some(physical) = allocation.physical(vreg) {
                    *field = physical;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::allocator::{Allocator, LinearScan};
    use crate::constraints::RegisterConstraints;
    use crate::register::Reg;

    /// A virtual register's field value (id 0 -> VIRTUAL_BASE).
    fn v(id: u32) -> u8 {
        Reg::general(id).to_field()
    }

    #[test]
    fn a_reused_physical_register_gets_one_occupancy_per_definition() {
        let stream = [
            Instruction::AddImmediate { d: 3, a: 4, immediate: 1 }, // r3 = r4 + 1 (def r3)
            Instruction::Or { a: 5, s: 3, b: 3 },                   // r5 = r3 (r3's last use)
            Instruction::AddImmediate { d: 3, a: 5, immediate: 2 }, // r3 = r5 + 2 (redef r3)
        ];
        let liveness = analyze(&stream);
        let r3: Vec<_> = liveness
            .pinned
            .iter()
            .filter(|occupancy| occupancy.register == 3)
            .map(|occupancy| (occupancy.start, occupancy.end))
            .collect();
        // Two distinct lives of r3: the first value [0,1], the redefinition [2,2].
        assert_eq!(r3, [(0, 1), (2, 2)]);
    }

    #[test]
    fn a_redefined_virtual_keeps_one_range_spanning_the_redefinition() {
        // v0 = base high; a value materializes between v0's use and its
        // redefinition as the effective address (the offset≠0 store shape):
        //   lis v0; addi r3,v0; li v1,C; add v0,r3,r0; stw v1,-4(v0)
        // v0 must span [0,4] (one home across the redefinition at 3), so v1's
        // overlapping range gets the NEXT register — matching mwcc's r4/r5.
        let stream = [
            Instruction::AddImmediate { d: v(0), a: 0, immediate: 0 },  // lis-ish: def v0
            Instruction::AddImmediate { d: 3, a: v(0), immediate: 0 },  // use v0
            Instruction::AddImmediate { d: v(1), a: 0, immediate: 9 },  // def v1
            Instruction::Add { d: v(0), a: 3, b: 0 },                   // REDEFINE v0
            Instruction::StoreWord { s: v(1), a: v(0), offset: -4 },    // use both
        ];
        let liveness = analyze(&stream);
        let v0 = liveness.intervals.iter().find(|interval| interval.vreg.id == 0).unwrap();
        assert_eq!((v0.start, v0.end), (0, 4));
        let constraints = RegisterConstraints::gekko();
        let allocation = LinearScan.allocate(&liveness.intervals, &liveness.pinned, &liveness.calls, &constraints).unwrap();
        let home0 = allocation.physical(VirtualRegister::new(0, Class::General)).unwrap();
        let home1 = allocation.physical(VirtualRegister::new(1, Class::General)).unwrap();
        assert_ne!(home0, home1);
        assert!(home0 < home1, "the earlier-defined base takes the lower register");
    }

    #[test]
    fn a_virtual_reuses_a_source_register_that_dies_at_its_definition() {
        let mut stream = [
            Instruction::Add { d: v(0), a: 3, b: 4 }, // v0 = r3 + r4 (r3,r4 die here)
            Instruction::Or { a: 3, s: v(0), b: v(0) }, // r3 = v0
        ];
        let constraints = RegisterConstraints::gekko();
        let liveness = analyze(&stream);
        let allocation = LinearScan.allocate(&liveness.intervals, &liveness.pinned, &liveness.calls, &constraints).unwrap();
        apply(&mut stream, &allocation);
        // r3/r4 are read at instruction 0 and dead after, so v0 may take r3 — the
        // half-open rule in action (a result reusing a just-consumed source).
        assert_eq!(allocation.physical(Reg::general(0).virtual_register().unwrap()), Some(3));
        assert_eq!(stream[0], Instruction::Add { d: 3, a: 3, b: 4 });
    }
}
