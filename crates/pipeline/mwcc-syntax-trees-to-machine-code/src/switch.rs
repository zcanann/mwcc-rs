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

use crate::generator::*;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, JumpTable, RelocationKind, RelocationTarget};
use mwcc_syntax_trees::{ArmBody, Expression, Statement, SwitchArm, Type};
use mwcc_versions::{CallDispatcherStyle, JumpTableBaseStyle};

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
    /// Emit a dense dispatcher whose arms all have the semantic form
    /// `result = callee(forwarded)`, then join at the statement immediately
    /// following the switch. Case bodies retain source order; table entries map
    /// values to those bodies and gaps to the join.
    pub(crate) fn emit_assignment_call_jump_table(
        &mut self,
        scrutinee_register: u8,
        arms: &[(i64, String)],
        forwarded_register: u8,
        result_register: u8,
    ) -> Compilation<()> {
        if arms.is_empty() {
            return Err(Diagnostic::error("an empty call dispatcher is not supported"));
        }
        let mut by_value = std::collections::HashMap::new();
        let mut min = i64::MAX;
        let mut max = i64::MIN;
        for (source_index, (value, _)) in arms.iter().enumerate() {
            if by_value.insert(*value, source_index).is_some() {
                return Err(Diagnostic::error("duplicate switch case values"));
            }
            min = min.min(*value);
            max = max.max(*value);
        }
        let subtract = min < 0 || min >= 3;
        let bound = if subtract { max - min } else { max };
        let negated_base = -min;
        if bound < 0
            || bound > u16::MAX as i64
            || (subtract && !(i16::MIN as i64..=i16::MAX as i64).contains(&negated_base))
        {
            return Err(Diagnostic::error(
                "switch jump-table index/base out of immediate range (roadmap)",
            ));
        }

        let (index_register, table_register) = if subtract {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: scrutinee_register,
                immediate: negated_base as i16,
            });
            (0, 3)
        } else {
            (scrutinee_register, 3)
        };
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: index_register,
                immediate: bound as u16,
            });
        let out_of_range = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: BGT.0,
                condition_bit: BGT.1,
                target: 0,
            });
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::JumpTable);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: table_register,
                a: 0,
                immediate: 0,
            });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: index_register,
                shift: 2,
            });
        self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: table_register,
            immediate: 0,
        });
        let entry_register = match self.behavior.call_dispatcher_style {
            CallDispatcherStyle::Legacy24x => 0,
            CallDispatcherStyle::Packed41 => 3,
        };
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: entry_register,
            a: 3,
            b: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: entry_register });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);

        let mut body_offsets = vec![0u32; arms.len()];
        let mut joins = Vec::with_capacity(arms.len().saturating_sub(1));
        for (source_index, (_, callee)) in arms.iter().enumerate() {
            body_offsets[source_index] = self.output.instructions.len() as u32 * 4;
            self.output
                .instructions
                .push(Instruction::move_register(3, forwarded_register));
            self.record_relocation(RelocationKind::Rel24, callee);
            self.output.instructions.push(Instruction::BranchAndLink {
                target: callee.clone(),
            });
            self.output
                .instructions
                .push(Instruction::move_register(result_register, 3));
            if source_index + 1 != arms.len() {
                let branch = self.output.instructions.len();
                self.output.instructions.push(Instruction::Branch { target: 0 });
                joins.push(branch);
            }
        }
        let join = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[out_of_range]
        {
            *target = join;
        }
        for branch in joins {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = join;
            }
        }
        let default_offset = join as u32 * 4;
        let entries = (0..=bound)
            .map(|index| {
                let value = if subtract { min + index } else { index };
                by_value
                    .get(&value)
                    .map_or(default_offset, |&source_index| body_offsets[source_index])
            })
            .collect();
        // Modern dispatchers retain a label per arm plus a build-specific
        // fixed dispatch block; deferred inlining retains one more hidden label
        // per arm. The fixed residue is independent of the register/string
        // family, so it is resolved explicitly by the compiler profile.
        // The legacy specialized owner accounts its labels before its strings.
        let anonymous_offset = match self.behavior.call_dispatcher_style {
            // The specialized legacy dispatcher accounts its internal labels
            // before its strings; the table follows those string slots directly.
            CallDispatcherStyle::Legacy24x => 0,
            CallDispatcherStyle::Packed41 => {
                arms.len() as u32
                    + u32::from(self.behavior.call_dispatcher_table_base_labels)
                    + arms.len() as u32
                        * u32::from(self.behavior.deferred_call_dispatcher_labels_per_case)
            }
        };
        self.output.jump_tables.push(JumpTable {
            entries,
            anonymous_offset,
        });
        Ok(())
    }

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
                let location = self.locations.get(name).ok_or_else(|| {
                    Diagnostic::error("switch scrutinee is not a known variable (roadmap)")
                })?;
                if !matches!(location.class, ValueClass::General) {
                    return Err(Diagnostic::error(
                        "only an integer switch scrutinee is supported yet (roadmap)",
                    ));
                }
                location.register
            }
            // A non-variable scrutinee (`switch(n & 3)`) evaluates into the general scratch
            // register first, exactly as mwcc does (`clrlwi r0,r3,30`); the comparison tree
            // then reads that register. evaluate_general defers any scrutinee it cannot lower.
            _ => {
                self.evaluate_general(scrutinee, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            }
        };

        // Sort the arms by value; the dispatch assumes ascending, distinct values.
        let mut sorted: Vec<&SwitchArm> = arms.iter().collect();
        sorted.sort_by_key(|arm| arm.value);
        if sorted.is_empty() {
            return Err(Diagnostic::error(
                "an empty switch is not supported (roadmap)",
            ));
        }
        for pair in sorted.windows(2) {
            if pair[0].value == pair[1].value {
                return Err(Diagnostic::error("duplicate switch case values"));
            }
        }
        // Dispatch decisions use value order, while mwcc emits case bodies in
        // source order. Keep those two orders independent and join them through
        // the sorted body index when patching branches.
        // A switch whose case values span at most 6 (so a jump table would hold at
        // most 6 entries) is *always* the comparison tree; mwcc never tables a span
        // that small. A wider span is sometimes a jump table — a
        // distribution-dependent decision (`{0,2,4,6}` tables but `{0,1,2,6}` does
        // not). The one wide-span shape that is *always* a table is a CONTIGUOUS run
        // of >= 7 cases (any base); handle that and defer the rest (never a
        // non-matching tree). `sorted` is ascending, so span = last - first + 1.
        let span = sorted[sorted.len() - 1].value - sorted[0].value + 1;
        if span > 6 {
            let contiguous = span == sorted.len() as i64;
            if contiguous && sorted.len() >= 7 && register == result {
                return self.emit_jump_table(
                    register,
                    arms,
                    &sorted,
                    default,
                    default_is_labeled,
                    return_type,
                    result,
                );
            }
            return Err(Diagnostic::error(
                "wide-span switch (jump table) not implemented for this shape yet (roadmap)",
            ));
        }
        // The tests are `cmpwi v` and `cmpwi v+1`, so both must fit the signed
        // 16-bit immediate.
        for arm in &sorted {
            if arm.value < i16::MIN as i64 || arm.value >= i16::MAX as i64 {
                return Err(Diagnostic::error(
                    "switch case value out of cmpwi immediate range (roadmap)",
                ));
            }
        }

        // Emit the comparison tree (pre-order), collecting the branches to patch.
        let values: Vec<i64> = sorted.iter().map(|arm| arm.value).collect();
        let mut patches: Vec<(usize, Target)> = Vec::new();
        self.lower_switch_range(
            register,
            &values,
            0,
            values.len() - 1,
            None,
            None,
            &mut patches,
        );

        // Case bodies in source order, then the default — each ends in `blr`.
        let mut body_start = vec![0usize; sorted.len()];
        let sorted_index_by_value: std::collections::HashMap<i64, usize> = sorted
            .iter()
            .enumerate()
            .map(|(index, arm)| (arm.value, index))
            .collect();
        let first_source_body = sorted_index_by_value[&arms[0].value];
        // When the final dispatch operation is an unconditional branch to the
        // first source body, that body is the natural fall-through. mwcc drops
        // the otherwise redundant `b +4`.
        let dispatch_end = self.output.instructions.len();
        if dispatch_end != 0
            && patches.iter().any(|&(index, target)| {
                index == dispatch_end - 1
                    && matches!(target, Target::Body(body) if body == first_source_body)
            })
            && matches!(
                self.output.instructions[dispatch_end - 1],
                Instruction::Branch { .. }
            )
        {
            self.output.instructions.pop();
            patches.retain(|&(index, _)| index != dispatch_end - 1);
        }
        for arm in arms {
            let index = sorted_index_by_value[&arm.value];
            body_start[index] = self.output.instructions.len();
            let Some(arm_result) = arm.result() else {
                return Err(Diagnostic::error(
                    "a statement-bodied switch arm is not supported yet (roadmap)",
                ));
            };
            self.evaluate_tail(arm_result, return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
        }
        let default_start = self.output.instructions.len();
        self.evaluate_tail(default, return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

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

    /// A whole-body `void` function that is a single `switch` with STATEMENT arms
    /// (`case V: <stores/...> break;`). Reuses the comparison-tree dispatch, then emits
    /// each arm's statements followed by `blr` (the arm's `break` is the void function's
    /// return). With a `default:` statement arm (`Some`) that arm's statements form a
    /// trailing default block; with NO default (`None`) the dispatch's out-of-range
    /// branches are rewritten from branch-to-default into conditional/unconditional
    /// returns (`b default`->`blr`, `bge default`->`bgelr`), matching mwcc. Deferred
    /// (never mis-compiled): a jump-table span (> 6), a fall-through arm, or an
    /// out-of-range case value — those keep the existing defer.
    pub(crate) fn emit_statement_switch(
        &mut self,
        scrutinee: &Expression,
        arms: &[SwitchArm],
        default_statements: Option<&[Statement]>,
    ) -> Compilation<()> {
        let register = match scrutinee {
            Expression::Variable(name) => {
                let location = self.locations.get(name).ok_or_else(|| {
                    Diagnostic::error("switch scrutinee is not a known variable (roadmap)")
                })?;
                if !matches!(location.class, ValueClass::General) {
                    return Err(Diagnostic::error(
                        "only an integer switch scrutinee is supported yet (roadmap)",
                    ));
                }
                location.register
            }
            _ => {
                self.evaluate_general(scrutinee, GENERAL_SCRATCH)?;
                GENERAL_SCRATCH
            }
        };
        let mut sorted: Vec<&SwitchArm> = arms.iter().collect();
        sorted.sort_by_key(|arm| arm.value);
        if sorted.is_empty() {
            return Err(Diagnostic::error(
                "an empty switch is not supported (roadmap)",
            ));
        }
        for pair in sorted.windows(2) {
            if pair[0].value == pair[1].value {
                return Err(Diagnostic::error("duplicate switch case values"));
            }
        }
        // Comparison tree only (defer a jump-table span); each `cmpwi v`/`cmpwi v+1` must fit i16.
        if sorted[sorted.len() - 1].value - sorted[0].value + 1 > 6 {
            return Err(Diagnostic::error(
                "a wide-span statement switch is not supported yet (roadmap)",
            ));
        }
        for arm in &sorted {
            if arm.value < i16::MIN as i64 || arm.value >= i16::MAX as i64 {
                return Err(Diagnostic::error(
                    "switch case value out of cmpwi immediate range (roadmap)",
                ));
            }
            let ArmBody::Statements(statements) = &arm.body else {
                return Err(Diagnostic::error(
                    "a value-returning statement-switch arm is not supported yet (roadmap)",
                ));
            };
            if arm.falls_through {
                return Err(Diagnostic::error(
                    "a fall-through statement-switch arm is not supported yet (roadmap)",
                ));
            }
            // A single statement per arm has no cross-statement scheduling, so `emit_statement`
            // reproduces mwcc exactly. A run of 2+ stores mwcc latency-schedules (batches the
            // constant loads, then the stores) — the sequential per-statement emission below would
            // diverge, so DEFER a multi-statement arm rather than ship wrong bytes (the store
            // scheduler is the roadmap fix). An empty arm (`case V: break;`) also defers.
            if statements.len() != 1 {
                return Err(Diagnostic::error(
                    "a multi-statement switch arm needs the store scheduler (roadmap)",
                ));
            }
        }
        // The default block is emitted as a straight statement run too, so it is subject to the
        // same single-statement constraint.
        if matches!(default_statements, Some(statements) if statements.len() != 1) {
            return Err(Diagnostic::error(
                "a multi-statement switch default needs the store scheduler (roadmap)",
            ));
        }

        let values: Vec<i64> = sorted.iter().map(|arm| arm.value).collect();
        let mut patches: Vec<(usize, Target)> = Vec::new();
        self.lower_switch_range(
            register,
            &values,
            0,
            values.len() - 1,
            None,
            None,
            &mut patches,
        );

        let sorted_index_by_value: std::collections::HashMap<i64, usize> = sorted
            .iter()
            .enumerate()
            .map(|(index, arm)| (arm.value, index))
            .collect();
        let first_source_body = sorted_index_by_value[&arms[0].value];

        let dispatch_end = self.output.instructions.len();
        if dispatch_end != 0
            && patches.iter().any(|&(index, target)| {
                index == dispatch_end - 1
                    && matches!(target, Target::Body(body) if body == first_source_body)
            })
            && matches!(
                self.output.instructions[dispatch_end - 1],
                Instruction::Branch { .. }
            )
        {
            self.output.instructions.pop();
            patches.retain(|&(index, _)| index != dispatch_end - 1);
        }

        // No-default terminal collapse. The first source case's body is laid out
        // immediately after the dispatch. When that case's leaf is the LAST dispatch
        // instruction pair — `<cond> Body(0); b Default` — and there is no default, mwcc does not
        // branch to the body and then return; it inverts the test into a conditional RETURN and
        // lets the body fall through: `cmpwi v; beq Body(0); b default` -> `cmpwi v; bnelr; <body>`
        // and `cmpwi v; bge Body(0); b default` -> `cmpwi v; bltlr; <body>`. This drops the
        // `b default` and the branch-to-body, saving one instruction. (The with-default and
        // value-returning forms keep the explicit `bge; b` — the collapse is unique to a default
        // whose action is a bare return.) Detected structurally: the final two dispatch
        // instructions are a forward conditional to that body followed by an unconditional to
        // `Default`. Every other no-default shape is handled by the branch-to-return rewrite below.
        if default_statements.is_none() {
            let n = self.output.instructions.len();
            let last_default = n >= 1
                && patches
                    .iter()
                    .any(|&(i, t)| i == n - 1 && matches!(t, Target::Default))
                && matches!(self.output.instructions[n - 1], Instruction::Branch { .. });
            let prev_first_body = n >= 2
                && patches
                    .iter()
                    .any(|&(i, t)| i == n - 2 && matches!(t, Target::Body(body) if body == first_source_body))
                && matches!(
                    self.output.instructions[n - 2],
                    Instruction::BranchConditionalForward { .. }
                );
            if last_default && prev_first_body {
                if let Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    ..
                } = self.output.instructions[n - 2]
                {
                    // Invert the branch sense (BO 12 branch-if-true <-> 4 branch-if-false), keeping
                    // the condition bit, so the test returns on the complement and falls through.
                    let inverted = if options == 12 { 4 } else { 12 };
                    self.output.instructions[n - 2] =
                        Instruction::BranchConditionalToLinkRegister {
                            options: inverted,
                            condition_bit,
                        };
                }
                self.output.instructions.pop(); // drop the unconditional `b default`
                patches.retain(|&(i, _)| i != n - 1 && i != n - 2);
            }
        }

        // Each arm's statements, then `blr` (the `break` returns from the void function).
        let mut body_start = vec![0usize; sorted.len()];
        for arm in arms {
            let index = sorted_index_by_value[&arm.value];
            body_start[index] = self.output.instructions.len();
            let ArmBody::Statements(statements) = &arm.body else {
                unreachable!()
            };
            for statement in statements {
                self.emit_statement(statement)?;
            }
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
        }
        // A `default:` arm becomes a trailing default block; without one, the out-of-range
        // branches return directly (rewritten below), so there is nothing to emit here.
        let default_start = match default_statements {
            Some(statements) => {
                let start = self.output.instructions.len();
                for statement in statements {
                    self.emit_statement(statement)?;
                }
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegister);
                Some(start)
            }
            None => None,
        };

        for (index, target) in patches {
            match target {
                Target::Body(body) => {
                    let destination = body_start[body];
                    match &mut self.output.instructions[index] {
                        Instruction::BranchConditionalForward { target, .. } => {
                            *target = destination
                        }
                        Instruction::Branch { target } => *target = destination,
                        _ => unreachable!("switch patch points at a non-branch instruction"),
                    }
                }
                Target::Default => match default_start {
                    // A trailing default block: resolve the branch to its address.
                    Some(destination) => match &mut self.output.instructions[index] {
                        Instruction::BranchConditionalForward { target, .. } => {
                            *target = destination
                        }
                        Instruction::Branch { target } => *target = destination,
                        _ => unreachable!("switch patch points at a non-branch instruction"),
                    },
                    // No default: the out-of-range branch returns in place — a conditional
                    // branch becomes the matching conditional return (`bge`->`bgelr`), an
                    // unconditional branch becomes `blr`.
                    None => {
                        self.output.instructions[index] = match self.output.instructions[index] {
                            Instruction::BranchConditionalForward {
                                options,
                                condition_bit,
                                ..
                            } => Instruction::BranchConditionalToLinkRegister {
                                options,
                                condition_bit,
                            },
                            Instruction::Branch { .. } => Instruction::BranchToLinkRegister,
                            _ => unreachable!("switch patch points at a non-branch instruction"),
                        };
                    }
                },
            }
        }
        Ok(())
    }

    /// Emit the jump-table dispatch for a contiguous, >= 7-case switch (the
    /// wide-span shape mwcc always tables). The scrutinee indexes the table:
    ///
    ///   cmplwi r3, max ; bgt default ; lis r4, table@ha ; slwi r0, r3, 2
    ///   addi r3, r4, table@lo ; lwzx r0, r3, r0 ; mtctr r0 ; bctr
    ///
    /// followed by the case bodies in value order and the default. A base of 0..2
    /// indexes by the scrutinee directly (the table spans 0..max, its low entries
    /// pointing at the default); a negative or >= 3 base is first rebased to zero
    /// (`addi r0, r3, -base`, the table holding exactly the cases). The table (one
    /// `.text` body offset per index) is recorded on the function; the writer
    /// materializes it as an anonymous `@N` object in `.data` and fills in the two
    /// `@N` address relocations (`lis`/`addi`) and the per-entry `ADDR32` relocations.
    fn emit_jump_table(
        &mut self,
        register: u8,
        arms: &[SwitchArm],
        sorted: &[&SwitchArm],
        default: &Expression,
        default_is_labeled: bool,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let min = sorted[0].value;
        let max = sorted[sorted.len() - 1].value;
        // mwcc rebases the index only when it must (a negative index) or when it
        // saves enough table entries (a base of 3+); a base of 0..2 is padded.
        let subtract = min < 0 || min >= 3;
        let bound = if subtract { max - min } else { max };
        let negated_base = -min;
        if bound > u16::MAX as i64
            || (subtract && (negated_base < i16::MIN as i64 || negated_base > i16::MAX as i64))
        {
            return Err(Diagnostic::error(
                "switch jump-table index/base out of immediate range (roadmap)",
            ));
        }

        // `index_register` holds the 0-based index; `table_register` builds the
        // table address. Rebasing frees the scrutinee register (so `lis` reuses it);
        // otherwise the scrutinee stays live for the `slwi`, so `lis` uses r4.
        let (index_register, table_register) = if subtract {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: register,
                immediate: negated_base as i16,
            });
            (0, 3)
        } else {
            (register, 4)
        };
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: index_register,
                immediate: bound as u16,
            });
        let bgt_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: BGT.0,
                condition_bit: BGT.1,
                target: 0,
            });
        self.record_target(RelocationKind::Addr16Ha, RelocationTarget::JumpTable);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: table_register,
                a: 0,
                immediate: 0,
            });
        let load_base = if self.behavior.jump_table_base_style == JumpTableBaseStyle::EarlyInPlace {
            self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
            self.output.instructions.push(Instruction::AddImmediate {
                d: table_register,
                a: table_register,
                immediate: 0,
            });
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: index_register,
                    shift: 2,
                });
            table_register
        } else {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: 0,
                    s: index_register,
                    shift: 2,
                });
            self.record_target(RelocationKind::Addr16Lo, RelocationTarget::JumpTable);
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: table_register,
                immediate: 0,
            });
            3
        };
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: 0,
            a: load_base,
            b: 0,
        });
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegister);

        // Case bodies in source order, then the default; record each value's offset.
        let mut body_offset = std::collections::HashMap::new();
        for arm in arms {
            body_offset.insert(arm.value, self.output.instructions.len() as u32 * 4);
            let Some(arm_result) = arm.result() else {
                return Err(Diagnostic::error(
                    "a statement-bodied switch arm is not supported yet (roadmap)",
                ));
            };
            self.evaluate_tail(arm_result, return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
        }
        let default_offset = self.output.instructions.len() as u32 * 4;
        let default_index = self.output.instructions.len();
        self.evaluate_tail(default, return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[bgt_index]
        {
            *target = default_index;
        }
        // The table runs over indices 0..=bound: a rebased index `i` is value
        // `min + i`; an un-rebased index is value `i`. An absent value (the padded
        // low entries) points at the default.
        let entries: Vec<u32> = (0..=bound)
            .map(|i| {
                let value = if subtract { min + i } else { i };
                *body_offset.get(&value).unwrap_or(&default_offset)
            })
            .collect();
        // mwcc numbers the table's `@N` past one label per case (not per table
        // entry — a padded table has more entries than cases) plus the dispatch,
        // and one more when the default is an explicit `default:` label.
        let anonymous_offset = sorted.len() as u32 + 1 + if default_is_labeled { 1 } else { 0 };
        self.output.jump_tables.push(JumpTable {
            entries,
            anonymous_offset,
        });
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

        // mwcc pivots on the value at the range CENTRE (`ceil((min+max)/2)`). When that centre is
        // not a case but centre-1 and centre+1 are (a "tight hole": two cases exactly 2 apart
        // straddling it), mwcc tests the hole and sends it to the default instead of pivoting on a
        // case: `cmpwi centre; beq default; bge right`, splitting left=[lo..centre-1] /
        // right=[centre+1..hi]. Every other centre is a case (or a wider hole), for which the
        // median-case pivot below already matches mwcc.
        let sum = values[lo] + values[hi];
        let centre = if sum >= 0 { (sum + 1) / 2 } else { sum / 2 };
        if values[lo..=hi].binary_search(&centre).is_err() {
            if let (Ok(left_rel), Ok(right_rel)) = (
                values[lo..=hi].binary_search(&(centre - 1)),
                values[lo..=hi].binary_search(&(centre + 1)),
            ) {
                let left_hi = lo + left_rel; // index of centre-1
                let right_lo = lo + right_rel; // index of centre+1 (== left_hi + 1)
                self.emit_switch_compare(register, centre);
                self.emit_switch_conditional(patches, BEQ, Target::Default);
                let bge_index = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options: BGE.0,
                        condition_bit: BGE.1,
                        target: 0,
                    });
                // Left [lo, centre-1] (never empty), bounded above by centre-1.
                if left_hi == lo && klo == Some(values[lo]) {
                    self.emit_switch_branch(patches, Target::Body(lo));
                } else {
                    self.lower_switch_range(
                        register,
                        values,
                        lo,
                        left_hi,
                        klo,
                        Some(centre - 1),
                        patches,
                    );
                }
                // Right [centre+1, hi] (never empty), bounded below by centre+1: the `bge` target.
                if right_lo == hi && khi == Some(values[hi]) {
                    patches.push((bge_index, Target::Body(hi)));
                } else {
                    let right_entry = self.lower_switch_range(
                        register,
                        values,
                        right_lo,
                        hi,
                        Some(centre + 1),
                        khi,
                        patches,
                    );
                    if let Instruction::BranchConditionalForward { target, .. } =
                        &mut self.output.instructions[bge_index]
                    {
                        *target = right_entry;
                    }
                }
                return entry;
            }
        }

        // Interior node: pivot lower-middle once the upper bound is closed, else
        // upper-middle.
        let mid = if khi.is_none() {
            lo + count / 2
        } else {
            lo + (count - 1) / 2
        };
        let pivot = values[mid];
        self.emit_switch_compare(register, pivot);
        self.emit_switch_conditional(patches, BEQ, Target::Body(mid));

        // The single `bge` selects the right range; the fall-through is the left
        // range, emitted inline next (pre-order).
        let bge_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: BGE.0,
                condition_bit: BGE.1,
                target: 0,
            });

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
            let right_entry = self.lower_switch_range(
                register,
                values,
                mid + 1,
                hi,
                Some(pivot + 1),
                khi,
                patches,
            );
            if let Instruction::BranchConditionalForward { target, .. } =
                &mut self.output.instructions[bge_index]
            {
                *target = right_entry;
            }
        }

        entry
    }

    fn emit_switch_compare(&mut self, register: u8, immediate: i64) {
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate {
                a: register,
                immediate: immediate as i16,
            });
    }

    /// Push a forward conditional branch (`(BO, BI)`) bound to `target`.
    fn emit_switch_conditional(
        &mut self,
        patches: &mut Vec<(usize, Target)>,
        options: (u8, u8),
        target: Target,
    ) {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: options.0,
                condition_bit: options.1,
                target: 0,
            });
        patches.push((index, target));
    }

    /// Push an unconditional branch bound to `target`.
    fn emit_switch_branch(&mut self, patches: &mut Vec<(usize, Target)>, target: Target) {
        let index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        patches.push((index, target));
    }
}

/// `beq` — branch if cr0[EQ] (BO=12 branch-if-true, BI=2 the EQ bit).
const BEQ: (u8, u8) = (12, 2);
/// `bge` — branch if not cr0[LT] (BO=4 branch-if-false, BI=0 the LT bit).
const BGE: (u8, u8) = (4, 0);
/// `bgt` — branch if cr0[GT] (BO=12 branch-if-true, BI=1 the GT bit).
const BGT: (u8, u8) = (12, 1);
