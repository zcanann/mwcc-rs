//! Build-163 merge lowering for integer selects tested by a memory load.

use super::*;

impl Generator {
    pub(crate) fn try_emit_legacy_memory_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<bool> {
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || !tail
            || !memory_test_condition(condition)
            || self.is_float_value(when_true)
            || self.is_float_value(when_false)
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

        self.output.anonymous_label_bump += 3;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if true_register.is_some() {
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
        if destination != phi {
            self.output
                .instructions
                .push(Instruction::move_register(destination, phi));
        }
        Ok(true)
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
