//! Control-flow layout for null-guarded pointer accesses.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit the non-speculative diamond for a pointer access protected by a
    /// null test. Mainline normalizes both source spellings to a hot access
    /// fall-through. Build 163 preserves a leading `!p`, leaving its cold
    /// constant arm first and branching to the hot access.
    pub(crate) fn emit_guarded_null_access(
        &mut self,
        condition: &Expression,
        pointer: &str,
        hot: &Expression,
        cold: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<bool> {
        let Some(pointer_register) = self.lookup_general(pointer) else {
            return Ok(false);
        };
        self.output
            .instructions
            .push(Instruction::CompareLogicalWordImmediate {
                a: pointer_register,
                immediate: 0,
            });

        let preserves_negated_source = self.behavior.integer_select_style
            == mwcc_versions::IntegerSelectStyle::BranchPreserving
            && matches!(
                condition,
                Expression::Unary {
                    operator: UnaryOperator::LogicalNot,
                    ..
                }
            );
        let (fallthrough, branch_arm, options) = if preserves_negated_source {
            (cold, hot, 4) // bne HOT
        } else {
            (hot, cold, 12) // beq COLD
        };

        let branch_index = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit: 2,
                target: 0,
            });
        self.evaluate_tail(fallthrough, return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let branch_arm_label = self.output.instructions.len();
        self.patch_forward(branch_index, branch_arm_label);
        self.evaluate_tail(branch_arm, return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
