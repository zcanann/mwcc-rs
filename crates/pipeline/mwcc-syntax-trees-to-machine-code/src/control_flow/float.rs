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
    pub(crate) fn place_float_comparison_operands(
        &mut self,
        left: &Expression,
        right: &Expression,
        reserved: &[u8],
    ) -> Compilation<(u8, u8)> {
        let left_register = if self.is_float_located(left) {
            let newly: Vec<u8> = reserved
                .iter()
                .copied()
                .filter(|register| self.reserved.insert(*register))
                .collect();
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
        // A constant right operand loads into the scratch at the comparison's WIDTH: a DOUBLE compare
        // pools an 8-byte constant (`lfd`), a single pools 4 bytes (`lfs`). An integer literal in a
        // float comparison (`a < 0`) promotes to that same float type. The width is taken from the
        // left operand (the variable side of a `var REL const` comparison).
        let constant_bits = match right {
            Expression::FloatLiteral(value) => Some(*value),
            Expression::IntegerLiteral(value) => Some(*value as f64),
            _ => None,
        };
        let right_register = if let Some(value) = constant_bits {
            if self.is_double_value(left) {
                self.load_double_constant(FLOAT_SCRATCH, value.to_bits());
            } else {
                self.load_float_constant(FLOAT_SCRATCH, value as f32);
            }
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
        let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        else {
            return Err(Diagnostic::error(
                "float conditional needs a comparison condition",
            ));
        };
        if !is_comparison(*operator) {
            return Err(Diagnostic::error(
                "float conditional needs a comparison condition",
            ));
        }
        // A float conditional branch advances mwcc's anonymous-`@N` counter by 3.
        self.output.has_float_branch = true;
        // Each arm is a float leaf (its value in a register) or the NEGATION of a leaf (the fabs
        // family `cond ? -x : x`: the base is in the register, the arm value is `fneg base`). The
        // negated arm becomes an `fneg` tail; a plain leaf becomes the branch-returned value or `fmr`.
        let (true_register, true_negate) = self.float_select_arm(when_true)?;
        let (false_register, false_negate) = self.float_select_arm(when_false)?;
        // The condition operands may be memory loads: a located left operand loads
        // into a free register (avoiding the select values), the right into the
        // scratch; leaf operands stay in place.
        let (left_register, right_register) =
            self.place_float_comparison_operands(left, right, &[true_register, false_register])?;

        // Equality (`==`/`!=`) uses the QUIET compare `fcmpu` (IEEE equality does not signal on NaN);
        // the relational operators (`<`/`>`/`<=`/`>=`) use the signaling ordered compare `fcmpo`.
        if matches!(operator, BinaryOperator::Equal | BinaryOperator::NotEqual) {
            self.output
                .instructions
                .push(Instruction::FloatCompareUnordered {
                    a: left_register,
                    b: right_register,
                });
        } else {
            self.output
                .instructions
                .push(Instruction::FloatCompareOrdered {
                    a: left_register,
                    b: right_register,
                });
        }
        // `<=` / `>=` on FLOATS must be FALSE for unordered (NaN) operands. A direct `ble`/`bge`
        // (branch-if-not-gt / not-lt) would also take the branch when unordered, so mwcc instead OoRs
        // the strict bit into the eq bit (`cror eq, lt|gt, eq`) and branches on eq. Integer `<=`/`>=`
        // keep the direct branch (no unordered case) and never reach this float path.
        let (positive_options, condition_bit) = match operator {
            BinaryOperator::LessEqual | BinaryOperator::GreaterEqual => {
                let strict_bit = if *operator == BinaryOperator::LessEqual {
                    0
                } else {
                    1
                }; // lt / gt
                self.output
                    .instructions
                    .push(Instruction::ConditionRegisterOr {
                        d: 2,
                        a: strict_bit,
                        b: 2,
                    });
                (12, 2) // branch-if-eq
            }
            _ => positive_branch(*operator),
        };

        // The arm returned via the branch must be a plain leaf already in the result register; the
        // OTHER arm becomes the fall-through tail (`fmr` for a leaf, `fneg` for a negated one).
        if tail && !true_negate && true_register == destination {
            // true value already in the result: return on the true branch.
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: positive_options,
                    condition_bit,
                });
            self.emit_float_select_tail(destination, false_register, false_negate);
            return Ok(());
        }
        if self.behavior.integer_select_style
            == mwcc_versions::IntegerSelectStyle::BranchPreserving
            && tail
            && !true_negate
            && !false_negate
            && false_register == destination
            && true_register != destination
        {
            // Build 163 keeps the true arm's register as the phi instead of
            // returning early from the false arm. The true path skips the copy;
            // the false path overwrites phi, followed by one result move.
            let false_arm = self.fresh_label();
            let join = self.fresh_label();
            self.emit_branch_conditional_to(
                positive_options ^ 8,
                condition_bit,
                false_arm,
            );
            self.emit_branch_to(join);
            self.bind_label(false_arm);
            self.output.instructions.push(Instruction::FloatMove {
                d: true_register,
                b: false_register,
            });
            self.bind_label(join);
            self.output.instructions.push(Instruction::FloatMove {
                d: destination,
                b: true_register,
            });
            return Ok(());
        }
        if tail && !false_negate && false_register == destination {
            self.output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister {
                    options: positive_options ^ 8,
                    condition_bit,
                });
            self.emit_float_select_tail(destination, true_register, true_negate);
            return Ok(());
        }
        if !tail {
            let false_arm = self.fresh_label();
            let join = self.fresh_label();
            self.emit_branch_conditional_to(
                positive_options ^ 8,
                condition_bit,
                false_arm,
            );
            self.emit_float_select_tail(destination, true_register, true_negate);
            self.emit_branch_to(join);
            self.bind_label(false_arm);
            self.emit_float_select_tail(destination, false_register, false_negate);
            self.bind_label(join);
            return Ok(());
        }
        Err(Diagnostic::error(
            "tail float select has no result-register arm",
        ))
    }

    /// Classify a float select arm: a plain leaf (`(register, false)`) or the negation of a leaf
    /// (`(base_register, true)` — the fabs family `cond ? -x : x`).
    fn float_select_arm(&self, arm: &Expression) -> Compilation<(u8, bool)> {
        if let Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } = arm
        {
            Ok((self.float_register_of_leaf(operand)?, true))
        } else {
            Ok((self.float_register_of_leaf(arm)?, false))
        }
    }

    /// Emit the fall-through arm of a tail float select: `fneg` a negated arm, else `fmr` a leaf that
    /// is not already in the destination.
    fn emit_float_select_tail(&mut self, destination: u8, register: u8, negate: bool) {
        if negate {
            self.output.instructions.push(Instruction::FloatNegate {
                d: destination,
                b: register,
            });
        } else if destination != register {
            self.output.instructions.push(Instruction::FloatMove {
                d: destination,
                b: register,
            });
        }
    }
}
