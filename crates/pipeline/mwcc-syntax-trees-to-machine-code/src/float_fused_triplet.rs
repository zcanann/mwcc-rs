//! Contracted multiply-add selection and operand scheduling.

use crate::analysis::as_multiplication;
use crate::generator::{Generator, FLOAT_SCRATCH};
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

impl Generator {
    /// Try to fuse `left op right` into a multiply-add when one side is a
    /// multiplication.
    ///
    /// The generic form only owns products whose factors are already resident
    /// in FPRs. Memory-backed factors use the measured three-load specialization
    /// below when its addend is also a direct load; other memory product trees
    /// remain ordinary multiply/add trees. In particular, MWCC does not contract
    /// the common squared-length shape `x*x + z*z`.
    pub(crate) fn try_emit_float_fused(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if !self.behavior.contract_floating_point {
            return Ok(false);
        }
        if self.try_emit_located_fused_triplet(operator, left, right, destination, double)? {
            return Ok(true);
        }
        if let Some((x, y)) = register_product(left, |factor| self.is_float_leaf(factor)) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(right)?;
            self.output.instructions.push(match (operator, double) {
                (BinaryOperator::Add, false) => Instruction::FloatMultiplyAddSingle {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, false) => Instruction::FloatMultiplySubtractSingle {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Add, true) => Instruction::FloatMultiplyAddDouble {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, true) => Instruction::FloatMultiplySubtractDouble {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        if let Some((x, y)) = register_product(right, |factor| self.is_float_leaf(factor)) {
            let multiplicand = self.float_register_of_leaf(x)?;
            let multiplier = self.float_register_of_leaf(y)?;
            let addend = self.place_float_addend(left)?;
            self.output.instructions.push(match (operator, double) {
                (BinaryOperator::Add, false) => Instruction::FloatMultiplyAddSingle {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, false) => {
                    Instruction::FloatNegativeMultiplySubtractSingle {
                        d: destination,
                        a: multiplicand,
                        c: multiplier,
                        b: addend,
                    }
                }
                (BinaryOperator::Add, true) => Instruction::FloatMultiplyAddDouble {
                    d: destination,
                    a: multiplicand,
                    c: multiplier,
                    b: addend,
                },
                (BinaryOperator::Subtract, true) => {
                    Instruction::FloatNegativeMultiplySubtractDouble {
                        d: destination,
                        a: multiplicand,
                        c: multiplier,
                        b: addend,
                    }
                }
                _ => unreachable!("caller restricts to add/subtract"),
            });
            return Ok(true);
        }
        Ok(false)
    }

    /// Emit the measured `addend + x * y` memory triplet.
    ///
    /// MWCC fills the independent load lanes in f2, f1, then f0 and contracts
    /// directly into f0. Virtual preferences retain that placement while still
    /// letting the allocator avoid a genuinely live FPR in broader contexts.
    pub(crate) fn try_emit_located_fused_triplet(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if double || operator != BinaryOperator::Add {
            return Ok(false);
        }
        let (addend, x, y) = match right {
            Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: x,
                right: y,
            } => (left, x.as_ref(), y.as_ref()),
            _ => match left {
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    left: x,
                    right: y,
                } => (right, x.as_ref(), y.as_ref()),
                _ => return Ok(false),
            },
        };
        if !self.is_float_located(addend) || !self.is_float_located(x) || !self.is_float_located(y)
        {
            return Ok(false);
        }

        let (multiplicand, multiplier, addend_register) = if destination == FLOAT_SCRATCH {
            (
                self.fresh_virtual_float_preferring(2),
                self.fresh_virtual_float_preferring(1),
                destination,
            )
        } else {
            // When this triplet is the left child of a larger expression, keep
            // its result in the requested home and load the second factor there.
            // The first factor gets a separate live lane while f0 remains free
            // for both addends and the sibling subtree.
            if destination >= mwcc_vreg::VIRTUAL_BASE {
                self.register_prefer
                    .insert(u32::from(destination - mwcc_vreg::VIRTUAL_BASE), 3);
            }
            (
                self.fresh_virtual_float_preferring(4),
                destination,
                FLOAT_SCRATCH,
            )
        };
        self.emit_located_operand(x, multiplicand)?;
        self.emit_located_operand(y, multiplier)?;
        self.emit_located_operand(addend, addend_register)?;
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: destination,
                a: multiplicand,
                c: multiplier,
                b: addend_register,
            });
        Ok(true)
    }
}

fn register_product(
    expression: &Expression,
    is_register_leaf: impl Fn(&Expression) -> bool,
) -> Option<(&Expression, &Expression)> {
    let (left, right) = as_multiplication(expression)?;
    (is_register_leaf(left) && is_register_leaf(right)).then_some((left, right))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    fn product(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn generic_fusion_only_claims_register_resident_products() {
        let variable = |name: &str| Expression::Variable(name.into());
        let member = |offset| Expression::Member {
            base: Box::new(variable("vector")),
            offset,
            member_type: Type::Float,
            index_stride: None,
        };
        let is_variable = |expression: &Expression| matches!(expression, Expression::Variable(_));

        assert!(register_product(&product(variable("x"), variable("y")), is_variable,).is_some());
        assert!(register_product(&product(member(0), member(8)), is_variable).is_none());
    }
}
