//! Contracted multiply-add selection and operand scheduling.

use crate::analysis::{as_multiplication, same_operand};
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
        if self.try_emit_adjacent_linear_interpolation(
            operator,
            left,
            right,
            destination,
            double,
        )? {
            return Ok(true);
        }
        if self.try_emit_promoted_integer_fused_triplet(
            operator,
            left,
            right,
            destination,
            double,
        )? {
            return Ok(true);
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

    /// Emit the canonical adjacent-sample interpolation
    /// `(1 - amount) * values[index] + amount * values[index + 1]`.
    ///
    /// Besides selecting the final fused multiply-add, MWCC computes the
    /// scaled index once and forms an adjacent address for the second load.
    /// Keeping that address transaction inside the expression selector avoids
    /// relying on a later peephole to rediscover source-level aliasing after
    /// the two independent subscript lowerings have already duplicated it.
    fn try_emit_adjacent_linear_interpolation(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if operator != BinaryOperator::Add {
            return Ok(false);
        }
        let Some(interpolation) = adjacent_linear_interpolation(left, right) else {
            return Ok(false);
        };
        let pointee = self.pointee_of(interpolation.base)?;
        if !matches!(pointee, mwcc_syntax_trees::Pointee::Float | mwcc_syntax_trees::Pointee::Double)
            || double != matches!(pointee, mwcc_syntax_trees::Pointee::Double)
        {
            return Ok(false);
        }

        let index = self.general_register_of_leaf(interpolation.index)?;
        let scaled = self.fresh_virtual_general_preferring(0);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: index,
                shift: pointee.size().trailing_zeros() as u8,
            });

        let complement = self.fresh_virtual_float_preferring(3);
        self.load_float_literal(complement, 1.0, double);

        let (resolved_pointee, base) = self.resolve_pointer(interpolation.base)?;
        debug_assert_eq!(resolved_pointee, pointee);
        // Prefer coalescing the now-dead source index with the address. If the
        // index remains live outside this expression, ordinary interference
        // moves the address to another GPR without changing the lowering.
        let adjacent = self.fresh_virtual_general_preferring(index);
        self.output.instructions.push(Instruction::Add {
            d: adjacent,
            a: base,
            b: scaled,
        });

        let current = self.fresh_virtual_float_preferring(2);
        let next = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
        self.output.instructions.push(crate::expressions::indexed_load(
            pointee, current, base, scaled,
        )?);
        self.output.instructions.push(crate::expressions::displacement_load(
            pointee,
            next,
            adjacent,
            pointee.size() as i16,
        )?);

        let amount = self.float_register_of_leaf(interpolation.amount)?;
        self.output.instructions.push(if double {
            Instruction::FloatSubtractDouble {
                d: complement,
                a: complement,
                b: amount,
            }
        } else {
            Instruction::FloatSubtractSingle {
                d: complement,
                a: complement,
                b: amount,
            }
        });
        self.output.instructions.push(if double {
            Instruction::FloatMultiplyDouble {
                d: next,
                a: amount,
                c: next,
            }
        } else {
            Instruction::FloatMultiplySingle {
                d: next,
                a: amount,
                c: next,
            }
        });
        self.output.instructions.push(if double {
            Instruction::FloatMultiplyAddDouble {
                d: destination,
                a: complement,
                c: current,
                b: next,
            }
        } else {
            Instruction::FloatMultiplyAddSingle {
                d: destination,
                a: complement,
                c: current,
                b: next,
            }
        });
        Ok(true)
    }

    /// Emit `(int * float_load) + float_load` as the contracted mixed-type
    /// triplet selected by MWCC. The integer is promoted through the magic-bias
    /// frame image into f2, while the multiplier and addend occupy f1 and f0.
    fn try_emit_promoted_integer_fused_triplet(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<bool> {
        if let Some((integer, multiplier, base, direction)) = promoted_integer_register_fusion(
            operator,
            left,
            right,
            |expression| {
                self.cast_operand_width(expression)
                    .is_none_or(|width| width >= 32)
                    && self.general_register_of_leaf(expression).is_ok()
            },
            |expression| self.is_float_leaf(expression),
        ) {
            let promoted = self.fresh_virtual_float_preferring(FLOAT_SCRATCH);
            // The lowest free non-result lane holds the bias: f3 when the
            // usual f1/f2 parameter pair is live, f1 when callee-saved values
            // occupy the high bank. Expressing that as a preference lets the
            // allocator derive both schedules from the same lowering.
            let bias = self.fresh_virtual_float_preferring(1);
            self.emit_int_to_float(integer, promoted, double, bias)?;
            let multiplier = self.float_register_of_leaf(multiplier)?;
            let base = self.float_register_of_leaf(base)?;
            self.output.instructions.push(match (direction, double) {
                (PromotedIntegerFusion::Add, false) => {
                    Instruction::FloatMultiplyAddSingle {
                        d: destination,
                        a: promoted,
                        c: multiplier,
                        b: base,
                    }
                }
                (PromotedIntegerFusion::ProductMinusBase, false) => {
                    Instruction::FloatMultiplySubtractSingle {
                        d: destination,
                        a: promoted,
                        c: multiplier,
                        b: base,
                    }
                }
                (PromotedIntegerFusion::BaseMinusProduct, false) => {
                    Instruction::FloatNegativeMultiplySubtractSingle {
                        d: destination,
                        a: promoted,
                        c: multiplier,
                        b: base,
                    }
                }
                (PromotedIntegerFusion::Add, true) => {
                    Instruction::FloatMultiplyAddDouble {
                        d: destination,
                        a: promoted,
                        c: multiplier,
                        b: base,
                    }
                }
                (PromotedIntegerFusion::ProductMinusBase, true) => {
                    Instruction::FloatMultiplySubtractDouble {
                        d: destination,
                        a: promoted,
                        c: multiplier,
                        b: base,
                    }
                }
                (PromotedIntegerFusion::BaseMinusProduct, true) => {
                    Instruction::FloatNegativeMultiplySubtractDouble {
                        d: destination,
                        a: promoted,
                        c: multiplier,
                        b: base,
                    }
                }
            });
            return Ok(true);
        }
        if double || operator != BinaryOperator::Add {
            return Ok(false);
        }
        let Some((integer, multiplier, addend)) = promoted_integer_triplet(
            left,
            right,
            |expression| {
                self.general_register_of_leaf(expression).is_ok()
                    || (self.non_leaf
                        && self.is_word_load(expression)
                        && !self.is_float_value(expression))
            },
            |expression| self.is_float_located(expression),
        ) else {
            return Ok(false);
        };

        if destination >= mwcc_vreg::VIRTUAL_BASE {
            self.register_prefer.insert(
                mwcc_vreg::VirtualRegister::new(
                    u32::from(destination - mwcc_vreg::VIRTUAL_BASE),
                    mwcc_vreg::Class::Float,
                ),
                0,
            );
        }
        let promoted = self.fresh_virtual_float_preferring(2);
        let multiplier_register = self.fresh_virtual_float_preferring(1);
        if self.general_register_of_leaf(integer).is_ok() {
            self.emit_int_to_float(integer, promoted, false, 3)?;
        } else {
            let integer_register = self.fresh_virtual_general_preferring(3);
            self.evaluate_general(integer, integer_register)?;
            let scratch = self.claim_int_to_float_scratch()?;
            let signed = self.signedness_of(integer)?;
            self.emit_int_to_float_body_at(
                integer_register,
                promoted,
                false,
                signed,
                3,
                crate::casts::IntToFloatSchedule::LeafValue,
                scratch,
            );
        }
        self.emit_located_operand(multiplier, multiplier_register)?;
        self.emit_located_operand(addend, destination)?;
        self.output
            .instructions
            .push(Instruction::FloatMultiplyAddSingle {
                d: destination,
                a: promoted,
                c: multiplier_register,
                b: destination,
            });
        Ok(true)
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
                self.register_prefer.insert(
                    mwcc_vreg::VirtualRegister::new(
                        u32::from(destination - mwcc_vreg::VIRTUAL_BASE),
                        mwcc_vreg::Class::Float,
                    ),
                    3,
                );
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PromotedIntegerFusion {
    Add,
    ProductMinusBase,
    BaseMinusProduct,
}

struct AdjacentLinearInterpolation<'a> {
    amount: &'a Expression,
    base: &'a Expression,
    index: &'a Expression,
}

fn adjacent_linear_interpolation<'a>(
    complement_product: &'a Expression,
    amount_product: &'a Expression,
) -> Option<AdjacentLinearInterpolation<'a>> {
    let (complement_left, complement_right) = as_multiplication(complement_product)?;
    let (amount, current) = if let Some(amount) = one_minus(complement_left) {
        (amount, complement_right)
    } else {
        (one_minus(complement_right)?, complement_left)
    };
    let Expression::Index {
        base: current_base,
        index: current_index,
    } = current
    else {
        return None;
    };

    let (amount_left, amount_right) = as_multiplication(amount_product)?;
    let next = if same_operand(amount_left, amount) {
        amount_right
    } else if same_operand(amount_right, amount) {
        amount_left
    } else {
        return None;
    };
    let Expression::Index {
        base: next_base,
        index: next_index,
    } = next
    else {
        return None;
    };
    if !same_operand(current_base, next_base) || !is_adjacent_index(current_index, next_index) {
        return None;
    }
    Some(AdjacentLinearInterpolation {
        amount,
        base: current_base,
        index: current_index,
    })
}

