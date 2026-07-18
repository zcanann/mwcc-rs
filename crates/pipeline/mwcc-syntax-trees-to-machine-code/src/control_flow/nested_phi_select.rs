//! Build-163 shared-register lowering for right-nested integer selects.

use super::*;

impl Generator {
    /// Build 163 flattens a right-nested ternary chain into branches that all
    /// merge through one source register. Arms already occupying that phi home
    /// only branch to the shared join; other arms overwrite it on their path.
    pub(crate) fn try_emit_legacy_nested_phi_select(
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
            || !tail
            || origin != ConditionalOrigin::Ternary
            || !matches!(when_false, Expression::Conditional { .. })
        {
            return Ok(false);
        }

        let mut arms = vec![(condition, when_true)];
        let mut terminal = when_false;
        while let Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin: ConditionalOrigin::Ternary,
        } = terminal
        {
            arms.push((condition, when_true));
            terminal = when_false;
        }
        let simple = |arm: &Expression| leaf_name(arm).is_some() || constant_value(arm).is_some();
        if !simple(terminal)
            || arms.iter().any(|(_, arm)| !simple(arm))
            || self.is_float_value(terminal)
            || arms.iter().any(|(_, arm)| self.is_float_value(arm))
        {
            return Ok(false);
        }

        let phi = arms
            .iter()
            .find_map(|(_, arm)| leaf_name(arm).and_then(|name| self.lookup_general(name)))
            .or_else(|| leaf_name(terminal).and_then(|name| self.lookup_general(name)));
        let Some(phi) = phi else {
            return Ok(false);
        };
        if phi == destination {
            return Ok(false);
        }
        let arm_register = |generator: &Self, arm: &Expression| {
            leaf_name(arm).and_then(|name| generator.lookup_general(name))
        };
        if arms
            .iter()
            .any(|(_, arm)| leaf_name(arm).is_some() && arm_register(self, arm).is_none())
            || (leaf_name(terminal).is_some() && arm_register(self, terminal).is_none())
        {
            return Ok(false);
        }

        // Preserve the established anonymous-label accounting of the nested
        // select path: the terminal merge owns the ternary's three labels.
        self.output.anonymous_label_bump += 3;
        let terminal_is_phi = arm_register(self, terminal) == Some(phi);
        let mut join_branches = Vec::new();
        for (index, (condition, arm)) in arms.iter().enumerate() {
            let last = index + 1 == arms.len();
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let false_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options,
                    condition_bit,
                    target: 0,
                });

            if arm_register(self, arm) != Some(phi) {
                self.place_select_value(arm, phi)?;
            }
            if last && terminal_is_phi {
                let join = self.output.instructions.len();
                self.patch_forward(false_branch, join);
                continue;
            }

            let join_branch = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::Branch { target: 0 });
            join_branches.push(join_branch);
            let next = self.output.instructions.len();
            self.patch_forward(false_branch, next);
        }
        if !terminal_is_phi {
            self.place_select_value(terminal, phi)?;
        }
        let join = self.output.instructions.len();
        for branch in join_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = join;
            }
        }
        self.output
            .instructions
            .push(Instruction::move_register(destination, phi));
        Ok(true)
    }
}
