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
use crate::description::{for_each_register, register_operands};
use crate::register::{Class, Reg, VirtualRegister, VIRTUAL_BASE};

/// The live ranges in a stream: one interval per virtual register, one occupancy
/// per physical register that appears.
#[derive(Debug, Default, Clone)]
pub struct Liveness {
    pub intervals: Vec<LiveInterval>,
    pub pinned: Vec<PinnedOccupancy>,
}

/// Compute liveness from a selected instruction stream.
pub fn analyze(instructions: &[Instruction]) -> Liveness {
    let mut first: HashMap<(Class, u8), usize> = HashMap::new();
    let mut last: HashMap<(Class, u8), usize> = HashMap::new();
    let mut order: Vec<(Class, u8)> = Vec::new(); // first-seen order, for determinism

    for (index, instruction) in instructions.iter().enumerate() {
        for operand in register_operands(instruction) {
            let key = (operand.class, operand.register);
            first.entry(key).or_insert_with(|| {
                order.push(key);
                index
            });
            last.insert(key, index);
        }
    }

    let mut liveness = Liveness::default();
    for key in order {
        let (class, value) = key;
        let (start, end) = (first[&key], last[&key]);
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
    fn analyze_separates_virtual_intervals_from_physical_occupancies() {
        let stream = [
            Instruction::Add { d: v(0), a: 3, b: 4 }, // v0 = r3 + r4
            Instruction::Add { d: v(1), a: 3, b: 4 }, // v1 = r3 + r4 (v0 still live)
            Instruction::Add { d: 3, a: v(0), b: v(1) }, // r3 = v0 + v1
        ];
        let liveness = analyze(&stream);
        assert_eq!(liveness.intervals.len(), 2);
        // v0 lives [0,2], v1 lives [1,2].
        assert_eq!((liveness.intervals[0].start, liveness.intervals[0].end), (0, 2));
        assert_eq!((liveness.intervals[1].start, liveness.intervals[1].end), (1, 2));
        // r3 is used early and defined late: one conservative occupancy [0,2].
        let r3 = liveness.pinned.iter().find(|p| p.register == 3).unwrap();
        assert_eq!((r3.start, r3.end), (0, 2));
    }

    #[test]
    fn allocate_then_apply_resolves_virtuals_avoiding_pinned() {
        let mut stream = [
            Instruction::Add { d: v(0), a: 3, b: 4 },
            Instruction::Add { d: v(1), a: 3, b: 4 },
            Instruction::Add { d: 3, a: v(0), b: v(1) },
        ];
        let constraints = RegisterConstraints::gekko();
        let liveness = analyze(&stream);
        let allocation = LinearScan.allocate(&liveness.intervals, &liveness.pinned, &constraints).unwrap();
        apply(&mut stream, &allocation);
        // v0 and v1 avoid r3/r4 (busy), taking the next free r5/r6.
        assert_eq!(
            stream,
            [
                Instruction::Add { d: 5, a: 3, b: 4 },
                Instruction::Add { d: 6, a: 3, b: 4 },
                Instruction::Add { d: 3, a: 5, b: 6 },
            ]
        );
    }

    #[test]
    fn a_freed_physical_register_is_reused_by_a_later_virtual() {
        let mut stream = [
            Instruction::AddImmediate { d: v(0), a: 3, immediate: 1 }, // v0 = r3 + 1
            Instruction::Or { a: 3, s: v(0), b: v(0) },                // r3 = v0  (mr)
            Instruction::AddImmediate { d: v(1), a: 4, immediate: 2 }, // v1 = r4 + 2
            Instruction::Or { a: 4, s: v(1), b: v(1) },                // r4 = v1  (mr)
        ];
        let constraints = RegisterConstraints::gekko();
        let liveness = analyze(&stream);
        let allocation = LinearScan.allocate(&liveness.intervals, &liveness.pinned, &constraints).unwrap();
        apply(&mut stream, &allocation);
        // v0 lives [0,1] alongside r3, so it avoids r3 -> r4. By v1's range [2,3]
        // r3 is free again, so v1 reuses it (r4 is now the busy one).
        assert_eq!(allocation.physical(Reg::general(0).virtual_register().unwrap()), Some(4));
        assert_eq!(allocation.physical(Reg::general(1).virtual_register().unwrap()), Some(3));
        assert!(stream.iter().all(|instruction| !matches!(instruction, Instruction::AddImmediate { d, .. } if Reg::is_virtual_field(*d))));
    }
}
