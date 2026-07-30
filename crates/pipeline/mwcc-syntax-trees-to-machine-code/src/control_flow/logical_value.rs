//! N-ary logical values materialized through one shared boolean diamond.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Materialize a pure three-or-more-term `||` chain once.
    ///
    /// Recursive binary lowering creates an intermediate 0/1 diamond for every
    /// nested node. MWCC instead lets every early true test enter one shared
    /// true block, lets the final false test enter one false block, and joins
    /// after a single materialization of each boolean value.
    pub(crate) fn try_emit_flat_logical_or_value(
        &mut self,
        expression: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if self.behavior.logical_or_value_style != mwcc_versions::LogicalOrValueStyle::TrueFirst
            || crate::analysis::expression_has_side_effect(expression)
        {
            return Ok(false);
        }
        if self.try_emit_trailing_global_alternative_value(expression, destination)? {
            return Ok(true);
        }
        if self.try_emit_bounded_array_alternative_value(expression, destination)? {
            return Ok(true);
        }
        let Some(terms) = logical_or_terms(expression) else {
            return Ok(false);
        };

        let true_value = self.fresh_label();
        let false_value = self.fresh_label();
        let join = self.fresh_label();
        let last = terms.len() - 1;
        for (index, term) in terms.into_iter().enumerate() {
            let (false_options, condition_bit) = self.emit_condition_test(term)?;
            self.emit_branch_conditional_to(
                if index == last {
                    false_options
                } else {
                    false_options ^ 8
                },
                condition_bit,
                if index == last {
                    false_value
                } else {
                    true_value
                },
            );
        }

        self.bind_label(true_value);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 1));
        self.emit_branch_to(join);
        self.bind_label(false_value);
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        self.bind_label(join);
        if destination != GENERAL_SCRATCH {
            self.output
                .instructions
                .push(Instruction::move_register(destination, GENERAL_SCRATCH));
        }
        Ok(true)
    }
}

pub(super) fn logical_or_terms(expression: &Expression) -> Option<Vec<&Expression>> {
    fn collect<'a>(expression: &'a Expression, terms: &mut Vec<&'a Expression>) -> bool {
        match expression {
            Expression::Binary {
                operator: BinaryOperator::LogicalOr,
                left,
                right,
            } => collect(left, terms) && collect(right, terms),
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                ..
            } => false,
            _ => {
                terms.push(expression);
                true
            }
        }
    }

    let mut terms = Vec::new();
    (collect(expression, &mut terms) && terms.len() >= 3).then_some(terms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn or(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn flattens_both_associations_without_reordering_terms() {
        let left_associated = or(
            or(
                Expression::Variable("a".into()),
                Expression::Variable("b".into()),
            ),
            Expression::Variable("c".into()),
        );
        let right_associated = or(
            Expression::Variable("a".into()),
            or(
                Expression::Variable("b".into()),
                Expression::Variable("c".into()),
            ),
        );

        for expression in [&left_associated, &right_associated] {
            let terms = logical_or_terms(expression).expect("three-term pure OR");
            let names: Vec<_> = terms.iter().filter_map(|term| leaf_name(term)).collect();
            assert_eq!(names, ["a", "b", "c"]);
        }
    }

    #[test]
    fn leaves_binary_and_mixed_logical_values_to_their_existing_owners() {
        let binary = or(
            Expression::Variable("a".into()),
            Expression::Variable("b".into()),
        );
        assert!(logical_or_terms(&binary).is_none());

        let mixed = or(
            or(
                Expression::Variable("a".into()),
                Expression::Variable("b".into()),
            ),
            Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(Expression::Variable("c".into())),
                right: Box::new(Expression::Variable("d".into())),
            },
        );
        assert!(logical_or_terms(&mixed).is_none());
    }
}