fn one_minus(expression: &Expression) -> Option<&Expression> {
    let Expression::Binary {
        operator: BinaryOperator::Subtract,
        left,
        right,
    } = expression
    else {
        return None;
    };
    is_float_one(left).then_some(right)
}

fn is_float_one(mut expression: &Expression) -> bool {
    while let Expression::Cast {
        target_type: mwcc_syntax_trees::Type::Float | mwcc_syntax_trees::Type::Double,
        operand,
    } = expression
    {
        expression = operand;
    }
    matches!(expression, Expression::FloatLiteral(value) if *value == 1.0)
}

fn is_adjacent_index(current: &Expression, next: &Expression) -> bool {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = next
    else {
        return false;
    };
    (same_operand(left, current) && matches!(right.as_ref(), Expression::IntegerLiteral(1)))
        || (same_operand(right, current)
            && matches!(left.as_ref(), Expression::IntegerLiteral(1)))
}

fn promoted_integer_register_fusion<'a>(
    operator: BinaryOperator,
    left: &'a Expression,
    right: &'a Expression,
    is_integer: impl Fn(&Expression) -> bool,
    is_float: impl Fn(&Expression) -> bool,
) -> Option<(
    &'a Expression,
    &'a Expression,
    &'a Expression,
    PromotedIntegerFusion,
)> {
    let (product, base, direction) = match operator {
        BinaryOperator::Add if as_multiplication(left).is_some() => {
            (left, right, PromotedIntegerFusion::Add)
        }
        BinaryOperator::Add if as_multiplication(right).is_some() => {
            (right, left, PromotedIntegerFusion::Add)
        }
        BinaryOperator::Subtract if as_multiplication(left).is_some() => {
            (left, right, PromotedIntegerFusion::ProductMinusBase)
        }
        BinaryOperator::Subtract if as_multiplication(right).is_some() => {
            (right, left, PromotedIntegerFusion::BaseMinusProduct)
        }
        _ => return None,
    };
    if !is_float(base) {
        return None;
    }
    let (first, second) = as_multiplication(product)?;
    match (is_integer(first), is_float(first), is_integer(second), is_float(second)) {
        (true, false, false, true) => Some((first, second, base, direction)),
        (false, true, true, false) => Some((second, first, base, direction)),
        _ => None,
    }
}

