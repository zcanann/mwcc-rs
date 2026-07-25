//! Build-163 shared-register lowering for simple integer selects.

use super::*;

impl Generator {
    /// Build 163 keeps a positive guarded return and its masked fallback as a
    /// short-circuit diamond even when the value is an inlined initializer:
    /// every failed condition enters the fallback, while the completed guard
    /// copies the live source into the destination and jumps to the join.
    pub(crate) fn try_emit_legacy_guarded_mask_fallback_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
        origin: ConditionalOrigin,
    ) -> Compilation<bool> {
        if tail
            || destination == GENERAL_SCRATCH
            || origin != ConditionalOrigin::IfReturns
        {
            return Ok(false);
        }
        let Expression::Variable(true_name) = when_true else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitOr,
            left: masked,
            right: high,
        } = when_false
        else {
            return Ok(false);
        };
        let Expression::Binary {
            operator: BinaryOperator::BitAnd,
            left: masked_source,
            right: mask,
        } = masked.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(masked_source.as_ref(), Expression::Variable(name) if name == true_name)
            || constant_value(mask).is_none()
            || constant_value(high).is_none()
        {
            return Ok(false);
        }
        let Some(true_register) = self.lookup_general(true_name) else {
            return Ok(false);
        };

        fn collect<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
            if let Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left,
                right,
            } = expression
            {
                collect(left, terms);
                collect(right, terms);
            } else {
                terms.push(expression);
            }
        }
        let mut terms = Vec::new();
        collect(condition, &mut terms);
        if terms.len() < 2 {
            return Ok(false);
        }

        let previous_cache = self.begin_condition_global_cache(condition);
        let previous_float_cache = self.begin_condition_float_cache(condition);
        let emitted = (|| {
            self.preload_condition_global_cache(condition)?;
            let mut fallback_branches = Vec::with_capacity(terms.len());
            for term in terms {
                let (options, condition_bit) = self.emit_condition_test(term)?;
                fallback_branches.push(self.output.instructions.len());
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
            }
            self.output
                .instructions
                .push(Instruction::move_register(destination, true_register));
            let join_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            let fallback = self.output.instructions.len();
            for branch in fallback_branches {
                self.patch_forward(branch, fallback);
            }
            self.evaluate_general(when_false, destination)?;
            let join = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
                *target = join;
            }
            Ok(())
        })();
        self.restore_condition_global_cache(previous_cache);
        self.restore_condition_float_cache(previous_float_cache);
        emitted?;
        Ok(true)
    }

    /// Build 163 materializes a simple ternary into the ABI result register
    /// when a surrounding call has forced the operands into callee-saved
    /// registers.  The leaf implementation below can instead use an operand
    /// register as a phi, but that would lengthen its live range across the
    /// epilogue and is not the framed schedule mwcc chooses.
    pub(crate) fn try_emit_legacy_framed_simple_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
        origin: ConditionalOrigin,
    ) -> Compilation<bool> {
        let simple = |arm: &Expression| leaf_name(arm).is_some() || constant_value(arm).is_some();
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || !self.non_leaf
            || !tail
            || origin != ConditionalOrigin::Ternary
            || !simple(when_true)
            || !simple(when_false)
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
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
        self.place_select_value(when_true, destination)?;
        let join_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let false_arm = self.output.instructions.len();
        self.patch_forward(false_branch, false_arm);
        self.place_select_value(when_false, destination)?;
        let join = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[join_branch] {
            *target = join;
        }
        Ok(true)
    }

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
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || self.non_leaf
            || !tail
            || origin == ConditionalOrigin::IfAssignments
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
        {
            return Ok(false);
        }
        if super::absolute_value::absolute_value_target(condition, when_true, when_false).is_some()
        {
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
        let computed_arm = if true_computed { when_true } else { when_false };
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
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
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
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || self.non_leaf
            || origin != ConditionalOrigin::Ternary
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
        {
            return Ok(None);
        }
        let Some(true_register) = leaf_name(when_true).and_then(|name| self.lookup_general(name))
        else {
            return Ok(None);
        };
        if leaf_name(when_false)
            .and_then(|name| self.lookup_general(name))
            .is_none()
        {
            return Ok(None);
        }
        self.emit_legacy_phi_merge(condition, when_true, when_false, true_register, true)?;
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
                ) =>
            {
                self.signedness_of(left)?
            }
            _ => false,
        };
        // An if/else assignment has a named merge variable even when only its
        // false source is a leaf. Build 163 overwrites that leaf's register on
        // the true path, then moves the shared value to the return register.
        if true_register.is_none()
            && origin != ConditionalOrigin::IfAssignments
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

    fn place_legacy_phi_value(&mut self, value: &Expression, destination: u8) -> Compilation<()> {
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
