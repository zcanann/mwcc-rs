//! Paired complement-product initialization for structured legacy bodies.
//!
//! Build 163 treats two adjacent four-factor products as one floating DAG. It
//! loads the shared `1.0f` once, overlaps independent member loads from both
//! products, and balances each multiply chain. A retained scalar initializer
//! can fill the final product's issue window. This plan preserves those source
//! dependencies until all physical homes are known.

#[allow(unused_imports)]
use super::*;
use crate::generator::{float_compare_literal_key, PreloadedFloatCompareLiteral};

pub(super) struct StructuredComplementProductPair {
    product_names: [String; 2],
    products: [[Expression; 4]; 2],
    interleaved_general_name: String,
    interleaved_general_type: Type,
    interleaved_general_initializer: Expression,
    threshold: Expression,
}

impl StructuredComplementProductPair {
    pub(super) fn plan(
        function: &Function,
        saved_float_locals: &[&LocalDeclaration],
        eager_general_locals: &[&LocalDeclaration],
        frame_convention: FrameConvention,
    ) -> Option<Self> {
        if frame_convention != FrameConvention::LinkageFirst {
            return None;
        }
        if saved_float_locals.len() != 2
            || saved_float_locals
                .iter()
                .any(|local| local.declared_type != Type::Float)
        {
            return None;
        }
        let [Statement::Assign {
            name: first_name,
            value: first_value,
        }, Statement::Assign {
            name: second_name,
            value: second_value,
        }, ..] = function.statements.as_slice()
        else {
            return None;
        };
        if first_name == second_name
            || ![first_name, second_name].iter().all(|name| {
                saved_float_locals
                    .iter()
                    .any(|local| local.name.as_str() == name.as_str())
            })
        {
            return None;
        }
        let products = [
            complement_product(first_value)?,
            complement_product(second_value)?,
        ];
        let [interleaved] = eager_general_locals else {
            return None;
        };
        let initializer = interleaved.initializer.as_ref()?;
        let loaded_initializer = match initializer {
            Expression::Cast { operand, .. } => operand.as_ref(),
            expression => expression,
        };
        if !matches!(
            loaded_initializer,
            Expression::Member {
                member_type: Type::Pointer(_) | Type::Int | Type::UnsignedInt,
                ..
            }
        ) {
            return None;
        }
        let threshold = leading_product_threshold(function, first_name)?;

        Some(Self {
            product_names: [first_name.clone(), second_name.clone()],
            products,
            interleaved_general_name: interleaved.name.clone(),
            interleaved_general_type: interleaved.declared_type,
            interleaved_general_initializer: initializer.clone(),
            threshold,
        })
    }

    pub(super) fn interleaves_general_initializer(&self, name: &str) -> bool {
        self.interleaved_general_name == name
    }

    pub(super) fn product_names(&self) -> [&str; 2] {
        [&self.product_names[0], &self.product_names[1]]
    }

    pub(super) fn consumed_statement_prefix(&self) -> usize {
        2
    }

    pub(super) fn saved_general_home_preference(
        &self,
        total_homes: usize,
        home_index: usize,
    ) -> Option<u8> {
        (total_homes == 2 && home_index < 2).then_some(30 + home_index as u8)
    }
}

