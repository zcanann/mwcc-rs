//! Branchless selects, short-circuit && / ||, sign-clamp and mask forms.

#[allow(unused_imports)]
use super::*;

impl Generator {

    /// Emit a short-circuit `&&`/`||` in tail position as mwcc does: each operand
    /// is tested (a leaf against zero, a comparison directly) with an early
    /// conditional return. Each operand may be a leaf or a comparison.
    pub(crate) fn emit_short_circuit(&mut self, operator: BinaryOperator, left: &Expression, right: &Expression, result: u8) -> Compilation<()> {
        // If evaluating the RIGHT operand reads the RESULT register — as a value or through a load
        // base (`a && a`; `p && p[0]` where p is in `result`) — the accumulator (`li result,…`)
        // clobbers a value the right operand still needs, and the scratch-register fallback (r0)
        // then collides with a load through that pointer. mwcc reuses the compare or uses a third
        // register; neither is modeled, so defer rather than emit wrong bytes. (A `(a==c1)||(a==c2)`
        // comparison form, whose operands only TEST the register, still uses the scratch path.)
        let names_in_result: std::collections::HashSet<&str> = self
            .locations
            .iter()
            .filter(|(_, location)| location.register == result)
            .map(|(name, _)| name.as_str())
            .collect();
        // A COMPARISON or nested logical right operand only TESTS the register (`cmpwi`), leaving
        // the value intact, so the scratch path stays byte-exact; exclude those. A leaf/load right
        // operand reads the register as a value and is the unsafe case.
        let right_is_comparison = matches!(
            right,
            Expression::Binary { operator, .. }
                if matches!(
                    operator,
                    BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::LessEqual
                        | BinaryOperator::GreaterEqual | BinaryOperator::Equal | BinaryOperator::NotEqual
                        | BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                )
        );
        if !names_in_result.is_empty() && !right_is_comparison && crate::analysis::reads_register(right, &names_in_result) {
            return Err(mwcc_core::Diagnostic::error("a short-circuit whose right operand reuses the result register is not modeled yet (roadmap)"));
        }
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
                // the branch taken when left is TRUE is the negation of the skip-when-false branch.
                let set_one = self.fresh_label();
                self.emit_branch_conditional_to(left_skip ^ 8, left_bit, set_one);
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: right_skip, condition_bit: right_bit });
                self.bind_label(set_one);
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
        // `(a == c1) || (a == c2)` for CONSECUTIVE constants is, as a VALUE, mwcc's unsigned
        // range check `(unsigned)(a - min) <= 1` — a branchless idiom (`addi; subfic; orc;
        // srwi; subf; srwi.; bnelr`) not reproduced here. Defer rather than emit our
        // compare-branch form (a byte diff). NON-consecutive constants use the same
        // compare-branch idiom as mwcc (byte-exact), and the `if (...)` CONDITION form takes a
        // different path, so both are unaffected.
        if matches!(operator, BinaryOperator::LogicalOr) {
            let as_equality_constant = |expression: &Expression| -> Option<(String, i64)> {
                if let Expression::Binary { operator: BinaryOperator::Equal, left, right } = expression {
                    if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), constant_value(right)) {
                        return Some((name.clone(), constant));
                    }
                    if let (Some(constant), Expression::Variable(name)) = (constant_value(left), right.as_ref()) {
                        return Some((name.clone(), constant));
                    }
                }
                None
            };
            if let (Some((left_variable, left_constant)), Some((right_variable, right_constant))) =
                (as_equality_constant(left), as_equality_constant(right))
            {
                if left_variable == right_variable && (left_constant - right_constant).abs() == 1 {
                    return Err(Diagnostic::error("a consecutive-constant equality OR value is mwcc's unsigned range idiom (roadmap)"));
                }
            }
        }
        match operator {
            BinaryOperator::LogicalAnd => {
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(scratch, 0));
                let exit = self.fresh_label();
                self.emit_branch_conditional_to(left_skip, left_bit, exit);
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                self.emit_branch_conditional_to(right_skip, right_bit, exit);
                self.output.instructions.push(Instruction::load_immediate(scratch, 1));
                self.bind_label(exit);
                if result != scratch {
                    self.output.instructions.push(Instruction::move_register(result, scratch));
                }
            }
            BinaryOperator::LogicalOr => {
                let (left_skip, left_bit) = self.emit_condition_test(left)?;
                self.output.instructions.push(Instruction::load_immediate(scratch, 0));
                let set_one = self.fresh_label();
                self.emit_branch_conditional_to(left_skip ^ 8, left_bit, set_one);
                let (right_skip, right_bit) = self.emit_condition_test(right)?;
                let exit = self.fresh_label();
                self.emit_branch_conditional_to(right_skip, right_bit, exit);
                self.bind_label(set_one);
                self.output.instructions.push(Instruction::load_immediate(scratch, 1));
                self.bind_label(exit);
                if result != scratch {
                    self.output.instructions.push(Instruction::move_register(result, scratch));
                }
            }
            _ => unreachable!("caller restricts to logical and/or"),
        }
        Ok(())
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
    pub(crate) fn try_emit_branchless_mask(&mut self, condition: &Expression, value: &Expression, complement: bool, destination: u8) -> Compilation<bool> {
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
        // A single-op computed value (`a+1`, `a*2`, `a&m`) is evaluated into r0, exactly
        // like a constant materialized there: `neg t,c; <op> r0; or d,t,c; srawi d,31;
        // and/andc d,r0,d`. The `-c` temp `t` must avoid the value's operand registers
        // (else the `neg` clobbers them before the op reads them), so it goes in a fresh
        // virtual the allocator places after them (mwcc's r5). A multi-op value would need
        // temporaries beyond the scratch and defers.
        if self.is_single_op_register_value(value) {
            let temp = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::Negate { d: temp, a: condition_register });
            self.evaluate_general(value, GENERAL_SCRATCH)?;
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
    pub(crate) fn try_emit_sign_clamp(&mut self, condition: &Expression, when_true: &Expression, when_false: &Expression, destination: u8) -> Compilation<bool> {
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
        // This clamp-to-zero SELECT is a ternary: advance mwcc's anonymous-`@N` counter by 3,
        // like the other ternary forms. The value is integer (signedness checked above), so no
        // float guard is needed; only a frame fn's extab numbering observes it.
        self.output.anonymous_label_bump += 3;
        Ok(true)
    }

    /// `(a REL 0) ? b : 0` / `(a REL 0) ? 0 : b` where `b` is a leaf DIFFERENT from the
    /// condition operand `a` — a branchless MASKED SELECT (the clamp `try_emit_sign_clamp`
    /// generalized to a distinct value). mwcc builds the sign mask of `a` (all-ones exactly
    /// when the relation holds) and combines it with `b` via `and` (`? b : 0`) or `andc`
    /// (`? 0 : b`):
    ///   `< 0` : `srawi r0,a,31`
    ///   `> 0` : `neg r0,a; andc r0,r0,a; srawi r0,r0,31`
    ///   `<= 0`: `neg r0,a; orc r0,a,r0; srawi r0,r0,31`
    ///   `>= 0`: `srwi a,a,31; addi r0,a,-1`  (a is dead after the compare, so — unlike the
    ///          clamp, where a is also the value — its register carries the 0/1 flag)
    /// then `and`/`andc r3,b,r0`. Restricted to an in-register destination (the return/assign
    /// context these were measured in); a store (scratch destination) defers.
    pub(crate) fn try_emit_masked_select(&mut self, condition: &Expression, when_true: &Expression, when_false: &Expression, destination: u8) -> Compilation<bool> {
        if destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let Expression::Binary { operator, left, right } = condition else { return Ok(false) };
        if !is_zero_literal(right)
            || !matches!(operator, BinaryOperator::Less | BinaryOperator::Greater | BinaryOperator::GreaterEqual | BinaryOperator::LessEqual)
        {
            return Ok(false);
        }
        let Some(a_name) = leaf_name(left) else { return Ok(false) };
        if !self.signedness_of(left)? {
            return Ok(false);
        }
        // The non-zero arm is a leaf `b` different from `a` (`b == a` is the clamp, handled
        // earlier by try_emit_sign_clamp). `? b : 0` keeps b where the mask is set (`and`);
        // `? 0 : b` keeps b where it is clear (`andc`).
        let (value, use_andc) = if is_zero_literal(when_false) {
            (when_true, false)
        } else if is_zero_literal(when_true) {
            (when_false, true)
        } else {
            return Ok(false);
        };
        let Some(b_name) = leaf_name(value) else { return Ok(false) };
        if b_name == a_name {
            return Ok(false);
        }
        let Some(b) = self.lookup_general(b_name) else { return Ok(false) };
        let x = self.general_register_of_leaf(left)?;
        // The sign mask of `a`, all-ones exactly when the relation holds.
        match operator {
            BinaryOperator::Less => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: x, shift: 31 });
            }
            BinaryOperator::Greater => {
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: x });
                self.output.instructions.push(Instruction::AndComplement { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: x });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
            }
            BinaryOperator::LessEqual => {
                self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: x });
                self.output.instructions.push(Instruction::OrComplement { a: GENERAL_SCRATCH, s: x, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
            }
            _ => {
                // GreaterEqual: `a` is dead after the compare, so its register carries the 0/1 flag.
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: x, s: x, shift: 31 });
                self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: x, immediate: -1 });
            }
        }
        self.output.instructions.push(if use_andc {
            Instruction::AndComplement { a: destination, s: b, b: GENERAL_SCRATCH }
        } else {
            Instruction::And { a: destination, s: b, b: GENERAL_SCRATCH }
        });
        // A ternary select: advance mwcc's anonymous-`@N` counter by 3, like its siblings.
        self.output.anonymous_label_bump += 3;
        Ok(true)
    }

    pub(crate) fn try_emit_consecutive_constants(&mut self, condition: &Expression, when_true: &Expression, when_false: &Expression, destination: u8) -> Compilation<bool> {
        // The truth value comes from a leaf in its register, or — in a tail context
        // — a full-word memory load brought into the destination (`*q ? 1 : 2` is
        // `lwz r3,…; neg r0,r3; or; srawi r3; addi`). A load is taken only after the
        // arms are confirmed (so a non-matching shape emits nothing).
        let (Some(c1), Some(c2)) = (constant_value(when_true), constant_value(when_false)) else {
            return Ok(false);
        };
        if c1 == 0 || c2 == 0 || (c1 - c2).abs() != 1 || i16::try_from(c2).is_err() {
            return Ok(false);
        }
        // A COMPARISON condition `cmp ? c1 : c2` with consecutive arms, INCREASING (c1 > c2): the
        // value is `cmp + c2` — the bare comparison (0/1, computed exactly as `return a REL b`) plus
        // the smaller constant. (The DECREASING case `c1 < c2` is mwcc's `-(cmp) + c1` via a subfc/
        // subfe negated-mask idiom we do not reproduce yet, so it keeps deferring rather than diff.)
        // Only a non-scratch destination (the flag can't share the comparison's r0 in a value context).
        if destination != GENERAL_SCRATCH && c1 > c2 {
            if let Expression::Binary { operator, .. } = condition {
                if is_comparison(*operator) {
                    if let Ok(minimum) = i16::try_from(c2) {
                        self.evaluate_general(condition, destination)?;
                        self.output.instructions.push(Instruction::AddImmediate { d: destination, a: destination, immediate: minimum });
                        return Ok(true);
                    }
                }
            }
        }
        // The truth value comes from a leaf in its register, or — in a tail context
        // — a full-word memory load brought into the destination (`*q ? 1 : 2` is
        // `lwz r3,…; neg r0,r3; or; srawi r3; addi`). A load is taken only after the
        // arms are confirmed (so a non-matching shape emits nothing).
        let leaf_register = leaf_name(condition).and_then(|name| self.lookup_general(name));
        let loadable = leaf_register.is_none() && destination != GENERAL_SCRATCH && self.is_word_load(condition);
        if leaf_register.is_none() && !loadable {
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
    pub(crate) fn place_select_value(&mut self, value: &Expression, destination: u8) -> Compilation<()> {
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

    /// `if (a && b) return X; return Y;` (or `||`) lowers as a short-circuit branching
    /// straight into the two return blocks — mwcc branches each term to the taken or
    /// fall-through return rather than computing the logical operator as a 0/1 value:
    ///
    ///     &&: cmpwi rA,0; beq FALL; cmpwi rB,0; beq FALL; <X>; blr; FALL: <Y>; blr
    ///     ||: cmpwi rA,0; bne TAKEN; cmpwi rB,0; beq FALL; TAKEN: <X>; blr; FALL: <Y>; blr
    ///
    /// Restricted to a single &&/|| chain of leaf/comparison terms (no mixed/nested logical)
    /// with leaf-or-constant return values; anything else returns false to defer.
    pub(crate) fn try_emit_short_circuit_guard(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        result: u8,
    ) -> Compilation<bool> {
        let operator = match condition {
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), .. } => *operator,
            _ => return Ok(false),
        };
        // Flatten the same-operator chain into its terms; a nested different logical operator
        // (mixed `a && b || c`) is left for the general path.
        fn collect<'e>(condition: &'e Expression, operator: BinaryOperator, terms: &mut Vec<&'e Expression>) -> bool {
            match condition {
                Expression::Binary { operator: inner, left, right } if *inner == operator => {
                    collect(left, operator, terms) && collect(right, operator, terms)
                }
                Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, .. } => false,
                _ => {
                    terms.push(condition);
                    true
                }
            }
        }
        let mut terms = Vec::new();
        if !collect(condition, operator, &mut terms) {
            return Ok(false);
        }
        let is_simple = |expression: &Expression| leaf_name(expression).is_some() || constant_value(expression).is_some();
        if !is_simple(when_true) || !is_simple(when_false) {
            return Ok(false);
        }

        // When the taken value already sits in the result register, mwcc drops the separate
        // taken block. For AND the deciding (last) term becomes a conditional return
        // (`cmpwi; bnelr`); for OR every early term is a conditional return (any true term
        // returns the taken value in the result), the last term branches to the fall block,
        // and the last-true path falls through to a trailing `blr`.
        let taken_in_result = leaf_name(when_true).and_then(|name| self.lookup_general(name)) == Some(result);
        let use_conditional_return = taken_in_result;
        // The FALL-THROUGH value already in the result folds an OR's last term into a
        // conditional return (`if (s < 1 || s > 6) return -1; return s;` — the false side
        // of `s > 6` is `blelr`, no fall block at all; the taken block follows).
        let fall_folded = !use_conditional_return
            && operator == BinaryOperator::LogicalOr
            && leaf_name(when_false).and_then(|name| self.lookup_general(name)) == Some(result);

        let mut to_taken = Vec::new();
        let mut to_fall = Vec::new();
        let last = terms.len() - 1;
        for (index, term) in terms.iter().enumerate() {
            let (options, condition_bit) = self.emit_condition_test(term)?;
            let branch_index = self.output.instructions.len();
            if operator == BinaryOperator::LogicalAnd {
                if use_conditional_return && index == last {
                    // The all-true path returns the taken value already in the result.
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                } else {
                    // Any false term fails the AND -> fall-through return.
                    self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                    to_fall.push(branch_index);
                }
            } else if index == last {
                if fall_folded {
                    // The last term's FALSE side returns the fall-through value directly.
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                } else {
                    // OR: the last term false branches to the fall block; true falls through.
                    self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                    to_fall.push(branch_index);
                }
            } else if use_conditional_return {
                // OR taken-in-result: an early true term returns the taken value in the result.
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
            } else {
                // OR: an early true term jumps to the taken block.
                self.output.instructions.push(Instruction::BranchConditionalForward { options: options ^ 8, condition_bit, target: 0 });
                to_taken.push(branch_index);
            }
        }
        // OR taken-in-result: the last-true path falls through here, returning the taken value.
        if use_conditional_return && operator == BinaryOperator::LogicalOr {
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        }
        // The taken (guard) block sits right after the short-circuit (the all-true / last-true
        // path falls into it); the fall-through return follows it. With the conditional-return
        // form the taken value is already returned, so only the fall block remains.
        if !use_conditional_return {
            let taken_block = self.output.instructions.len();
            self.place_select_value(when_true, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            for branch_index in to_taken {
                self.patch_forward(branch_index, taken_block);
            }
        }
        // With the fall side folded into the last term's conditional return, there is no
        // fall block (nothing branches to one).
        if !fall_folded {
            let fall_block = self.output.instructions.len();
            self.place_select_value(when_false, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            for branch_index in to_fall {
                self.patch_forward(branch_index, fall_block);
            }
        }
        Ok(true)
    }

}
