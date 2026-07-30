//! Store selects that reuse the value already computed for their comparison.
//!
//! Build 163 retains the compared arithmetic value across an upper/lower-bound
//! ternary instead of evaluating the selected value again. Small bounds can
//! branch around an in-place constant materialization. Bounds that require a
//! register use that register as the select's phi.

use super::*;

struct ComparedValueStoreSelect<'a> {
    operator: BinaryOperator,
    value: &'a Expression,
    bound: &'a Expression,
    constant: i64,
}

fn recognize_compared_value_store_select<'a>(
    condition: &'a Expression,
    when_true: &'a Expression,
    when_false: &'a Expression,
    origin: ConditionalOrigin,
) -> Option<ComparedValueStoreSelect<'a>> {
    if origin != ConditionalOrigin::Ternary {
        return None;
    }
    let Expression::Binary {
        operator,
        left,
        right,
    } = condition
    else {
        return None;
    };
    if !matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual
    ) {
        return None;
    }
    let constant = constant_value(right)?;
    if constant_value(when_true) != Some(constant)
        || !structurally_equal(left, when_false)
        || expression_has_side_effect(left)
        || !is_simple_arithmetic_arm(left)
        || !matches!(
            left.as_ref(),
            Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } if as_member(left).is_some() && as_member(right).is_some()
        )
    {
        return None;
    }

    Some(ComparedValueStoreSelect {
        operator: *operator,
        value: left,
        bound: right,
        constant,
    })
}

impl Generator {
    /// Emit `(value REL bound) ? bound : value` when used directly by an
    /// integer store, returning the register that owns the selected value.
    pub(crate) fn try_emit_compared_value_store_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        origin: ConditionalOrigin,
    ) -> Compilation<Option<u8>> {
        if self.behavior.integer_select_style != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || !self.non_leaf
        {
            return Ok(None);
        }
        let Some(plan) =
            recognize_compared_value_store_select(condition, when_true, when_false, origin)
        else {
            return Ok(None);
        };
        if self.is_float_value(plan.value) || self.is_float_value(plan.bound) {
            return Ok(None);
        }

        let signed = self.usual_integer_binary_signedness(plan.value, plan.bound)?;
        let immediate = if signed {
            i16::try_from(plan.constant)
                .ok()
                .map(|value| EitherImmediate::Signed(value))
        } else {
            u16::try_from(plan.constant)
                .ok()
                .map(|value| EitherImmediate::Unsigned(value))
        };
        let (options, condition_bit) =
            false_branch_bo_bi(plan.operator).expect("recognizer accepts only comparisons");

        if let Some(immediate) = immediate {
            self.evaluate_general(plan.value, GENERAL_SCRATCH)?;
            self.output.instructions.push(match immediate {
                EitherImmediate::Signed(immediate) => Instruction::CompareWordImmediate {
                    a: GENERAL_SCRATCH,
                    immediate,
                },
                EitherImmediate::Unsigned(immediate) => Instruction::CompareLogicalWordImmediate {
                    a: GENERAL_SCRATCH,
                    immediate,
                },
            });
            let join = self.fresh_label();
            self.emit_branch_conditional_to(options, condition_bit, join);
            self.load_integer_constant(GENERAL_SCRATCH, plan.constant);
            self.bind_label(join);
            return Ok(Some(GENERAL_SCRATCH));
        }

        if !(0..=u32::MAX as i64).contains(&plan.constant) || plan.constant as u32 & 0xffff != 0 {
            return Ok(None);
        }

        let phi =
            self.fresh_virtual_general_preferring(mwcc_target::Eabi::FIRST_GENERAL_ARGUMENT + 1);
        let value_start = self.output.instructions.len();
        self.evaluate_general(plan.value, GENERAL_SCRATCH)?;
        let constant = self.output.instructions.len();
        self.load_integer_constant(phi, plan.constant);
        debug_assert_eq!(self.output.instructions.len(), constant + 1);
        // The independent high-half materialization fills the latency between
        // the two member loads.
        crate::move_instruction_before_retargeting(self, constant, value_start + 1);
        self.output.instructions.push(if signed {
            Instruction::CompareWord {
                a: GENERAL_SCRATCH,
                b: phi,
            }
        } else {
            Instruction::CompareLogicalWord {
                a: GENERAL_SCRATCH,
                b: phi,
            }
        });
        let false_arm = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, false_arm);
        self.emit_branch_to(join);
        self.bind_label(false_arm);
        self.output
            .instructions
            .push(Instruction::move_register(phi, GENERAL_SCRATCH));
        self.bind_label(join);
        Ok(Some(phi))
    }
}

enum EitherImmediate {
    Signed(i16),
    Unsigned(u16),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn difference() -> Expression {
        let member = |offset| Expression::Member {
            base: Box::new(Expression::Variable("transfer".into())),
            offset,
            member_type: mwcc_syntax_trees::Type::UnsignedInt,
            index_stride: None,
        };
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(member(0)),
            right: Box::new(member(4)),
        }
    }

    #[test]
    fn recognizes_a_bound_selected_over_the_compared_arithmetic_value() {
        let value = difference();
        let bound = Expression::IntegerLiteral(0x80000);
        let condition = Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(value.clone()),
            right: Box::new(bound.clone()),
        };

        let plan = recognize_compared_value_store_select(
            &condition,
            &bound,
            &value,
            ConditionalOrigin::Ternary,
        )
        .expect("upper-bound store select");
        assert_eq!(plan.operator, BinaryOperator::Greater);
        assert_eq!(plan.constant, 0x80000);
        assert!(structurally_equal(plan.value, &value));
    }

    #[test]
    fn rejects_a_false_arm_that_is_not_the_compared_value() {
        let value = difference();
        let bound = Expression::IntegerLiteral(1000);
        let condition = Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(value),
            right: Box::new(bound.clone()),
        };

        assert!(recognize_compared_value_store_select(
            &condition,
            &bound,
            &Expression::Variable("other".into()),
            ConditionalOrigin::Ternary,
        )
        .is_none());
    }
}
