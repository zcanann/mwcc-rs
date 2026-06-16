//! `switch` dispatch lowering.
//!
//! For a small or sparse switch (n <= 6 cases) mwcc emits a balanced
//! binary-search comparison tree; a larger/denser switch uses a jump table
//! (roadmap). The tree is reproduced here exactly by *threading the known range*
//! of the scrutinee through the recursion: every comparison narrows the interval
//! `[klo, khi]` that the scrutinee can still occupy, and each leaf picks its test
//! form from where its value sits in that interval —
//!
//!   * value at the top of the known range (bottom open)  -> `cmpwi v;   bge body; b default`
//!   * value at the bottom of the known range (top open)  -> `cmpwi v+1; bge default; b body`
//!   * value strictly inside the range (or isolated)      -> `cmpwi v;   beq body; b default`
//!   * value pinned on both sides                         -> no test; branch straight to the body
//!
//! The pivot of an interior node is `lo + n/2` when the upper bound is still open
//! and `lo + (n-1)/2` once it is closed. Comparison nodes are laid out in
//! pre-order (node, left subtree, right subtree); the case bodies follow in sorted
//! value order, with the default last.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, JumpTable, RelocationKind, RelocationTarget};
use mwcc_syntax_trees::{Expression, SwitchArm, Type};
use crate::generator::*;

/// A pending dispatch-branch destination, resolved to an instruction index once
/// the case bodies have been laid out.
#[derive(Clone, Copy)]
enum Target {
    /// The body of the case at this index in the sorted-by-value arm list.
    Body(usize),
    /// The default / fall-through return.
    Default,
}

