//! Dominance rules for a float value handed to an immediately guarded use.
//!
//! The cache mechanics live in the parent module. This module decides whether
//! a direct truthiness load is present on every path into the guarded body, so
//! expression-level float retention remains independent of statement edges.

use super::*;
use mwcc_syntax_trees::{BinaryOperator, UnaryOperator};

impl Generator {
    pub(crate) fn condition_float_value_is_retained_by_guarded_followup(
        &self,
        operand: &Expression,
    ) -> bool {
        self.condition_float_cache
            .guarded_followup
            .as_ref()
            .zip(self.condition_float_cache.condition.as_ref())
            .is_some_and(|(followup, condition)| {
                (self.condition_float_cache.comparison_followup
                    || condition_tests_float_truth(condition, operand))
                    && true_edge_guarantees_value(condition, operand)
                    && pure_prefix_contains(followup, operand, &mut false)
            })
    }

    /// Build the cache for a first, immediately guarded statement.
    ///
    /// The structured statement owner proves that no source statement lies
    /// before this statement. Retain only values that dominate the condition's
    /// true edge and occur in the pure prefix of the guarded expression.
    pub(crate) fn condition_float_true_edge_cache(
        &self,
        followup: &Expression,
    ) -> ConditionFloatCache {
        let intra_condition = self
            .condition_float_cache
            .condition
            .as_ref()
            .map(|previous_condition| {
                self.condition_float_cache
                    .edge_observed
                    .iter()
                    .filter(|value| {
                        true_edge_guarantees_value(previous_condition, &value.expression)
                            && pure_prefix_contains(
                                followup,
                                &value.expression,
                                &mut false,
                            )
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        ConditionFloatCache {
            active: self.condition_float_cache.active,
            guarded_edge: true,
            recording_allowed: self.condition_float_cache.recording_allowed,
            intra_condition,
            zero_register: self.condition_float_cache.zero_register,
            literals: self.condition_float_cache.literals.clone(),
            ..ConditionFloatCache::default()
        }
    }

    /// Build the cache for the false arm of a plain comparison. Both operands
    /// of a non-short-circuit comparison dominate either selected edge, unlike
    /// `&&` and `||`, whose false edge can be reached before a later term.
    pub(crate) fn condition_float_plain_false_edge_cache(
        &self,
        followup: &Expression,
    ) -> ConditionFloatCache {
        let intra_condition = self
            .condition_float_cache
            .condition
            .as_ref()
            .filter(|condition| {
                matches!(
                    condition,
                    Expression::Binary {
                        operator: BinaryOperator::Equal
                            | BinaryOperator::NotEqual
                            | BinaryOperator::Less
                            | BinaryOperator::LessEqual
                            | BinaryOperator::Greater
                            | BinaryOperator::GreaterEqual,
                        ..
                    }
                )
            })
            .map(|condition| {
                self.condition_float_cache
                    .edge_observed
                    .iter()
                    .filter(|value| {
                        evaluation_guarantees_load(condition, &value.expression)
                            && pure_prefix_contains(
                                followup,
                                &value.expression,
                                &mut false,
                            )
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        ConditionFloatCache {
            active: self.condition_float_cache.active,
            guarded_edge: true,
            recording_allowed: self.condition_float_cache.recording_allowed,
            intra_condition,
            zero_register: self.condition_float_cache.zero_register,
            literals: self.condition_float_cache.literals.clone(),
            ..ConditionFloatCache::default()
        }
    }
}

/// Whether every path that selects a condition's true edge evaluates `target`.
///
/// This is deliberately stricter than syntactic containment. Both operands of
/// `&&` run when the result is true, while the right operand of `||` may be
/// skipped. A value merely seen while emitting a condition is therefore not
/// automatically available to its guarded body.
fn true_edge_guarantees_value(expression: &Expression, target: &Expression) -> bool {
    if same_retained_float_expression(expression, target) {
        return true;
    }
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } => {
            true_edge_guarantees_value(left, target)
                || true_edge_guarantees_value(right, target)
        }
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left,
            ..
        } => evaluation_guarantees_load(left, target),
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            evaluation_guarantees_load(left, target)
                || evaluation_guarantees_load(right, target)
        }
        Expression::Conditional { condition, .. } => {
            evaluation_guarantees_load(condition, target)
        }
        Expression::Member { base, .. }
        | Expression::MemberAddress { base, .. }
        | Expression::Unary { operand: base, .. }
        | Expression::Cast { operand: base, .. }
        | Expression::Dereference { pointer: base }
        | Expression::AddressOf { operand: base }
        | Expression::IndexedUpdateValue { value: base }
        | Expression::BitFieldRead {
            extracted: base, ..
        }
        | Expression::PostStep { target: base, .. } => {
            evaluation_guarantees_load(base, target)
        }
        Expression::Assign {
            target: left,
            value: right,
        } => {
            evaluation_guarantees_load(left, target)
                || evaluation_guarantees_load(right, target)
        }
        Expression::Call { arguments, .. }
        | Expression::CallThrough { arguments, .. }
        | Expression::VirtualCall { arguments, .. }
        | Expression::ConstructedNew { arguments, .. }
        | Expression::AggregateLiteral(arguments) => arguments
            .iter()
            .any(|argument| evaluation_guarantees_load(argument, target)),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

/// Whether `target` is itself a boolean term of the condition.
fn condition_tests_float_truth(expression: &Expression, target: &Expression) -> bool {
    if same_retained_float_expression(expression, target) {
        return true;
    }
    match expression {
        Expression::Unary {
            operator: UnaryOperator::LogicalNot,
            operand,
        } => condition_tests_float_truth(operand, target),
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
            left,
            right,
        } => {
            condition_tests_float_truth(left, target)
                || condition_tests_float_truth(right, target)
        }
        _ => false,
    }
}

/// Whether evaluating an expression at all necessarily evaluates `target`.
fn evaluation_guarantees_load(expression: &Expression, target: &Expression) -> bool {
    if same_retained_float_expression(expression, target) {
        return true;
    }
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
            left,
            ..
        } => evaluation_guarantees_load(left, target),
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right }
        | Expression::Assign {
            target: left,
            value: right,
        } => {
            evaluation_guarantees_load(left, target)
                || evaluation_guarantees_load(right, target)
        }
        Expression::Conditional { condition, .. } => {
            evaluation_guarantees_load(condition, target)
        }
        Expression::Member { base, .. }
        | Expression::MemberAddress { base, .. }
        | Expression::Unary { operand: base, .. }
        | Expression::Cast { operand: base, .. }
        | Expression::Dereference { pointer: base }
        | Expression::AddressOf { operand: base }
        | Expression::IndexedUpdateValue { value: base }
        | Expression::BitFieldRead {
            extracted: base, ..
        }
        | Expression::PostStep { target: base, .. } => {
            evaluation_guarantees_load(base, target)
        }
        Expression::Call { arguments, .. }
        | Expression::CallThrough { arguments, .. }
        | Expression::VirtualCall { arguments, .. }
        | Expression::ConstructedNew { arguments, .. }
        | Expression::AggregateLiteral(arguments) => arguments
            .iter()
            .any(|argument| evaluation_guarantees_load(argument, target)),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::Type;

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset,
            member_type: Type::Float,
            index_stride: None,
        }
    }

    #[test]
    fn true_and_edge_retains_a_load_from_its_right_term() {
        let target = member(0);
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(member(8)),
            right: Box::new(target.clone()),
        };

        assert!(true_edge_guarantees_value(&condition, &target));
    }

    #[test]
    fn true_or_edge_does_not_retain_a_load_from_its_skippable_right_term() {
        let target = member(0);
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(member(8)),
            right: Box::new(target.clone()),
        };

        assert!(!true_edge_guarantees_value(&condition, &target));
    }

    #[test]
    fn comparison_operand_is_not_a_direct_float_truth_test() {
        let target = member(0);
        let condition = Expression::Binary {
            operator: BinaryOperator::Greater,
            left: Box::new(target.clone()),
            right: Box::new(Expression::FloatLiteral(0.0)),
        };

        assert!(!condition_tests_float_truth(&condition, &target));
    }
}
