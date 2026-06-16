//! The instruction scheduler (Phase E).
//!
//! mwcceppc reorders instructions within a basic block to suit the Gekko's
//! in-order, dual-issue pipeline — it issues independent long-latency operations
//! early to hide their latency. So two streams that compute the same values in a
//! different order are both correct but only one is byte-identical to mwcc; this
//! pass reproduces mwcc's order.
//!
//! It works on the selected (post-allocation) instruction stream. Pure-register
//! arithmetic between *barriers* (memory accesses, compares, branches, link-
//! register moves) is reordered by list scheduling over the data-dependence DAG;
//! barriers stay put, so nothing crosses a side effect or a control-flow edge.
//! The pass is conservative by construction — it only permutes instructions whose
//! relative order the data dependences leave free.
//!
//! [`schedule`] returns the `old index -> new index` permutation so the caller can
//! remap anything keyed by instruction position (relocations). To keep v1 simple
//! and provably safe, a function containing a forward branch is left untouched
//! (its branch targets are instruction indices that a reorder would invalidate).

use mwcc_machine_code::Instruction;

use crate::description::{register_operands, RegisterRole};
use crate::register::Class;

/// Whether an instruction is a scheduling barrier: a memory access, a compare
/// (defines the condition register), a branch, or a link-register move. The
/// scheduler never moves these, and never moves another instruction across one,
/// so side effects and condition/control state keep their order.
fn is_barrier(instruction: &Instruction) -> bool {
    use Instruction::*;
    matches!(
        instruction,
        StoreWord { .. } | StoreByte { .. } | StoreHalfword { .. } | StoreFloatSingle { .. }
            | StoreFloatDouble { .. } | StoreWordWithUpdate { .. } | StoreWordIndexed { .. }
            | StoreByteIndexed { .. } | StoreHalfwordIndexed { .. } | StoreFloatSingleIndexed { .. }
            | LoadWord { .. } | LoadByteZero { .. } | LoadHalfwordZero { .. } | LoadHalfwordAlgebraic { .. }
            | LoadFloatSingle { .. } | LoadFloatDouble { .. } | LoadWordIndexed { .. }
            | LoadByteZeroIndexed { .. } | LoadHalfwordZeroIndexed { .. } | LoadHalfwordAlgebraicIndexed { .. }
            | LoadFloatSingleIndexed { .. }
            | FloatCompareOrdered { .. } | CompareWord { .. } | CompareWordImmediate { .. }
            | CompareLogicalWord { .. } | CompareLogicalWordImmediate { .. }
            | BranchConditionalForward { .. } | BranchConditionalToLinkRegister { .. } | Branch { .. }
            | BranchToLinkRegister | BranchToCountRegister | BranchAndLink { .. }
            | MoveFromLinkRegister { .. } | MoveToLinkRegister { .. } | MoveToCountRegister { .. }
    )
}

/// Hoist the epilogue's saved-LR reload (`lwz r0, frame+4(r1)`) up to immediately
/// after the last call, ahead of any post-call computation — reproducing mwcc,
/// which issues that load early so its latency overlaps the post-call work. The
/// reload is the `LoadWord { d: 0, a: 1, .. }` directly before the `mtlr`. It may
/// move past instructions that neither read nor write r0, but must stay after the
/// last `bl`. Returns the `old index -> new index` permutation (identity when
/// nothing moves) so the caller can remap relocation indices.
pub fn hoist_link_register_reload(instructions: &mut Vec<Instruction>) -> Vec<usize> {
    let identity: Vec<usize> = (0..instructions.len()).collect();
    let Some(mtlr) = instructions.iter().position(|instruction| matches!(instruction, Instruction::MoveToLinkRegister { .. })) else {
        return identity;
    };
    if mtlr == 0 {
        return identity;
    }
    let reload = mtlr - 1;
    if !matches!(instructions[reload], Instruction::LoadWord { d: 0, a: 1, .. }) {
        return identity;
    }
    let Some(call) = instructions[..reload].iter().rposition(|instruction| matches!(instruction, Instruction::BranchAndLink { .. })) else {
        return identity;
    };
    let target = call + 1;
    if target >= reload {
        return identity; // the reload already sits right after the call
    }
    // The reload writes r0; it can only pass post-call work that leaves r0 alone.
    if instructions[target..reload].iter().any(touches_register_zero) {
        return identity;
    }
    let moved = instructions.remove(reload);
    instructions.insert(target, moved);
    (0..instructions.len())
        .map(|old| {
            if old == reload {
                target
            } else if (target..reload).contains(&old) {
                old + 1
            } else {
                old
            }
        })
        .collect()
}