impl Generator {
    /// Emit the whole body of a function whose body is a single `switch`, as the
    /// comparison tree followed by the case bodies and the default. Defers (never
    /// miscompiles) any shape outside the supported subset.
    pub(crate) fn emit_switch(
        &mut self,
        scrutinee: &Expression,
        arms: &[SwitchArm],
        default: &Expression,
        default_is_labeled: bool,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        // The scrutinee must already live in a general register (a bare integer
        // parameter or local). The comparisons read it directly.
        let register = match scrutinee {
            Expression::Variable(name) => {
                let location = self
                    .locations
                    .get(name)
                    .ok_or_else(|| Diagnostic::error("switch scrutinee is not a known variable (roadmap)"))?;
                if !matches!(location.class, ValueClass::General) {
                    return Err(Diagnostic::error("only an integer switch scrutinee is supported yet (roadmap)"));
                }
                location.register
            }
            _ => return Err(Diagnostic::error("switch scrutinee must be a simple variable (roadmap)")),
        };

        // Sort the arms by value; the dispatch assumes ascending, distinct values.
        let mut sorted: Vec<&SwitchArm> = arms.iter().collect();
        sorted.sort_by_key(|arm| arm.value);
        if sorted.is_empty() {
            return Err(Diagnostic::error("an empty switch is not supported (roadmap)"));
        }
        for pair in sorted.windows(2) {
            if pair[0].value == pair[1].value {
                return Err(Diagnostic::error("duplicate switch case values"));
            }
        }
        // A switch whose case values span at most 6 (so a jump table would hold at
        // most 6 entries) is *always* the comparison tree; mwcc never tables a span
        // that small. A wider span is sometimes a jump table — a
        // distribution-dependent decision (`{0,2,4,6}` tables but `{0,1,2,6}` does
        // not). The one wide-span shape that is *always* a table is a CONTIGUOUS run
        // of >= 7 cases; handle that (zero-based, scrutinee in r3) and defer the rest
        // (never a non-matching tree). `sorted` is ascending, so span = last - first + 1.
        let span = sorted[sorted.len() - 1].value - sorted[0].value + 1;
        if span > 6 {
            let contiguous = span == sorted.len() as i64;
            if contiguous && sorted.len() >= 7 && sorted[0].value == 0 && register == result {
                return self.emit_jump_table(register, &sorted, default, default_is_labeled, return_type, result);
            }
            return Err(Diagnostic::error("wide-span switch (jump table) not implemented for this shape yet (roadmap)"));
        }
        // The tests are `cmpwi v` and `cmpwi v+1`, so both must fit the signed
        // 16-bit immediate.
        for arm in &sorted {
            if arm.value < i16::MIN as i64 || arm.value >= i16::MAX as i64 {
                return Err(Diagnostic::error("switch case value out of cmpwi immediate range (roadmap)"));
            }
        }

        // Emit the comparison tree (pre-order), collecting the branches to patch.
        let values: Vec<i64> = sorted.iter().map(|arm| arm.value).collect();
        let mut patches: Vec<(usize, Target)> = Vec::new();
        self.lower_switch_range(register, &values, 0, values.len() - 1, None, None, &mut patches);

        // Case bodies in sorted value order, then the default — each ends in `blr`.
        let mut body_start = vec![0usize; sorted.len()];
        for (index, arm) in sorted.iter().enumerate() {
            body_start[index] = self.output.instructions.len();
            self.evaluate_tail(&arm.result, return_type, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        }
        let default_start = self.output.instructions.len();
        self.evaluate_tail(default, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);

        // Resolve the dispatch branches now that the bodies have addresses.
        for (index, target) in patches {
            let destination = match target {
                Target::Body(body) => body_start[body],
                Target::Default => default_start,
            };
            match &mut self.output.instructions[index] {
                Instruction::BranchConditionalForward { target, .. } => *target = destination,
                Instruction::Branch { target } => *target = destination,
                _ => unreachable!("switch patch points at a non-branch instruction"),
            }
        }
        Ok(())
    }

    /// Emit the jump-table dispatch for a contiguous, zero-based, >= 7-case switch
    /// (the wide-span shape mwcc always tables). The scrutinee is the table index:
    ///
    ///   cmplwi r3, max ; bgt default ; lis r4, table@ha ; slwi r0, r3, 2
    ///   addi r3, r4, table@lo ; lwzx r0, r3, r0 ; mtctr r0 ; bctr
    ///
    /// followed by the case bodies in value order and the default. The table itself
    /// (one `.text` body offset per index) is recorded on the function; the writer
    /// materializes it as an anonymous `@N` object in `.data` and fills in the two
    /// `@N` address relocations (`lis`/`addi`) and the per-entry `ADDR32` relocations.
    fn emit_jump_table(
        &mut self,
        register: u8,
        sorted: &[&SwitchArm],
        default: &Expression,
        default_is_labeled: bool,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let max = sorted[sorted.len() - 1].value;
        // Bounds check: an unsigned compare catches both `x > max` and `x < 0`.
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: register, immediate: max as u16 });
        let bgt_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options: BGT.0, condition_bit: BGT.1, target: 0 });
        // Table base address (`lis r4, @ha` / `addi r3, r4, @lo`) and the index scale.
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: register, shift: 2 });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::LoadWordIndexed { d: 0, a: 3, b: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 0 });
        self.output.instructions.push(Instruction::BranchToCountRegister);

        // Case bodies in value order (== index order, since contiguous zero-based),
        // then the default. The table entry for index i is its body's byte offset.
        let mut entries = vec![0u32; sorted.len()];
        for (index, arm) in sorted.iter().enumerate() {
            entries[index] = self.output.instructions.len() as u32 * 4;
            self.evaluate_tail(&arm.result, return_type, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        }
        let default_index = self.output.instructions.len();
        self.evaluate_tail(default, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);

        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[bgt_index] {
            *target = default_index;
        }
        // mwcc numbers the table's `@N` past one label per case plus the dispatch,
        // and one more when the default is an explicit `default:` label.
        let anonymous_offset = entries.len() as u32 + 1 + if default_is_labeled { 1 } else { 0 };
        self.output.jump_table = Some(JumpTable { entries, anonymous_offset });
        Ok(())
    }

    /// Emit the comparison code for the sorted-value sub-range `[lo, hi]`, given the
    /// known bounds `klo`/`khi` the scrutinee already satisfies (`None` = open on
    /// that side). Returns the index of the first instruction emitted (the range's
    /// entry label). Only called for ranges that emit a test — a single value
    /// pinned on both sides has no code and is branched to directly by its parent.
    fn lower_switch_range(
        &mut self,
        register: u8,
        values: &[i64],
        lo: usize,
        hi: usize,
        klo: Option<i64>,
        khi: Option<i64>,
        patches: &mut Vec<(usize, Target)>,
    ) -> usize {
        let entry = self.output.instructions.len();
        let count = hi - lo + 1;

        if count == 1 {
            let value = values[lo];
            if khi == Some(value) {
                // Top of the known range, bottom open: `>= v` is exactly `v`.
                self.emit_switch_compare(register, value);
                self.emit_switch_conditional(patches, BGE, Target::Body(lo));
                self.emit_switch_branch(patches, Target::Default);
            } else if klo == Some(value) {
                // Bottom of the known range, top open: `>= v+1` is the default.
                self.emit_switch_compare(register, value + 1);
                self.emit_switch_conditional(patches, BGE, Target::Default);
                self.emit_switch_branch(patches, Target::Body(lo));
            } else {
                // Strictly inside the range (or isolated): an equality test.
                self.emit_switch_compare(register, value);
                self.emit_switch_conditional(patches, BEQ, Target::Body(lo));
                self.emit_switch_branch(patches, Target::Default);
            }
            return entry;
        }

        // Interior node: pivot lower-middle once the upper bound is closed, else
        // upper-middle.
        let mid = if khi.is_none() { lo + count / 2 } else { lo + (count - 1) / 2 };
        let pivot = values[mid];
        self.emit_switch_compare(register, pivot);
        self.emit_switch_conditional(patches, BEQ, Target::Body(mid));

        // The single `bge` selects the right range; the fall-through is the left
        // range, emitted inline next (pre-order).
        let bge_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options: BGE.0, condition_bit: BGE.1, target: 0 });

        // Left range [lo, mid-1], now bounded above by pivot-1.
        if mid == lo {
            // Empty: the `< pivot` region is the (bottom-open) default.
            self.emit_switch_branch(patches, Target::Default);
        } else if mid - 1 == lo && klo == Some(values[lo]) && pivot - 1 == values[lo] {
            // A single value pinned on both sides: branch straight to its body.
            self.emit_switch_branch(patches, Target::Body(lo));
        } else {
            self.lower_switch_range(register, values, lo, mid - 1, klo, Some(pivot - 1), patches);
        }

        // Right range [mid+1, hi], now bounded below by pivot+1: the `bge` target.
        if mid == hi {
            // Empty: the `> pivot` region is the (top-open) default.
            patches.push((bge_index, Target::Default));
        } else if mid + 1 == hi && khi == Some(values[hi]) && pivot + 1 == values[hi] {
            // A single value pinned on both sides: the `bge` jumps straight to it.
            patches.push((bge_index, Target::Body(hi)));
        } else {
            let right_entry = self.lower_switch_range(register, values, mid + 1, hi, Some(pivot + 1), khi, patches);
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[bge_index] {
                *target = right_entry;
            }
        }

        entry
    }

    fn emit_switch_compare(&mut self, register: u8, immediate: i64) {
        self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: immediate as i16 });
    }

    /// Push a forward conditional branch (`(BO, BI)`) bound to `target`.
    fn emit_switch_conditional(&mut self, patches: &mut Vec<(usize, Target)>, options: (u8, u8), target: Target) {
        let index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options: options.0, condition_bit: options.1, target: 0 });
        patches.push((index, target));
    }

    /// Push an unconditional branch bound to `target`.
    fn emit_switch_branch(&mut self, patches: &mut Vec<(usize, Target)>, target: Target) {
        let index = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        patches.push((index, target));
    }
}

/// `beq` — branch if cr0[EQ] (BO=12 branch-if-true, BI=2 the EQ bit).
const BEQ: (u8, u8) = (12, 2);
/// `bge` — branch if not cr0[LT] (BO=4 branch-if-false, BI=0 the LT bit).
const BGE: (u8, u8) = (4, 0);
/// `bgt` — branch if cr0[GT] (BO=12 branch-if-true, BI=1 the GT bit).
const BGT: (u8, u8) = (12, 1);
