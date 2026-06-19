//! Conditionals, float selects, short-circuit `&&`/`||`, branch tests.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, UnaryOperator};
use crate::analysis::*;
use crate::generator::*;

impl Generator {

    /// Emit a short-circuit `&&`/`||` in tail position as mwcc does: each operand
    /// is tested (a leaf against zero, a comparison directly) with an early
    /// conditional return. Each operand may be a leaf or a comparison.
    pub(crate) fn emit_short_circuit(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, result: u8) -> Compilation<()> {
        // If the right operand still reads the result register, the running result
        // cannot live there; mwcc computes it in r0 and copies it out at the end.
        if self.registers_used_by(right).contains(&result) {
            return self.emit_short_circuit_via_scratch(operator, left, right, result);
        }
        match operator {
            BinaryOperator::LogicalAnd => {
                // test left; result 0; return 0 if left false; test right; return 0 if right false; result 1.
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(result, 0));
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: left_skip, condition_bit: left_bit });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: right_skip, condition_bit: right_bit });
                self.output.instructions.push(Instruction::load_immediate(result, 1));
            }
            BinaryOperator::LogicalOr => {
                // test left; result 0; if left true skip to result 1; test right; return 0 if right false; result 1.
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(result, 0));
                let branch_index = self.output.instructions.len();
                // the branch taken when left is TRUE is the negation of the skip-when-false branch.
                self.output.instructions.push(Instruction::BranchConditionalForward { options: left_skip ^ 8, condition_bit: left_bit, target: 0 });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: right_skip, condition_bit: right_bit });
                let set_one = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = set_one;
                }
                self.output.instructions.push(Instruction::load_immediate(result, 1));
            }
            _ => unreachable!("caller restricts to logical and/or"),
        }
        Ok(())
    }

    /// Short-circuit `&&`/`||` whose result is built in the scratch register and
    /// copied to the destination at a common exit — used when the destination
    /// register is still needed by the right operand.
    pub(crate) fn emit_short_circuit_via_scratch(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, result: u8) -> Compilation<()> {
        let scratch = GENERAL_SCRATCH;
        match operator {
            BinaryOperator::LogicalAnd => {
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(scratch, 0));
                let exit_a = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: left_skip, condition_bit: left_bit, target: 0 });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                let exit_b = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: right_skip, condition_bit: right_bit, target: 0 });
                self.output.instructions.push(Instruction::load_immediate(scratch, 1));
                let exit = self.output.instructions.len();
                self.patch_forward(exit_a, exit);
                self.patch_forward(exit_b, exit);
                self.output.instructions.push(Instruction::move_register(result, scratch));
            }
            BinaryOperator::LogicalOr => {
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(scratch, 0));
                let to_set_one = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: left_skip ^ 8, condition_bit: left_bit, target: 0 });
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                let to_exit = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options: right_skip, condition_bit: right_bit, target: 0 });
                let set_one = self.output.instructions.len();
                self.output.instructions.push(Instruction::load_immediate(scratch, 1));
                let exit = self.output.instructions.len();
                self.patch_forward(to_set_one, set_one);
                self.patch_forward(to_exit, exit);
                self.output.instructions.push(Instruction::move_register(result, scratch));
            }
            _ => unreachable!("caller restricts to logical and/or"),
        }
        Ok(())
    }

    pub(crate) fn patch_forward(&mut self, branch_index: usize, target: usize) {
        if let Instruction::BranchConditionalForward { target: slot, .. } = &mut self.output.instructions[branch_index] {
            *slot = target;
        }
    }

    /// Emit a ternary `condition ? when_true : when_false` into `destination`,
    /// matching mwcc's shape: the false value's register is the working register,
    /// conditionally overwritten with the true value, then moved to the result.
    /// Leaf operands only for now.
    /// Branchless `cond ? value : 0` (`complement` false → `and`) or
    /// `cond ? 0 : value` (`complement` true → `andc`): build a mask that is
    /// all-ones when `cond != 0` (`neg`/`or`/`srawi`), then combine it with
    /// `value`. A leaf value keeps the mask in r0; a non-zero constant is
    /// materialized in r0, so the mask instead flows through a free register and
    /// the destination. The condition must be a plain (truthy) leaf.
    fn try_emit_branchless_mask(&mut self, condition: &Expression, value: &Expression, complement: bool, destination: u8) -> Compilation<bool> {
        // The condition is a leaf in its register, or — in a tail context with a
        // leaf value that does not occupy the destination — a full-word load brought
        // into the destination first (`*q ? x : 0` is `lwz r3; neg; or; srawi; and`).
        let value_leaf = leaf_name(value).and_then(|name| self.lookup_general(name));
        let condition_register = if let Some(register) = leaf_name(condition).and_then(|name| self.lookup_general(name)) {
            register
        } else if destination != GENERAL_SCRATCH
            && self.is_word_load(condition)
            && value_leaf.is_some()
            && value_leaf != Some(destination)
        {
            self.evaluate_general(condition, destination)?;
            destination
        } else {
            return Ok(false);
        };
        let combine = |destination: u8, source: u8, mask: u8| {
            if complement {
                Instruction::AndComplement { a: destination, s: source, b: mask }
            } else {
                Instruction::And { a: destination, s: source, b: mask }
            }
        };
        if let Some(value_register) = leaf_name(value).and_then(|name| self.lookup_general(name)) {
            // Leaf value: the mask lives in r0.
            self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: condition_register });
            self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: condition_register });
            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
            self.output.instructions.push(combine(destination, value_register, GENERAL_SCRATCH));
            return Ok(true);
        }
        if let Some(constant) = constant_value(value) {
            // `cond ? -1 : 0` is exactly the all-ones-when-true mask — no `and`.
            if constant == -1 && !complement {
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: condition_register });
                self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: condition_register });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: destination, s: GENERAL_SCRATCH, shift: 31 });
                return Ok(true);
            }
            // Constant value: it occupies r0, so the mask computes through a free
            // register (`neg`) and the destination (`or`/`srawi`).
            let Some(temp) = (3u8..=12).find(|r| *r != condition_register && !self.reserved.contains(r)) else {
                return Ok(false);
            };
            self.output.instructions.push(Instruction::Negate { d: temp, a: condition_register });
            self.load_integer_constant(GENERAL_SCRATCH, constant);
            self.output.instructions.push(Instruction::Or { a: destination, s: temp, b: condition_register });
            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: destination, s: destination, shift: 31 });
            self.output.instructions.push(combine(destination, GENERAL_SCRATCH, destination));
            return Ok(true);
        }
        Ok(false)
    }

    /// `cond ? c1 : c2` with a truthy leaf condition and consecutive non-zero
    /// constants: `neg`/`or` form the truth value (the sign bit of `-cond|cond`),
    /// then `srawi` (a -1/0 mask when the true value is lower) or `srwi` (a 0/1
    /// bool when it is higher), and `addi` the lower constant.
    /// `(x REL 0) ? x : 0` / `(x REL 0) ? 0 : x` (a clamp-to-zero): the sign mask
    /// of x combined with x via `and`/`andc`. The base mask is `srawi x,31` for the
    /// `<0` conditions and `neg; andc; srawi` for the `>0` conditions; which arm
    /// keeps x and whether the condition is the negated (`>=`/`<=`) sense pick
    /// `and` vs `andc`.
    fn try_emit_sign_clamp(&mut self, condition: &Expression, when_true: &Expression, when_false: &Expression, destination: u8) -> Compilation<bool> {
        let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
        if !is_zero_literal(right) {
            return Ok(false);
        }
        let Some(x_name) = leaf_name(left) else { return Ok(false) };
        let x_is_true = is_zero_literal(when_false) && leaf_name(when_true) == Some(x_name);
        let x_is_false = is_zero_literal(when_true) && leaf_name(when_false) == Some(x_name);
        if !(x_is_true || x_is_false) || !self.signedness_of(left)? {
            return Ok(false);
        }
        // `< 0` uses a `srawi` sign mask; `> 0` uses a `neg; andc; srawi` mask.
        // (`>= 0` / `<= 0` use different sequences — `srwi; addi` / `neg; orc;
        // srawi` — so they defer here rather than reuse these via and<->andc.)
        if !matches!(operator, BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::GreaterEqual | BinaryOperator::LessEqual) {
            return Ok(false);
        }
        let use_andc = x_is_false;
        let x = self.general_register_of_leaf(left)?;
        // The mask (all-ones exactly when the condition holds) goes in the scratch;
        // each relation builds it differently.
        match operator {
            // x < 0: the sign bit broadcast.
            BinaryOperator::Less => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: x, shift: 31 });
            }
            // x > 0: `(-x) & ~x` has the sign bit set iff x > 0.
            BinaryOperator::Greater => {
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: x });
                self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: x });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
            }
            // x >= 0: `(x >>> 31) - 1` (0/1 then minus one) — needs a free register.
            BinaryOperator::GreaterEqual => {
                let Some(free) = (3u8..=12).find(|r| *r != x && *r != destination && !self.reserved.contains(r)) else {
                    return Ok(false);
                };
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: free, s: x, shift: 31 });
                self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: free, immediate: -1 });
            }
            // x <= 0: `(x | ~(-x))` has the sign bit set iff x <= 0.
            _ => {
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: x });
                self.output.instructions.push(Instruction::OrComplement { a: GENERAL_SCRATCH, s: x, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
            }
        }
        self.output.instructions.push(if use_andc {
            Instruction::AndComplement { a: destination, s: x, b: GENERAL_SCRATCH }
        } else {
            Instruction::And { a: destination, s: x, b: GENERAL_SCRATCH }
        });
        Ok(true)
    }

    fn try_emit_consecutive_constants(&mut self, condition: &Expression, when_true: &Expression, when_false: &Expression, destination: u8) -> Compilation<bool> {
        // The truth value comes from a leaf in its register, or — in a tail context
        // — a full-word memory load brought into the destination (`*q ? 1 : 2` is
        // `lwz r3,…; neg r0,r3; or; srawi r3; addi`). A load is taken only after the
        // arms are confirmed (so a non-matching shape emits nothing).
        let leaf_register = leaf_name(condition).and_then(|name| self.lookup_general(name));
        let loadable = leaf_register.is_none() && destination != GENERAL_SCRATCH && self.is_word_load(condition);
        if leaf_register.is_none() && !loadable {
            return Ok(false);
        }
        let (Some(c1), Some(c2)) = (constant_value(when_true), constant_value(when_false)) else {
            return Ok(false);
        };
        if c1 == 0 || c2 == 0 || (c1 - c2).abs() != 1 || i16::try_from(c2).is_err() {
            return Ok(false);
        }
        let cond_register = match leaf_register {
            Some(register) => register,
            None => {
                self.evaluate_general(condition, destination)?;
                destination
            }
        };
        // The `neg`/`or` use r0; when the destination *is* r0 (a value/store
        // context) the mask goes to a fresh register so the final `addi` can land
        // in r0, matching mwcc. In a tail context the mask uses the destination.
        let mask_register = if destination == GENERAL_SCRATCH {
            self.fresh_virtual_general()
        } else {
            destination
        };
        self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: cond_register });
        self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: cond_register });
        if c1 < c2 {
            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: mask_register, s: GENERAL_SCRATCH, shift: 31 });
        } else {
            self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: mask_register, s: GENERAL_SCRATCH, shift: 31 });
        }
        self.output.instructions.push(Instruction::AddImmediate { d: destination, a: mask_register, immediate: c2 as i16 });
        Ok(true)
    }

    /// Place a select arm into the result: a constant is materialized with `li`
    /// (or `lis`/`ori`); a leaf variable is moved unless it already sits there.
    fn place_select_value(&mut self, value: &Expression, destination: u8) -> Compilation<()> {
        if let Some(constant) = constant_value(value) {
            self.load_integer_constant(destination, constant);
            return Ok(());
        }
        let register = self.general_register_of_leaf(value)?;
        if register != destination {
            self.output.instructions.push(Instruction::move_register(destination, register));
        }
        Ok(())
    }

    pub(crate) fn emit_conditional(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<()> {
        // `comparison ? 1 : 0` is the comparison; `comparison ? 0 : 1` is its negation.
        if let Expression::Binary { operator, left, right } = condition {
            if is_comparison(*operator) {
                match (constant_value(when_true), constant_value(when_false)) {
                    (Some(1), Some(0)) => return self.evaluate_general(condition, destination),
                    (Some(0), Some(1)) => {
                        let flipped = flip_comparison(*operator).unwrap();
                        return self.emit_comparison(flipped, left, right, destination);
                    }
                    _ => {}
                }
            }
        }

        // For a non-comparison condition, `cond ? 1 : 0` is the truthiness `cond != 0`
        // and `cond ? 0 : 1` is `cond == 0` — and the value (even a complex one) now
        // computes through the comparison idioms, which the allocator unlocked.
        let condition_is_comparison = matches!(condition, Expression::Binary { operator, .. } if is_comparison(*operator));
        if !condition_is_comparison {
            let zero = Expression::IntegerLiteral(0);
            match (constant_value(when_true), constant_value(when_false)) {
                (Some(1), Some(0)) => return self.emit_comparison(BinaryOperator::NotEqual, condition, &zero, destination),
                (Some(0), Some(1)) => return self.emit_comparison(BinaryOperator::Equal, condition, &zero, destination),
                _ => {}
            }
        }

        // `x < 0 ? -1 : 0` (and its mirror `x >= 0 ? 0 : -1`) is the sign mask:
        // arithmetic-shift the sign bit across the word, `srawi d, x, 31`. The
        // complement `x < 0 ? 0 : -1` instead broadcasts the inverted sign,
        // `srwi d, x, 31; addi d, d, -1` (0/1 then minus one).
        if let Some((value, complemented)) = sign_mask_select(condition, when_true, when_false) {
            if self.signedness_of(value)? {
                let register = self.general_register_of_leaf(value)?;
                if complemented {
                    self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: destination, s: register, shift: 31 });
                    self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: -1 });
                } else {
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: destination, s: register, shift: 31 });
                }
                return Ok(());
            }
        }

        // `(x < 0)` / `(x >= 0)` / `(x > 0)` `? c1 : c2` with consecutive non-zero
        // constants: the shifted sign bit (`srawi`/`srwi`) plus an offset, after a
        // `neg; andc` preamble for the `> 0` case (which needs the scratch, so it
        // only applies when the destination is not the scratch).
        if let Some(select) = sign_consecutive_select(condition, when_true, when_false) {
            if self.signedness_of(select.value)? && !(select.positive && destination == GENERAL_SCRATCH) {
                let register = self.general_register_of_leaf(select.value)?;
                let shift_source = if select.positive {
                    self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: register });
                    self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: register });
                    GENERAL_SCRATCH
                } else {
                    register
                };
                self.output.instructions.push(if select.arithmetic {
                    Instruction::ShiftRightAlgebraicImmediate { a: destination, s: shift_source, shift: 31 }
                } else {
                    Instruction::ShiftRightLogicalImmediate { a: destination, s: shift_source, shift: 31 }
                });
                self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: select.offset });
                return Ok(());
            }
        }

        // `(x REL 0) ? x : 0` (clamp-to-zero): a sign mask of x combined with x.
        if self.try_emit_sign_clamp(condition, when_true, when_false, destination)? {
            return Ok(());
        }

        // `cond ? c1 : c2` with consecutive non-zero constants is branchless: the
        // truth value (a -1/0 sign mask or a 0/1 bool) plus the lower constant.
        if self.try_emit_consecutive_constants(condition, when_true, when_false, destination)? {
            return Ok(());
        }

        // `cond ? x : 0` (AND) and `cond ? 0 : x` (ANDC) with a plain truth
        // condition are branchless: a mask all-ones when cond != 0, combined with
        // `x` (a leaf, or a non-zero constant materialized in r0).
        if is_zero_literal(when_false) && !is_zero_literal(when_true) {
            if self.try_emit_branchless_mask(condition, when_true, false, destination)? {
                return Ok(());
            }
        }
        if is_zero_literal(when_true) && !is_zero_literal(when_false) {
            if self.try_emit_branchless_mask(condition, when_false, true, destination)? {
                return Ok(());
            }
        }

        // `(x < 0) ? -x : x` is the branchless abs idiom: a sign mask via srawi,
        // then `(x ^ mask) - mask`. mwcc emits `srawi t,x,31; xor r0,t,x; subf d,t,r0`.
        if let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition {
            if is_zero_literal(right) && self.signedness_of(left)? {
                if let Expression::Unary { operator: UnaryOperator::Negate, operand } = when_true {
                    if let (Some(condition_name), Some(negated_name), Some(false_name)) =
                        (leaf_name(left), leaf_name(operand), leaf_name(when_false))
                    {
                        if condition_name == negated_name && condition_name == false_name {
                            let x = self.general_register_of_leaf(left)?;
                            let mask = self.fresh_virtual_general();
                            self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: mask, s: x, shift: 31 });
                            self.output.instructions.push(Instruction::Xor { a: GENERAL_SCRATCH, s: mask, b: x });
                            self.output.instructions.push(Instruction::SubtractFrom { d: destination, a: mask, b: GENERAL_SCRATCH });
                            return Ok(());
                        }
                    }
                }
            }
        }

        // mwcc only branches for NON-consecutive constant arms; two consecutive
        // constants (|c1-c2| == 1) always take a branchless mask form. If the
        // branchless path above could not produce it (an unhandled condition),
        // we must defer rather than emit the mismatching branch form.
        let consecutive_constants = matches!(
            (constant_value(when_true), constant_value(when_false)),
            (Some(a), Some(b)) if (a - b).abs() == 1
        );
        // A select with a non-zero constant arm uses a branch, not a register
        // move: mwcc tests the condition, materializes the constant-bearing arm
        // into the result, conditional-returns on that arm's branch, then the
        // other arm. When both are constant the false arm goes first (`beqlr`). A
        // zero arm instead uses the branchless and/andc forms above (with the
        // other arm materialized), whose register layout differs — those defer.
        if tail
            && !consecutive_constants
            && !is_zero_literal(when_true)
            && !is_zero_literal(when_false)
            && (constant_value(when_false).is_some() || constant_value(when_true).is_some())
        {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if constant_value(when_false).is_some() {
                // false-first: place false, return on the false branch, then true.
                self.place_select_value(when_false, destination)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                self.place_select_value(when_true, destination)?;
            } else {
                // true-first: place true, return on the negated (true) branch, then false.
                self.place_select_value(when_true, destination)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                self.place_select_value(when_false, destination)?;
            }
            return Ok(());
        }

        let true_register = self.general_register_of_leaf(when_true)?;
        let false_register = self.general_register_of_leaf(when_false)?;

        // Emit the condition test and the branch that skips the true path when it fails.
        let (options, condition_bit) = self.emit_condition_test(condition)?;

        // In tail position, when the false value already sits in the result
        // register, return early on the false path instead of branching forward.
        if tail && false_register == destination {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            if destination != true_register {
                self.output.instructions.push(Instruction::move_register(destination, true_register));
            }
            return Ok(());
        }

        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.output.instructions.push(Instruction::move_register(false_register, true_register));

        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        if destination != false_register {
            self.output.instructions.push(Instruction::move_register(destination, false_register));
        }
        Ok(())
    }

    /// Emit a float `condition ? when_true : when_false`. The condition must be a
    /// float comparison; in tail position, when one branch value already sits in
    /// the result register, return early on that branch (fcmpo + bclr).
    /// Place the two operands of a float comparison. A leaf variable stays in its
    /// register; a memory-loaded left operand loads into a free register (avoiding
    /// the `reserved` select-value registers), the right into the scratch; a float
    /// constant loads into the scratch.
    fn place_float_comparison_operands(&mut self, left: &Expression, right: &Expression, reserved: &[u8]) -> Compilation<(u8, u8)> {
        let left_register = if self.is_float_located(left) {
            let newly: Vec<u8> = reserved.iter().copied().filter(|register| self.reserved.insert(*register)).collect();
            let register = self.lowest_free_float();
            for register in &newly {
                self.reserved.remove(register);
            }
            let register = register?;
            self.emit_located_operand(left, register)?;
            register
        } else {
            self.float_register_of_leaf(left)?
        };
        let right_register = if let Expression::FloatLiteral(value) = right {
            self.load_float_constant(FLOAT_SCRATCH, *value as f32);
            FLOAT_SCRATCH
        } else if self.is_float_located(right) {
            self.emit_located_operand(right, FLOAT_SCRATCH)?;
            FLOAT_SCRATCH
        } else {
            self.float_register_of_leaf(right)?
        };
        Ok((left_register, right_register))
    }

    pub(crate) fn emit_float_conditional(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<()> {
        let Expression::Binary { operator, left, right } = condition else {
            return Err(Diagnostic::error("float conditional needs a comparison condition"));
        };
        if !is_comparison(*operator) {
            return Err(Diagnostic::error("float conditional needs a comparison condition"));
        }
        // A float conditional branch advances mwcc's anonymous-`@N` counter by 3.
        self.output.has_float_branch = true;
        let true_register = self.float_register_of_leaf(when_true)?;
        let false_register = self.float_register_of_leaf(when_false)?;
        // The condition operands may be memory loads: a located left operand loads
        // into a free register (avoiding the select values), the right into the
        // scratch; leaf operands stay in place.
        let (left_register, right_register) = self.place_float_comparison_operands(left, right, &[true_register, false_register])?;

        self.output.instructions.push(Instruction::FloatCompareOrdered { a: left_register, b: right_register });
        let (positive_options, condition_bit) = positive_branch(*operator);

        if tail && true_register == destination {
            // true value already in the result: return on the true branch.
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: positive_options, condition_bit });
            if destination != false_register {
                self.output.instructions.push(Instruction::FloatMove { d: destination, b: false_register });
            }
            return Ok(());
        }
        if tail && false_register == destination {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: positive_options ^ 8, condition_bit });
            if destination != true_register {
                self.output.instructions.push(Instruction::FloatMove { d: destination, b: true_register });
            }
            return Ok(());
        }
        Err(Diagnostic::error("non-tail float select not yet supported"))
    }

    /// Emit the test for a branch condition and return the `(BO, BI)` of the
    /// branch that skips the guarded code when the condition is **false**. A
    /// comparison condition uses `cmpw`/`cmpwi` with the negated relation; any
    /// other expression is tested against zero (`!= 0`).
    pub(crate) fn emit_condition_test(&mut self, condition: &Expression) -> Compilation<(u8, u8)> {
        // `!x` as a condition is `x == 0`: skip the guarded code when x != 0.
        if let Expression::Unary { operator: UnaryOperator::LogicalNot, operand } = condition {
            // `!(x & mask)` is the negated bit-test: rlwinm. then `bne` (skip when
            // the masked bits are set, so the body runs only when they are clear).
            if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = operand.as_ref() {
                if let (Some(register), Some(constant)) =
                    (leaf_name(left).and_then(|name| self.lookup_general(name)), constant_value(right))
                {
                    if let Some((begin, end)) = mask_to_run(constant as u32) {
                        self.output.instructions.push(Instruction::AndMaskRecord { a: GENERAL_SCRATCH, s: register, begin, end });
                        return Ok((4, 2)); // bne — skip when the masked bits are set
                    }
                }
            }
            let register = self.condition_operand_register(operand)?;
            self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
            return Ok((4, 2)); // bne — skip when x != 0
        }
        if let Expression::Binary { operator, left, right } = condition {
            if is_comparison(*operator) {
                // A member on both sides would both want the scratch; defer.
                if as_member(left).is_some() && as_member(right).is_some() {
                    return Err(Diagnostic::error("comparison of two members as a condition (roadmap)"));
                }
                let signed = self.signedness_of(left)? && self.signedness_of(right)?;
                let left_register = self.condition_operand_register(left)?;
                match (as_small_integer(right), is_zero_literal(right)) {
                    (Some(constant), _) if signed => {
                        self.output.instructions.push(Instruction::CompareWordImmediate { a: left_register, immediate: constant });
                    }
                    (Some(constant), _) => {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: left_register, immediate: constant as u16 });
                    }
                    (None, true) if signed => {
                        self.output.instructions.push(Instruction::CompareWordImmediate { a: left_register, immediate: 0 });
                    }
                    (None, true) => {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: left_register, immediate: 0 });
                    }
                    (None, false) => {
                        let right_register = self.condition_operand_register(right)?;
                        if signed {
                            self.output.instructions.push(Instruction::CompareWord { a: left_register, b: right_register });
                        } else {
                            self.output.instructions.push(Instruction::CompareLogicalWord { a: left_register, b: right_register });
                        }
                    }
                }
                // Branch when the comparison is false. BO: 4 = if-false, 12 = if-true. BI: 0=LT,1=GT,2=EQ.
                return Ok(match operator {
                    BinaryOperator::Greater => (4, 1),      // ble
                    BinaryOperator::Less => (4, 0),         // bge
                    BinaryOperator::GreaterEqual => (12, 0), // blt
                    BinaryOperator::LessEqual => (12, 1),    // bgt
                    BinaryOperator::Equal => (4, 2),         // bne
                    BinaryOperator::NotEqual => (12, 2),     // beq
                    _ => unreachable!("is_comparison restricts the operator"),
                });
            }
        }
        // `if (x & mask)` tests the masked bits with a record-form `rlwinm.` that
        // sets cr0 directly — no separate compare.
        if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = condition {
            if let (Some(register), Some(constant)) =
                (leaf_name(left).and_then(|name| self.lookup_general(name)), constant_value(right))
            {
                if let Some((begin, end)) = mask_to_run(constant as u32) {
                    self.output.instructions.push(Instruction::AndMaskRecord { a: GENERAL_SCRATCH, s: register, begin, end });
                    return Ok((12, 2)); // beq — skip when the masked bits are all zero
                }
            }
        }
        // Plain truth test: compare against zero, skip when equal.
        let register = self.condition_operand_register(condition)?;
        self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
        Ok((12, 2)) // beq — skip when condition == 0
    }

    /// The register holding a condition operand: a leaf variable stays in its home
    /// register; a struct member loads into the scratch (mwcc compares `r0`).
    pub(crate) fn condition_operand_register(&mut self, operand: &Expression) -> Compilation<u8> {
        if let Some((base, offset, member_type)) = as_member(operand) {
            self.emit_member_load(base, offset, member_type, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        // A full-word memory load (`*p`, `a[i]`) goes into the scratch; the caller
        // then compares it. (A narrow signed load needs a record-form extend
        // instead of a compare, so it is not taken here.)
        if self.is_word_load(operand) {
            self.evaluate_general(operand, GENERAL_SCRATCH)?;
            return Ok(GENERAL_SCRATCH);
        }
        self.general_register_of_leaf(operand)
    }
}

/// Recognize a sign-mask select on `x`, returning `(x, complemented)`:
///   `x < 0 ? -1 : 0` / `x >= 0 ? 0 : -1` → `(x, false)` — plain sign mask.
///   `x < 0 ? 0 : -1` / `x >= 0 ? -1 : 0` → `(x, true)`  — inverted sign mask.
fn sign_mask_select<'e>(condition: &'e Expression, when_true: &'e Expression, when_false: &'e Expression) -> Option<(&'e Expression, bool)> {
    let Expression::Binary { operator, left, right } = condition else { return None };
    if !is_zero_literal(right) {
        return None;
    }
    // Normalize the arms to (negative-case value, nonnegative-case value).
    let (negative_arm, nonnegative_arm) = match operator {
        BinaryOperator::Less => (when_true, when_false),         // x < 0 ? a : b
        BinaryOperator::GreaterEqual => (when_false, when_true), // x >= 0 ? b : a
        _ => return None,
    };
    if constant_value(negative_arm) == Some(-1) && is_zero_literal(nonnegative_arm) {
        Some((left.as_ref(), false)) // -1 when negative, 0 otherwise
    } else if is_zero_literal(negative_arm) && constant_value(nonnegative_arm) == Some(-1) {
        Some((left.as_ref(), true)) // 0 when negative, -1 otherwise
    } else {
        None
    }
}

