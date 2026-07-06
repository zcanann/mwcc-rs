//! Float comparison operand placement and float conditional emission.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit a float `condition ? when_true : when_false`. The condition must be a
    /// float comparison; in tail position, when one branch value already sits in
    /// the result register, return early on that branch (fcmpo + bclr).
    /// Place the two operands of a float comparison. A leaf variable stays in its
    /// register; a memory-loaded left operand loads into a free register (avoiding
    /// the `reserved` select-value registers), the right into the scratch; a float
    /// constant loads into the scratch.
    pub(crate) fn place_float_comparison_operands(&mut self, left: &Expression, right: &Expression, reserved: &[u8]) -> Compilation<(u8, u8)> {
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

        // Equality (`==`/`!=`) uses the QUIET compare `fcmpu` (IEEE equality does not signal on NaN);
        // the relational operators (`<`/`>`/`<=`/`>=`) use the signaling ordered compare `fcmpo`.
        if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            self.output.instructions.push(Instruction::FloatCompareUnordered { a: left_register, b: right_register });
        } else {
            self.output.instructions.push(Instruction::FloatCompareOrdered { a: left_register, b: right_register });
        }
        // `<=` / `>=` on FLOATS must be FALSE for unordered (NaN) operands. A direct `ble`/`bge`
        // (branch-if-not-gt / not-lt) would also take the branch when unordered, so mwcc instead OoRs
        // the strict bit into the eq bit (`cror eq, lt|gt, eq`) and branches on eq. Integer `<=`/`>=`
        // keep the direct branch (no unordered case) and never reach this float path.
        let (positive_options, condition_bit) = match operator {
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => {
                let strict_bit = if *operator == BinaryOperator::LessEqual { 0 } else { 1 }; // lt / gt
                self.output.instructions.push(Instruction::ConditionRegisterOr { d: 2, a: strict_bit, b: 2 });
                (12, 2) // branch-if-eq
            }
            _ => positive_branch(*operator),
        };

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

}
