//! Build-163 shared-register lowering for simple integer selects.

use super::*;

impl Generator {
    /// Build 163's leaf/computed tail selects normally merge through the leaf
    /// register when the leaf is the true arm. A power-of-two multiply is the
    /// exception: mwcc mutates the ABI result register in place and returns from
    /// each arm, regardless of which side contains the multiply.
    pub(crate) fn try_emit_legacy_leaf_computed_tail_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
        origin: ConditionalOrigin,
    ) -> Compilation<bool> {
        if self.behavior.integer_select_style
            != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || self.non_leaf
            || !tail
            || origin == ConditionalOrigin::IfAssignments
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
        {
            return Ok(false);
        }
        if super::absolute_value::absolute_value_target(condition, when_true, when_false).is_some() {
            return Ok(false);
        }
        let true_register = leaf_name(when_true).and_then(|name| self.lookup_general(name));
        let false_register = leaf_name(when_false).and_then(|name| self.lookup_general(name));
        if true_register == Some(destination) || false_register == Some(destination) {
            return Ok(false);
        }
        let true_computed = self.is_single_op_register_value(when_true);
        let false_computed = self.is_single_op_register_value(when_false);
        if !((true_register.is_some() && false_computed)
            || (true_computed && false_register.is_some()))
        {
            return Ok(false);
        }

        let power_of_two_multiply = |arm: &Expression| {
            matches!(arm,
                Expression::Binary { operator: BinaryOperator::Multiply, left, right }
                    if [left.as_ref(), right.as_ref()].iter().any(|operand|
                        constant_value(operand).is_some_and(|value|
                            value > 0 && (value & (value - 1)) == 0)))
        };
        let computed_arm = if true_computed {
            when_true
        } else {
            when_false
        };
        if power_of_two_multiply(computed_arm) {
            self.output.anonymous_label_bump += 3;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let false_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.evaluate_general(when_true, destination)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            let false_arm = self.output.instructions.len();
            self.patch_forward(false_branch, false_arm);
            self.evaluate_general(when_false, destination)?;
            return Ok(true);
        }

        let Some(phi) = true_register else {
            return Ok(false);
        };
        self.emit_legacy_phi_merge(condition, when_true, when_false, phi, true)?;
        if destination != phi {
            self.output
                .instructions
                .push(Instruction::move_register(destination, phi));
        }
        Ok(true)
    }

    /// Build 163 keeps a select containing one or two single-op computed arms
    /// as explicit control flow. A tail uses two return paths; a store/scratch
    /// value uses a full diamond. The other arm may be a 16-bit constant.
    pub(crate) fn try_emit_legacy_computed_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
        origin: ConditionalOrigin,
    ) -> Compilation<bool> {
        if self.behavior.integer_select_style
            != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || self.non_leaf
            || (!tail && destination != GENERAL_SCRATCH)
            || origin == ConditionalOrigin::IfAssignments
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
        {
            return Ok(false);
        }
        let true_computed = self.is_single_op_register_value(when_true);
        let false_computed = self.is_single_op_register_value(when_false);
        let constant_fits = |arm: &Expression| {
            constant_value(arm).is_some_and(|value| i16::try_from(value).is_ok())
        };
        if !(true_computed || constant_fits(when_true))
            || !(false_computed || constant_fits(when_false))
            || !(true_computed || false_computed)
        {
            return Ok(false);
        }

        self.output.anonymous_label_bump += 3;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let false_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.evaluate_general(when_true, destination)?;
        let join_branch = if tail {
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            None
        } else {
            let branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            Some(branch)
        };
        let false_arm = self.output.instructions.len();
        self.patch_forward(false_branch, false_arm);
        self.evaluate_general(when_false, destination)?;
        if let Some(join_branch) = join_branch {
            let join = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
                *target = join;
            }
        }
        Ok(true)
    }

    /// A direct leaf-to-leaf ternary used as a store value merges into the true
    /// arm's register in build 163. Return the register so the store can consume
    /// it without forcing the value through the ABI result register or scratch.
    pub(crate) fn try_emit_legacy_store_phi_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        origin: ConditionalOrigin,
    ) -> Compilation<Option<u8>> {
        if self.behavior.integer_select_style
            != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || self.non_leaf
            || origin != ConditionalOrigin::Ternary
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
        {
            return Ok(None);
        }
        let Some(true_register) =
            leaf_name(when_true).and_then(|name| self.lookup_general(name))
        else {
            return Ok(None);
        };
        if leaf_name(when_false)
            .and_then(|name| self.lookup_general(name))
            .is_none()
        {
            return Ok(None);
        }
        self.emit_legacy_phi_merge(
            condition,
            when_true,
            when_false,
            true_register,
            true,
        )?;
        Ok(Some(true_register))
    }

    pub(crate) fn try_emit_legacy_phi_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
        origin: ConditionalOrigin,
    ) -> Compilation<bool> {
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || !tail
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
            || origin == ConditionalOrigin::IfReturns
        {
            return Ok(false);
        }

        let true_register = leaf_name(when_true).and_then(|name| self.lookup_general(name));
        let false_register = leaf_name(when_false).and_then(|name| self.lookup_general(name));
        let simple = |arm: &Expression| leaf_name(arm).is_some() || constant_value(arm).is_some();
        if !simple(when_true) || !simple(when_false) {
            return Ok(false);
        }
        let Some(phi) = true_register.or(false_register) else {
            return Ok(false);
        };
        // A true arm already in the ABI result register takes mwcc's compact
        // conditional-return form (max/min/clamp). The same applies when the
        // only leaf is a false arm already in the result register.
        if true_register == Some(destination)
            || (true_register.is_none() && false_register == Some(destination))
        {
            return Ok(false);
        }
        let false_leaf_reads_condition =
            leaf_name(when_false).is_some_and(|name| expression_reads_name(condition, name));
        let signed_zero_relational = match condition {
            Expression::Binary {
                operator,
                left,
                right,
            } if is_zero_literal(right)
                && matches!(
                    operator,
                    BinaryOperator::Less
                        | BinaryOperator::Greater
                        | BinaryOperator::LessEqual
                        | BinaryOperator::GreaterEqual
                ) => self.signedness_of(left)?,
            _ => false,
        };
        if true_register.is_none()
            && !memory_test_condition(condition)
            && !false_leaf_reads_condition
            // Build 163's complemented sign select overwrites the false leaf
            // with zero on the true path, then moves that shared register to r3.
            && !(is_zero_literal(when_true) && signed_zero_relational)
        {
            return Ok(false);
        }

        self.emit_legacy_phi_merge(
            condition,
            when_true,
            when_false,
            phi,
            true_register.is_some(),
        )?;
        if destination != phi {
            self.output
                .instructions
                .push(Instruction::move_register(destination, phi));
        }
        Ok(true)
    }

    fn emit_legacy_phi_merge(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        phi: u8,
        true_is_phi: bool,
    ) -> Compilation<()> {
        self.output.anonymous_label_bump += 3;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if true_is_phi {
            // Keep the true arm in its source register. The true path jumps over
            // the false-arm move/materialization; the false path replaces it.
            let false_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            let join_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            let false_arm = self.output.instructions.len();
            self.patch_forward(false_branch, false_arm);
            self.place_legacy_phi_value(when_false, phi)?;
            let join = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
                *target = join;
            }
        } else {
            // The false arm already occupies `phi`: a false condition branches
            // directly to the join, while the true path overwrites that register.
            let false_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });
            self.place_legacy_phi_value(when_true, phi)?;
            let join = self.output.instructions.len();
            self.patch_forward(false_branch, join);
        }
        Ok(())
    }

    fn place_legacy_phi_value(
        &mut self,
        value: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        if self.is_single_op_register_value(value) {
            self.evaluate_general(value, destination)
        } else {
            self.place_select_value(value, destination)
        }
    }
}

fn memory_test_condition(condition: &Expression) -> bool {
    if memory_value(condition) {
        return true;
    }
    matches!(condition,
        Expression::Binary { operator, left, right }
            if is_comparison(*operator) && (memory_value(left) || memory_value(right)))
}

fn memory_value(expression: &Expression) -> bool {
    match expression {
        Expression::Dereference { .. } | Expression::Member { .. } | Expression::Index { .. } => {
            true
        }
        Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        } => memory_value(operand),
        _ => false,
    }
}