/// Whether an instruction reads or writes general register r0 (the scratch).
fn touches_register_zero(instruction: &Instruction) -> bool {
    register_operands(instruction).iter().any(|operand| operand.class == Class::General && operand.register == 0)
}

/// Whether the function has a forward branch — v1 leaves such functions untouched
/// because reordering would invalidate the branch's instruction-index target.
fn has_forward_branch(instructions: &[Instruction]) -> bool {
    instructions.iter().any(|instruction| matches!(instruction, Instruction::BranchConditionalForward { .. } | Instruction::Branch { .. }))
}

/// The (class, register) sets an instruction defines and uses.
fn defs_and_uses(instruction: &Instruction) -> (Vec<(Class, u8)>, Vec<(Class, u8)>) {
    let mut defs = Vec::new();
    let mut uses = Vec::new();
    for operand in register_operands(instruction) {
        let key = (operand.class, operand.register);
        match operand.role {
            RegisterRole::Define => defs.push(key),
            RegisterRole::Use => uses.push(key),
        }
    }
    (defs, uses)
}

/// Whether instruction `later` must stay after `earlier` because of a data
/// dependence: read-after-write, write-after-write, or write-after-read on any
/// register (`r0`/`f0` scratch reuse included — those are ordinary registers here).
fn depends_on(earlier: &Instruction, later: &Instruction) -> bool {
    let (earlier_defs, earlier_uses) = defs_and_uses(earlier);
    let (later_defs, later_uses) = defs_and_uses(later);
    let intersects = |a: &[(Class, u8)], b: &[(Class, u8)]| a.iter().any(|key| b.contains(key));
    intersects(&earlier_defs, &later_uses) // RAW
        || intersects(&earlier_defs, &later_defs) // WAW
        || intersects(&earlier_uses, &later_defs) // WAR
}

/// The latency rank of an instruction — higher issues earlier so its result is
/// ready by the time a later instruction needs it. mwcc hoists the long-latency
/// integer multiply and divide ahead of the cheap operations around them; this
/// reproduces that ordering. Everything else is rank 1 (issued in program order).
fn latency_rank(instruction: &Instruction) -> u8 {
    use Instruction::*;
    match instruction {
        DivideWord { .. } | DivideWordUnsigned { .. } | FloatDivideSingle { .. } => 3,
        MultiplyLow { .. } | MultiplyImmediate { .. } | FloatMultiplySingle { .. }
        | FloatMultiplyAddSingle { .. } | FloatMultiplySubtractSingle { .. }
        | FloatNegativeMultiplySubtractSingle { .. } => 2,
        _ => 1,
    }
}

/// Among the ready instructions (original indices into the run), choose the one
/// to issue next: the highest latency rank, breaking ties by the lowest original
/// index (so equal-rank instructions keep program order, and the policy is a
/// no-op for runs with no long-latency op to hoist).
fn pick_ready(ready: &[usize], instructions: &[Instruction]) -> usize {
    ready
        .iter()
        .copied()
        .max_by_key(|&index| (latency_rank(&instructions[index]), std::cmp::Reverse(index)))
        .unwrap()
}

/// List-schedule one run of schedulable instructions, given by their original
/// indices. Returns the original indices in scheduled order.
fn list_schedule(run: &[usize], instructions: &[Instruction]) -> Vec<usize> {
    let count = run.len();
    // predecessors[k] = how many earlier-in-run instructions instruction run[k]
    // still depends on; successors[k] = the run-local indices that depend on it.
    let mut remaining_predecessors = vec![0usize; count];
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); count];
    for later in 0..count {
        for earlier in 0..later {
            if depends_on(&instructions[run[earlier]], &instructions[run[later]]) {
                remaining_predecessors[later] += 1;
                successors[earlier].push(later);
            }
        }
    }

    let mut scheduled = Vec::with_capacity(count);
    let mut placed = vec![false; count];
    while scheduled.len() < count {
        let ready: Vec<usize> = (0..count)
            .filter(|&k| !placed[k] && remaining_predecessors[k] == 0)
            .collect();
        // `pick_ready` works in original indices; map the run-local ready set.
        let ready_original: Vec<usize> = ready.iter().map(|&k| run[k]).collect();
        let chosen_original = pick_ready(&ready_original, instructions);
        let chosen = run.iter().position(|&original| original == chosen_original).unwrap();

        placed[chosen] = true;
        scheduled.push(run[chosen]);
        for &successor in &successors[chosen] {
            remaining_predecessors[successor] -= 1;
        }
    }
    scheduled
}

