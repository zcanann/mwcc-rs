//! Generation-specific lowering of signed absolute-value selects.

use super::*;
use mwcc_versions::IntegerSelectStyle;

impl Generator {
    pub(crate) fn try_emit_absolute_value_select(
        &mut self,
        condition: &Expression,
        when_true: &Expression,
        when_false: &Expression,
        destination: u8,
        tail: bool,
    ) -> Compilation<bool> {
        let Some((value, value_when_true)) =
            absolute_value_target(condition, when_true, when_false)
        else {
            return Ok(false);
        };
        if !self.signedness_of(value)? {
            return Ok(false);
        }
        let source = self.general_register_of_leaf(value)?;

        match self.behavior.integer_select_style {
            IntegerSelectStyle::Branchless => {
                let mask = self.fresh_virtual_general();
                self.output
                    .instructions
                    .push(Instruction::ShiftRightAlgebraicImmediate {
                        a: mask,
                        s: source,
                        shift: 31,
                    });
                self.output.instructions.push(Instruction::Xor {
                    a: GENERAL_SCRATCH,
                    s: mask,
                    b: source,
                });
                self.output.instructions.push(Instruction::SubtractFrom {
                    d: destination,
                    a: mask,
                    b: GENERAL_SCRATCH,
                });
            }
            IntegerSelectStyle::BranchPreserving => {
                // The legacy form mutates the incoming value in place. A future
                // non-coalesced destination needs its own measured copy schedule.
                if source != destination {
                    return Ok(false);
                }
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                if tail {
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalToLinkRegister {
                            options: if value_when_true {
                                options ^ 8
                            } else {
                                options
                            },
                            condition_bit,
                        });
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: source,
                    });
                } else if value_when_true {
                    let negate_branch = self.output.instructions.len();
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
                    let negate = self.output.instructions.len();
                    self.patch_forward(negate_branch, negate);
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: source,
                    });
                    let join = self.output.instructions.len();
                    if let Instruction::Branch { target } =
                        &mut self.output.instructions[join_branch]
                    {
                        *target = join;
                    }
                } else {
                    let join_branch = self.output.instructions.len();
                    self.output
                        .instructions
                        .push(Instruction::BranchConditionalForward {
                            options,
                            condition_bit,
                            target: 0,
                        });
                    self.output.instructions.push(Instruction::Negate {
                        d: destination,
                        a: source,
                    });
                    let join = self.output.instructions.len();
                    self.patch_forward(join_branch, join);
                }
            }
        }
        Ok(true)
    }
}

/// The shared signed leaf and whether the non-negated value is the true arm.
pub(super) fn absolute_value_target<'e>(
    condition: &'e Expression,
    when_true: &'e Expression,
    when_false: &'e Expression,
) -> Option<(&'e Expression, bool)> {
    match condition {
        Expression::Binary {
            operator: BinaryOperator::Less | BinaryOperator::LessEqual,
            left,
            right,
        } if is_zero_literal(right) => match when_true {
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } if leaf_name(left).is_some()
                && leaf_name(left) == leaf_name(operand)
                && leaf_name(left) == leaf_name(when_false) =>
            {
                Some((left, false))
            }
            _ => None,
        },
        Expression::Binary {
            operator: BinaryOperator::Greater | BinaryOperator::GreaterEqual,
            left,
            right,
        } if is_zero_literal(right) => match when_false {
            Expression::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } if leaf_name(left).is_some()
                && leaf_name(left) == leaf_name(operand)
                && leaf_name(left) == leaf_name(when_true) =>
            {
                Some((left, true))
            }
            _ => None,
        },
        _ => None,
    }
}
