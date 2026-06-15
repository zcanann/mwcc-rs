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

        // `cond ? x : 0` with a plain truth condition is branchless: AND x with a
        // mask that is all-ones when cond != 0.
        if is_zero_literal(when_false) {
            if let (Some(condition_name), Some(value_name)) = (leaf_name(condition), leaf_name(when_true)) {
                if let (Some(condition_register), Some(value_register)) =
                    (self.lookup_general(condition_name), self.lookup_general(value_name))
                {
                    self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: condition_register });
                    self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: condition_register });
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
                    self.output.instructions.push(Instruction::And { a: destination, s: value_register, b: GENERAL_SCRATCH });
                    return Ok(());
                }
            }
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
        self.general_register_of_leaf(operand)
    }
}
