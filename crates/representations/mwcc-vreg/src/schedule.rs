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
            | FloatCompareOrdered { .. } | CompareWord { .. } | CompareWordImmediate { .. } | CompareWordImmediateField { .. }
            | CompareLogicalWord { .. } | CompareLogicalWordImmediate { .. }
            | BranchConditionalForward { .. } | BranchConditionalToLinkRegister { .. } | Branch { .. }
            | BranchToLinkRegister | BranchToCountRegister | BranchToCountRegisterAndLink | BranchAndLink { .. }
            | MoveFromLinkRegister { .. } | MoveToLinkRegister { .. } | MoveToCountRegister { .. }
    )
}

/// A store to memory. mwcc keeps a post-call store (the one consuming the call's
/// result) ahead of the epilogue's saved-LR reload, so the reload hoist must not pass
/// one — unlike a post-call load or register move, which it does overlap.
fn is_store(instruction: &Instruction) -> bool {
    use Instruction::*;
    matches!(
        instruction,
        StoreWord { .. } | StoreByte { .. } | StoreHalfword { .. } | StoreFloatSingle { .. }
            | StoreFloatDouble { .. } | StoreWordWithUpdate { .. } | StoreWordIndexed { .. }
            | StoreByteIndexed { .. } | StoreHalfwordIndexed { .. } | StoreFloatSingleIndexed { .. }
            | StoreFloatDoubleIndexed { .. }
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
    // Intra-function control flow makes the saved-LR reload a join point — it
    // commonly sits at a SHARED epilogue reached by several edges (early returns),
    // so hoisting it above a branch would skip it on some path. Branch targets are
    // also instruction indices that this reordering would invalidate. mwcc only
    // issues the reload early in straight-line functions, so bail when the function
    // has any forward/unconditional branch.
    if has_forward_branch(instructions) {
        return identity;
    }
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
    let Some(call) = instructions[..reload].iter().rposition(|instruction| {
        matches!(instruction, Instruction::BranchAndLink { .. } | Instruction::BranchToCountRegisterAndLink)
    }) else {
        return identity;
    };
    // mwcc issues a high-latency op that depends on the call result — a `mullw` combining the result
    // with a saved value — right after the call, overlapping its multi-cycle latency with the LR-reload
    // load. So the reload sits AFTER such a multiply, not before it. (Only a multiply whose destination
    // is not r0, so the reload does not clobber its result.)
    let mut target = call + 1;
    while target < reload && matches!(&instructions[target], Instruction::MultiplyLow { d, .. } if *d != 0) {
        target += 1;
    }
    if target >= reload {
        return identity; // the reload already sits right after the call
    }
    // The reload writes r0; it can only pass post-call work that leaves r0 alone.
    if instructions[target..reload].iter().any(touches_register_zero) {
        return identity;
    }
    // It must also stay after a store mwcc keeps ahead of it — the store that consumes
    // the call result (`g = h();` -> `bl; stw r3; lwz r0`). mwcc does overlap the
    // reload with a post-call *load* or register move (the local-reload cases), so only
    // a store blocks the hoist, not every barrier.
    if instructions[target..reload].iter().any(is_store) {
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

/// Drop `mr rX, rX` self-moves (`or rX,rX,rX`) the register allocator produces when it
/// colors a value's virtual home to the register the value already holds (`foo()+1` ->
/// `mr r3,r3; addi r0,r3,1`). mwcc coalesces these away. A self-move is a no-op, so removing
/// it is byte-neutral — it only shortens the function. Returns the old->new index permutation
/// so relocations can be remapped; a removed self-move never carries a relocation, so its
/// own mapping is a don't-care (pointed at the next survivor).
pub fn coalesce_self_moves(instructions: &mut Vec<Instruction>) -> Vec<usize> {
    let original = std::mem::take(instructions);
    let mut permutation = Vec::with_capacity(original.len());
    let mut next = 0;
    for instruction in original {
        permutation.push(next);
        let is_self_move = matches!(&instruction, Instruction::Or { a, s, b } if a == s && s == b);
        if !is_self_move {
            instructions.push(instruction);
            next += 1;
        }
    }
    permutation
}

/// Fill the `lis -> addi` address-formation latency slot with the first later
/// independent load-immediate, as mwcc's scheduler does: a dependent `addi`
/// issued in the very next slot stalls on the `lis` result, so mwcc issues one
/// ready `li` between them instead (measured across the store-fill corpus:
/// `lis r3; li v0; addi base,r3; li v1; ...` from the natural `lis; addi; li...`
/// order). Runs on the VIRTUAL stream, before allocation, so liveness and the
/// allocator see the slot-filled order. Straight-line functions only — a
/// reorder would invalidate branch-target indices (the fire-657 constraint).
/// Returns the old->new index permutation for relocation remap.
pub fn fill_address_latency_slots(instructions: &mut Vec<Instruction>) -> Vec<usize> {
    let mut permutation: Vec<usize> = (0..instructions.len()).collect();
    if has_forward_branch(instructions) {
        return permutation;
    }
    let mut index = 0;
    while index + 1 < instructions.len() {
        // The @ha/@lo pair: a `lis` immediately followed by an `addi` that reads it.
        let stalls = matches!(instructions[index], Instruction::AddImmediateShifted { .. })
            && matches!(instructions[index + 1], Instruction::AddImmediate { .. })
            && depends_on(&instructions[index], &instructions[index + 1]);
        if !stalls {
            index += 1;
            continue;
        }
        // The candidate fill is the FIRST load-immediate (`li`, i.e. `addi rD,0,…`)
        // before the next barrier — mwcc fills the single stall slot with the one
        // ready instruction; anything past the first belongs after the addi.
        let mut fill = None;
        let mut cursor = index + 2;
        while cursor < instructions.len() && !is_barrier(&instructions[cursor]) {
            if matches!(instructions[cursor], Instruction::AddImmediate { a: 0, .. }) {
                let independent = (index + 1..cursor).all(|between| {
                    !depends_on(&instructions[between], &instructions[cursor])
                        && !depends_on(&instructions[cursor], &instructions[between])
                });
                if independent {
                    fill = Some(cursor);
                }
                break;
            }
            cursor += 1;
        }
        let Some(fill) = fill else {
            index += 2;
            continue;
        };
        let moved = instructions.remove(fill);
        instructions.insert(index + 1, moved);
        for slot in &mut permutation {
            if *slot == fill {
                *slot = index + 1;
            } else if (index + 1..fill).contains(slot) {
                *slot += 1;
            }
        }
        index += 3; // past the lis, the fill, and the addi
    }
    permutation
}

/// Schedule a non-leaf function's link-register save (`stw r0,20(r1)`) past the
/// leading argument materializations of its first call. mwcc fills the `mflr`->save
/// latency gap with up to two ready instructions, so `stwu; mflr r0; li r3,…; stw
/// r0,20(r1); bl` rather than saving immediately. Returns the index permutation
/// (old -> new) so relocations can be remapped; identity when nothing moved.
pub fn schedule_link_register_save(instructions: &mut Vec<Instruction>) -> Vec<usize> {
    let identity: Vec<usize> = (0..instructions.len()).collect();
    // Reordering shifts instruction indices, which would invalidate branch targets.
    if has_forward_branch(instructions) {
        return identity;
    }
    // The non-leaf prologue: `mflr r0` immediately followed by `stw r0,20(r1)`. A
    // callee-saved or already-scheduled prologue does not match (the save is not the
    // very next instruction), so it is left untouched.
    let Some(mflr) = instructions.iter().position(|instruction| matches!(instruction, Instruction::MoveFromLinkRegister { d: 0 })) else {
        return identity;
    };
    let save = mflr + 1;
    if save >= instructions.len() || !matches!(instructions[save], Instruction::StoreWord { s: 0, a: 1, offset: 20 }) {
        return identity;
    }
    // An INDIRECT call delays the save past the same gap: `mr r12, fp` then the
    // argument moves, ending in `mtctr r12; bctrl`. The leading run here is the setup
    // moves (`mr`, i.e. `or rD,rS,rS`) and any `li`-form argument.
    let moved_count = if save + 1 < instructions.len() && matches!(instructions[save + 1], Instruction::Or { a: 12, .. }) {
        let mut run = 0;
        while save + 1 + run < instructions.len()
            && matches!(instructions[save + 1 + run], Instruction::Or { .. } | Instruction::AddImmediate { a: 0, .. } | Instruction::AddImmediateShifted { a: 0, .. })
        {
            run += 1;
        }
        let dispatch = save + 1 + run;
        if dispatch + 1 >= instructions.len()
            || !matches!(instructions[dispatch], Instruction::MoveToCountRegister { .. })
            || !matches!(instructions[dispatch + 1], Instruction::BranchToCountRegisterAndLink)
        {
            return identity;
        }
        run.min(2)
    } else {
        // The leading run of argument materializations mwcc hoists into the latency
        // gap: load-immediate forms only (`li`, `lis`, and an SDA21 string/global
        // address, which is `addi rD,0,…` + a relocation — all `a == 0`). A frame- or
        // register-relative `addi rD,r1,…` (e.g. `&local`) is NOT hoisted; it stays
        // after the save, so the run requires `a == 0`.
        let mut run = 0;
        while save + 1 + run < instructions.len()
            && matches!(instructions[save + 1 + run], Instruction::AddImmediate { a: 0, .. } | Instruction::AddImmediateShifted { a: 0, .. })
        {
            run += 1;
        }
        let next = save + 1 + run;
        // The leading `li`-run is hoisted into the gap even when more argument
        // computation follows before the call — a `&global + n` offset add (`addi
        // r3,r3,n`) or a register shuffle. mwcc fills the latency slot with the ready
        // loads regardless, so require only that a call (`bl`, or an indirect `bctrl`
        // through a global function pointer's `lwz r12`) follows the run, not that it is
        // the very next instruction. (Only the run is moved; the trailing work stays.)
        if run == 0 || !instructions[next..].iter().any(|instruction| matches!(instruction, Instruction::BranchAndLink { .. } | Instruction::BranchToCountRegisterAndLink)) {
            return identity;
        }
        run.min(2)
    };
    if moved_count == 0 {
        return identity;
    }
    // The save reads r0 (from `mflr`); it may only pass instructions that leave r0
    // alone (argument materializations write r3.., never the scratch).
    if instructions[save + 1..save + 1 + moved_count].iter().any(touches_register_zero) {
        return identity;
    }
    let moved = instructions.remove(save);
    instructions.insert(save + moved_count, moved);
    (0..instructions.len())
        .map(|old| {
            if old == save {
                save + moved_count
            } else if (save + 1..=save + moved_count).contains(&old) {
                old - 1
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
        DivideWord { .. } | DivideWordUnsigned { .. } | FloatDivideSingle { .. }
        | FloatDivideDouble { .. } => 3,
        // The DOUBLE multiply family ranks with its single cousins: measured in
        // the float-table class (canary 1052), a leading `fmul` keeps the run's
        // head against a latency-chain lis, and a program-order `fmadd` keeps its
        // place against a later `fmul` — same rank, program order breaks ties.
        MultiplyLow { .. } | MultiplyImmediate { .. } | FloatMultiplySingle { .. }
        | FloatMultiplyAddSingle { .. } | FloatMultiplySubtractSingle { .. }
        | FloatNegativeMultiplySubtractSingle { .. } | FloatMultiplyDouble { .. }
        | FloatMultiplyAddDouble { .. } | FloatMultiplySubtractDouble { .. }
        | FloatNegativeMultiplySubtractDouble { .. } => 2,
        _ => 1,
    }
}

/// List-schedule one run of schedulable instructions, given by their original
/// indices. Returns the original indices in scheduled order.
fn list_schedule(run: &[usize], instructions: &[Instruction]) -> Vec<usize> {
    let count = run.len();
    // An address-formation `lis` (`a == 0`) whose dependent `addi` sits in the
    // same run heads a latency chain: mwcc issues it at the run's head, ahead of
    // independent single-cycle materializations (measured: `reg(5, cb)` hoists
    // `lis r4,cb@ha` above `li r3,5`). Rank it with the long-latency ops. A `lis`
    // whose consumer is a barrier (the `stwu @lo` fold) is NOT boosted — mwcc
    // keeps the fold's value `li` ahead of it.
    let heads_latency_chain: Vec<bool> = (0..count)
        .map(|k| {
            matches!(instructions[run[k]], Instruction::AddImmediateShifted { a: 0, .. })
                && (k + 1..count).any(|later| {
                    matches!(instructions[run[later]], Instruction::AddImmediate { .. })
                        && depends_on(&instructions[run[k]], &instructions[run[later]])
                })
        })
        .collect();
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
        // Among the ready instructions: the highest latency rank first (a
        // latency-chain-head `lis` counts as rank 2), ties in program order.
        let chosen = (0..count)
            .filter(|&k| !placed[k] && remaining_predecessors[k] == 0)
            .max_by_key(|&k| {
                let rank = latency_rank(&instructions[run[k]]).max(if heads_latency_chain[k] { 2 } else { 1 });
                (rank, std::cmp::Reverse(run[k]))
            })
            .unwrap();

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
    fn the_lis_addi_latency_slot_takes_the_first_independent_li() {
        // Natural order `lis; addi; li v0; li v1` — the addi stalls on the lis,
        // so the first li moves into the slot: `lis; li v0; addi; li v1`.
        let mut stream = vec![
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },  // 0: lis r3,@ha
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },         // 1: addi r4,r3,@lo (stalls)
            Instruction::AddImmediate { d: 5, a: 0, immediate: 7 },         // 2: li v0 (the fill)
            Instruction::AddImmediate { d: 6, a: 0, immediate: 9 },         // 3: li v1 (stays)
            Instruction::StoreWord { s: 5, a: 4, offset: 0 },
            Instruction::BranchToLinkRegister,
        ];
        let permutation = fill_address_latency_slots(&mut stream);
        assert_eq!(
            stream,
            vec![
                Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
                Instruction::AddImmediate { d: 5, a: 0, immediate: 7 },
                Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
                Instruction::AddImmediate { d: 6, a: 0, immediate: 9 },
                Instruction::StoreWord { s: 5, a: 4, offset: 0 },
                Instruction::BranchToLinkRegister,
            ]
        );
        // The addi (old 1) slides to 2; the fill (old 2) lands at 1.
        assert_eq!(permutation, vec![0, 2, 1, 3, 4, 5]);
    }

    #[test]
    fn the_slot_fill_leaves_dependent_or_absent_candidates_alone() {
        // The only later li DEFINES the register the addi already wrote (WAW with
        // the crossed addi) — moving it would reorder the writes, so no fill.
        let mut stream = vec![
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 0, immediate: 7 },
            Instruction::BranchToLinkRegister,
        ];
        let before = stream.clone();
        assert_eq!(fill_address_latency_slots(&mut stream), vec![0, 1, 2, 3]);
        assert_eq!(stream, before);

        // A barrier (store) before any li: nothing to fill with.
        let mut stream = vec![
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
            Instruction::StoreWord { s: 4, a: 3, offset: 0 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 7 },
            Instruction::BranchToLinkRegister,
        ];
        let before = stream.clone();
        assert_eq!(fill_address_latency_slots(&mut stream), vec![0, 1, 2, 3, 4]);
        assert_eq!(stream, before);

        // A forward branch anywhere: the whole function is left untouched.
        let mut stream = vec![
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 3, immediate: 0 },
            Instruction::AddImmediate { d: 5, a: 0, immediate: 7 },
            Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: 4 },
            Instruction::BranchToLinkRegister,
        ];
        let before = stream.clone();
        assert_eq!(fill_address_latency_slots(&mut stream), vec![0, 1, 2, 3, 4]);
        assert_eq!(stream, before);
    }

    #[test]
    fn a_lis_heading_a_latency_chain_issues_at_the_run_head() {
        // `reg(5, cb)` natural order: `li r3,5; lis r4; addi r4,r4` — the lis has
        // a dependent addi in the run, so it issues first (measured).
        let mut stream = vec![
            Instruction::AddImmediate { d: 3, a: 0, immediate: 5 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::BranchAndLink { target: String::from("reg") },
        ];
        let permutation = schedule(&mut stream);
        assert_eq!(
            stream,
            vec![
                Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
                Instruction::AddImmediate { d: 3, a: 0, immediate: 5 },
                Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
                Instruction::BranchAndLink { target: String::from("reg") },
            ]
        );
        assert_eq!(permutation, vec![1, 0, 2, 3]);

        // A lis whose consumer is a barrier (the `stwu @lo` fold), NOT an in-run
        // addi: no boost — the fold keeps its value `li` ahead of the lis.
        let mut stream = vec![
            Instruction::AddImmediate { d: 4, a: 0, immediate: 7 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::StoreWordWithUpdate { s: 4, a: 3, offset: 0 },
        ];
        let before = stream.clone();
        assert_eq!(schedule(&mut stream), vec![0, 1, 2]);
        assert_eq!(stream, before);
    }

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