fn register_product(
    expression: &Expression,
    is_register_leaf: impl Fn(&Expression) -> bool,
) -> Option<(&Expression, &Expression)> {
    let (left, right) = as_multiplication(expression)?;
    (is_register_leaf(left) && is_register_leaf(right)).then_some((left, right))
}

fn promoted_integer_triplet<'a>(
    left: &'a Expression,
    right: &'a Expression,
    is_integer: impl Fn(&Expression) -> bool,
    is_float_load: impl Fn(&Expression) -> bool,
) -> Option<(&'a Expression, &'a Expression, &'a Expression)> {
    let (product, addend) = if as_multiplication(left).is_some() {
        (left, right)
    } else if as_multiplication(right).is_some() {
        (right, left)
    } else {
        return None;
    };
    if !is_float_load(addend) {
        return None;
    }
    let (first, second) = as_multiplication(product)?;
    match (
        is_integer(first),
        is_float_load(first),
        is_integer(second),
        is_float_load(second),
    ) {
        (true, false, false, true) => Some((first, second, addend)),
        (false, true, true, false) => Some((second, first, addend)),
        _ => None,
    }
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

    #[test]
    fn recognizes_an_integer_promoted_fused_triplet() {
        let variable = |name: &str| Expression::Variable(name.into());
        let member = |offset| Expression::Member {
            base: Box::new(variable("data")),
            offset,
            member_type: Type::Float,
            index_stride: None,
        };
        let expression = product(variable("count"), member(4));
        let is_integer =
            |expression: &Expression| matches!(expression, Expression::Variable(name) if name == "count");
        let is_float_load = |expression: &Expression| matches!(expression, Expression::Member {
            member_type: Type::Float,
            ..
        });

        assert!(promoted_integer_triplet(
            &expression,
            &member(8),
            is_integer,
            is_float_load,
        )
        .is_some());
    }

    #[test]
    fn recognizes_both_subtraction_directions_for_promoted_integer_products() {
        let variable = |name: &str| Expression::Variable(name.into());
        let product = product(variable("count"), variable("scale"));
        let is_integer =
            |expression: &Expression| matches!(expression, Expression::Variable(name) if name == "count");
        let is_float =
            |expression: &Expression| matches!(expression, Expression::Variable(name) if name != "count");

        let (_, _, _, base_minus_product) = promoted_integer_register_fusion(
            BinaryOperator::Subtract,
            &variable("base"),
            &product,
            is_integer,
            is_float,
        )
        .expect("base minus promoted product");
        let (_, _, _, product_minus_base) = promoted_integer_register_fusion(
            BinaryOperator::Subtract,
            &product,
            &variable("base"),
            is_integer,
            is_float,
        )
        .expect("promoted product minus base");

        assert_eq!(base_minus_product, PromotedIntegerFusion::BaseMinusProduct);
        assert_eq!(product_minus_base, PromotedIntegerFusion::ProductMinusBase);
    }

    #[test]
    fn recognizes_factor_commutation_in_an_adjacent_linear_interpolation() {
        let variable = |name: &str| Expression::Variable(name.into());
        let index = |index| Expression::Index {
            base: Box::new(variable("samples")),
            index: Box::new(index),
        };
        let amount = variable("amount");
        let complement = Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::FloatLiteral(1.0)),
            right: Box::new(amount.clone()),
        };
        let current = product(index(variable("index")), complement);
        let next_index = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(variable("index")),
            right: Box::new(Expression::IntegerLiteral(1)),
        };
        let next = product(index(next_index), amount);

        let interpolation = adjacent_linear_interpolation(&current, &next)
            .expect("adjacent linear interpolation");
        assert!(matches!(interpolation.amount, Expression::Variable(name) if name == "amount"));
        assert!(matches!(interpolation.base, Expression::Variable(name) if name == "samples"));
        assert!(matches!(interpolation.index, Expression::Variable(name) if name == "index"));
    }
}
