//! Build-163 shared-register lowering for simple integer selects.

use super::*;

impl Generator {
    /// Build 163 keeps a tail select between zero and one single-op value as
    /// two return paths instead of forming the mainline mask-and-combine idiom.
    pub(crate) fn try_emit_legacy_computed_zero_tail(
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
        let one_zero_one_computed = (is_zero_literal(when_true)
            && self.is_single_op_register_value(when_false))
            || (self.is_single_op_register_value(when_true) && is_zero_literal(when_false));
        if !one_zero_one_computed {
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
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let false_arm = self.output.instructions.len();
        self.patch_forward(false_branch, false_arm);
        self.evaluate_general(when_false, destination)?;
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
        if true_register.is_none()
            && !memory_test_condition(condition)
            && !false_leaf_reads_condition
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
            self.place_select_value(when_false, phi)?;
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
            self.place_select_value(when_true, phi)?;
            let join = self.output.instructions.len();
            self.patch_forward(false_branch, join);
        }
        Ok(())
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
