//! Build-163 early returns over normalized, value-tracked tails.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower a single normalized guard for build 163. The branch-preserving
    /// pipeline retains a real source diamond for a plain return or a tracked
    /// one-input tail. If the tail consumes r3 or two-plus parameters, it is
    /// issued directly into r3 before a conditional return so the guarded value
    /// can be materialized afterward without a merge register.
    pub(crate) fn try_legacy_tracked_guard_return(
        &mut self,
        function: &Function,
        tail: &Expression,
        result: u8,
    ) -> Compilation<bool> {
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [guard] = function.guards.as_slice() else {
            return Ok(false);
        };
        if constant_value(&guard.value).is_none()
            && !matches!(&guard.value, Expression::Variable(_))
        {
            return Ok(false);
        }
        let simple_tail = matches!(tail, Expression::Binary { left, right, .. }
            if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
        if !simple_tail {
            return Ok(false);
        }

        let tail_reads_result = self.locations.iter().any(|(name, location)| {
            location.class == ValueClass::General
                && location.register == result
                && expression_reads_name(tail, name)
        });
        let distinct_parameter_reads = function
            .parameters
            .iter()
            .filter(|parameter| expression_reads_name(tail, &parameter.name))
            .count();
        let tracked_tail = !function.statements.is_empty() || !function.locals.is_empty();
        let direct_tail = tracked_tail && (tail_reads_result || distinct_parameter_reads >= 2);

        // The direct-tail form cannot preserve a guarded variable already in r3.
        // Leave that merge-register shape to the existing conservative path.
        if direct_tail
            && matches!(&guard.value, Expression::Variable(name)
                if self.lookup_general(name) == Some(result))
        {
            return Ok(false);
        }

        let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
        if direct_tail {
            self.evaluate_tail(tail, function.return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options,
                    condition_bit,
                });
            self.evaluate_tail(&guard.value, function.return_type, result)?;
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }

        let continuation_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options,
                condition_bit,
                target: 0,
            });
        self.evaluate_tail(&guard.value, function.return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        let continuation = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[continuation_branch]
        {
            *target = continuation;
        }
        self.evaluate_tail(tail, function.return_type, result)?;
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