/// Reorder `instructions` in place to mwcc's schedule, returning the
/// `old index -> new index` permutation (identity if nothing moved). A function
/// with a forward branch is left untouched and the identity permutation returned.
pub fn schedule(instructions: &mut Vec<Instruction>) -> Vec<usize> {
    let count = instructions.len();
    let identity: Vec<usize> = (0..count).collect();
    if has_forward_branch(instructions) {
        return identity;
    }

    // new position -> old index, by walking the stream and scheduling each
    // maximal run of schedulable instructions, leaving barriers fixed.
    let mut order: Vec<usize> = Vec::with_capacity(count);
    let mut index = 0;
    while index < count {
        if is_barrier(&instructions[index]) {
            order.push(index);
            index += 1;
        } else {
            let start = index;
            while index < count && !is_barrier(&instructions[index]) {
                index += 1;
            }
            let run: Vec<usize> = (start..index).collect();
            order.extend(list_schedule(&run, instructions));
        }
    }

    if order == identity {
        return identity;
    }
    let mut old_to_new = vec![0usize; count];
    for (new_position, &old) in order.iter().enumerate() {
        old_to_new[old] = new_position;
    }
    let reordered: Vec<Instruction> = order.iter().map(|&old| instructions[old].clone()).collect();
    *instructions = reordered;
    old_to_new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_independent_multiply_is_hoisted_ahead_of_a_cheap_op() {
        // ((a*b)+1)*(c*d): the second product (index 2) is independent and issues
        // before the dependent addi (index 1), hiding the multiply latency.
        let mut stream = vec![
            Instruction::MultiplyLow { d: 3, a: 3, b: 4 },     // 0: a*b
            Instruction::AddImmediate { d: 3, a: 3, immediate: 1 }, // 1: +1 (needs 0)
            Instruction::MultiplyLow { d: 0, a: 5, b: 6 },     // 2: c*d (independent)
            Instruction::MultiplyLow { d: 3, a: 3, b: 0 },     // 3: needs 1 and 2
            Instruction::BranchToLinkRegister,
        ];
        let permutation = schedule(&mut stream);
        assert_eq!(
            stream,
            vec![
                Instruction::MultiplyLow { d: 3, a: 3, b: 4 },
                Instruction::MultiplyLow { d: 0, a: 5, b: 6 },
                Instruction::AddImmediate { d: 3, a: 3, immediate: 1 },
                Instruction::MultiplyLow { d: 3, a: 3, b: 0 },
                Instruction::BranchToLinkRegister,
            ]
        );
        // old index 1 (addi) moved to new position 2; old 2 (c*d) to position 1.
        assert_eq!(permutation, vec![0, 2, 1, 3, 4]);
    }

    #[test]
    fn dependences_constrain_a_run_to_a_valid_order() {
        // b = a*a (i1) depends on a defined by i0; both must keep order.
        let mut stream = vec![
            Instruction::Add { d: 3, a: 3, b: 4 },       // r3 = r3 + r4
            Instruction::MultiplyLow { d: 3, a: 3, b: 3 }, // r3 = r3 * r3
            Instruction::BranchToLinkRegister,
        ];
        let original = stream.clone();
        schedule(&mut stream);
        assert_eq!(stream, original); // the dependence forbids any reorder anyway
    }

    #[test]
    fn a_function_with_a_forward_branch_is_left_untouched() {
        let mut stream = vec![
            Instruction::Add { d: 3, a: 3, b: 4 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 3 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 1 },
            Instruction::BranchToLinkRegister,
        ];
        let original = stream.clone();
        let permutation = schedule(&mut stream);
        assert_eq!(stream, original);
        assert_eq!(permutation, (0..4).collect::<Vec<_>>());
    }

    #[test]
    fn barriers_stay_fixed_and_bound_the_runs() {
        // Two independent adds separated by a store: the store must not move, and
        // with the identity policy nothing else does either.
        let mut stream = vec![
            Instruction::Add { d: 3, a: 3, b: 4 },
            Instruction::StoreWord { s: 3, a: 1, offset: 8 },
            Instruction::Add { d: 5, a: 5, b: 6 },
            Instruction::BranchToLinkRegister,
        ];
        let original = stream.clone();
        schedule(&mut stream);
        assert_eq!(stream, original);
    }
}