/// A recognized sign-compare select with consecutive non-zero constant arms.
/// `value` is the compared operand; `arithmetic` picks `srawi` (`-1/0`) vs `srwi`
/// (`0/1`); `offset` is the trailing `addi`. When `positive` is set the truth is
/// `x > 0`, needing a `neg; andc` preamble to form the mask base from `x`.
struct SignConsecutive<'e> {
    value: &'e Expression,
    positive: bool,
    arithmetic: bool,
    offset: i16,
}

/// Recognize a sign-compare select with consecutive non-zero constant arms —
/// `(x < 0)`, `(x >= 0)`, or `(x > 0)` `? c1 : c2` with `|c1-c2| == 1`. The
/// shifted sign bit (`srawi`/`srwi x,31`, optionally after `neg; andc` for the
/// `> 0` case) plus an offset reproduces the two constants.
fn sign_consecutive_select<'e>(condition: &'e Expression, when_true: &Expression, when_false: &Expression) -> Option<SignConsecutive<'e>> {
    let Expression::Binary { operator, left, right } = condition else { return None };
    if !is_zero_literal(right) {
        return None;
    }
    let (c1, c2) = (constant_value(when_true)?, constant_value(when_false)?);
    if c1 == 0 || c2 == 0 || (c1 - c2).abs() != 1 {
        return None;
    }
    let value = left.as_ref();
    match operator {
        // x < 0 ? c1 : c2 — mask + c2; the mask is -1/0 (srawi) when the negative
        // arm c1 is the lower constant, else 0/1 (srwi). Both orders match mwcc.
        BinaryOperator::Less => Some(SignConsecutive { value, positive: false, arithmetic: c1 < c2, offset: i16::try_from(c2).ok()? }),
        // x >= 0 ? c1 : c2 — only the c1<c2 order is this clean `srwi d,x,31; addi c1`
        // form (the negative arm c2 is the higher constant). The reverse order uses
        // an extra `xori`, so defer it rather than emit the two-instruction shape.
        BinaryOperator::GreaterEqual if c1 < c2 => Some(SignConsecutive { value, positive: false, arithmetic: false, offset: i16::try_from(c1).ok()? }),
        // x > 0 ? c1 : c2 — `neg r0,x; andc r0,r0,x` sets bit 31 iff x > 0, then the
        // same srawi/srwi + addi c2. Both arm orders match mwcc.
        BinaryOperator::Greater => Some(SignConsecutive { value, positive: true, arithmetic: c1 < c2, offset: i16::try_from(c2).ok()? }),
        _ => None,
    }
}