impl Generator {
    pub(super) fn emit_structured_complement_product_pair(
        &mut self,
        plan: &StructuredComplementProductPair,
        destinations: [u8; 2],
    ) -> Compilation<()> {
        const ONE: u8 = 7;
        self.load_float_constant(ONE, 1.0);
        self.preloaded_float_compare_literals
            .push(PreloadedFloatCompareLiteral {
                key: float_compare_literal_key(&Expression::FloatLiteral(1.0), false)
                    .expect("one is a float literal"),
                register: ONE,
                remaining_uses: 1,
                reuse_for_following_value: false,
            });

        self.evaluate_float(&plan.products[0][0], 2)?;
        self.evaluate_float(&plan.products[0][1], 1)?;
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 4, a: ONE, b: 2 });
        self.evaluate_float(&plan.products[0][2], 5)?;
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 3, a: ONE, b: 1 });
        self.evaluate_float(&plan.products[1][0], 2)?;
        self.evaluate_float(&plan.products[1][1], 1)?;
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 4, a: 4, c: 3 });
        self.evaluate_float(&plan.products[0][3], 6)?;
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 5, a: ONE, b: 5 });
        self.evaluate_float(&plan.products[1][2], 3)?;
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 2, a: ONE, b: 2 });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 1, a: ONE, b: 1 });

        let Expression::FloatLiteral(threshold) = plan.threshold else {
            unreachable!("the complement-product threshold was recognized")
        };
        self.load_float_constant(0, threshold as f32);
        self.preloaded_float_compare_literals
            .push(PreloadedFloatCompareLiteral {
                key: float_compare_literal_key(&plan.threshold, false)
                    .expect("the threshold is a float literal"),
                register: 0,
                remaining_uses: 1,
                reuse_for_following_value: true,
            });

        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 5, a: 5, c: 4 });
        self.evaluate_float(&plan.products[1][3], 4)?;
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 6, a: ONE, b: 6 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 2, c: 1 });

        let general_home = self
            .lookup_general(&plan.interleaved_general_name)
            .ok_or_else(|| {
                Diagnostic::error("interleaved complement-product scalar has no saved home")
            })?;
        self.evaluate(
            &plan.interleaved_general_initializer,
            plan.interleaved_general_type,
            general_home,
        )?;

        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 3, a: ONE, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: destinations[0],
                a: 6,
                c: 5,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 2, a: ONE, b: 4 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 3, c: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle {
                d: destinations[1],
                a: 2,
                c: 1,
            });
        Ok(())
    }
}

fn complement_product(expression: &Expression) -> Option<[Expression; 4]> {
    let mut terms = Vec::new();
    collect_product_terms(expression, &mut terms);
    let [leading, factors @ ..] = terms.as_slice() else {
        return None;
    };
    if !is_one(leading) || factors.len() != 4 {
        return None;
    }
    factors
        .iter()
        .map(|factor| {
            let Expression::Binary {
                operator: BinaryOperator::Subtract,
                left,
                right,
            } = *factor
            else {
                return None;
            };
            (is_one(left) && crate::condition_float_cache::is_direct_float_memory_load(right))
                .then(|| right.as_ref().clone())
        })
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn collect_product_terms<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) {
    if let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    {
        collect_product_terms(left, terms);
        collect_product_terms(right, terms);
    } else {
        terms.push(expression);
    }
}

fn is_one(expression: &Expression) -> bool {
    matches!(expression, Expression::FloatLiteral(value) if *value as f32 == 1.0)
        || matches!(expression, Expression::IntegerLiteral(1))
}

fn leading_product_threshold(function: &Function, product: &str) -> Option<Expression> {
    function.statements.iter().find_map(|statement| {
        let Statement::If { condition, .. } = statement else {
            return None;
        };
        find_product_threshold(condition, product)
    })
}

fn find_product_threshold(expression: &Expression, product: &str) -> Option<Expression> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return None;
    };
    if *operator == BinaryOperator::LogicalOr {
        return find_product_threshold(left, product)
            .or_else(|| find_product_threshold(right, product));
    }
    if !matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::LessEqual
            | BinaryOperator::Greater
            | BinaryOperator::GreaterEqual
    ) {
        return None;
    }
    match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), literal @ Expression::FloatLiteral(_))
        | (literal @ Expression::FloatLiteral(_), Expression::Variable(name))
            if name == product =>
        {
            Some(literal.clone())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("pads".into())),
            offset,
            member_type: Type::Float,
            index_stride: None,
        }
    }

    fn multiply(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn recognizes_a_four_factor_complement_product() {
        let complement = |offset| Expression::Binary {
            operator: BinaryOperator::Subtract,
            left: Box::new(Expression::FloatLiteral(1.0)),
            right: Box::new(member(offset)),
        };
        let expression = multiply(
            multiply(
                multiply(
                    multiply(Expression::FloatLiteral(1.0), complement(0)),
                    complement(4),
                ),
                complement(8),
            ),
            complement(12),
        );

        let factors = complement_product(&expression).expect("the product should match");
        assert!(matches!(factors[3], Expression::Member { offset: 12, .. }));
    }
}
