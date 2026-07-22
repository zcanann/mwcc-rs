//! Float memory values retained along side-effect-free condition edges.
//!
//! MWCC can keep a loaded float live when an early-return guard falls through
//! into the next condition.  This cache models only that control-flow edge. It
//! is restored before the guarded body is emitted, and calls or mutations make
//! a condition ineligible to feed a later one.

use crate::generator::Generator;
use mwcc_syntax_trees::Expression;

#[derive(Clone)]
pub(crate) struct ConditionFloatValue {
    expression: Expression,
    register: u8,
}

#[derive(Clone, Default)]
pub(crate) struct ConditionFloatCache {
    active: bool,
    recording_allowed: bool,
    reusable: Vec<ConditionFloatValue>,
    observed: Vec<ConditionFloatValue>,
}

impl Generator {
    pub(crate) fn begin_condition_float_cache(
        &mut self,
        condition: &Expression,
    ) -> ConditionFloatCache {
        let previous = std::mem::take(&mut self.condition_float_cache);
        self.condition_float_cache.active = true;
        self.condition_float_cache.recording_allowed = !expression_has_value_barrier(condition);
        previous
    }

    /// Carry values only onto the pure prefix of the next condition. A later
    /// call in that condition does not prevent an earlier comparison from
    /// consuming the value, but it does prevent that condition from feeding a
    /// third guard.
    pub(crate) fn continue_condition_float_cache(&mut self, condition: &Expression) {
        let previous = std::mem::take(&mut self.condition_float_cache);
        self.condition_float_cache.active = true;
        self.condition_float_cache.recording_allowed = !expression_has_value_barrier(condition);
        self.condition_float_cache.reusable = previous
            .observed
            .into_iter()
            .filter(|value| pure_prefix_contains(condition, &value.expression, &mut false))
            .collect();
    }

    pub(crate) fn restore_condition_float_cache(&mut self, previous: ConditionFloatCache) {
        self.condition_float_cache = previous;
    }

    pub(crate) fn condition_float_register(&mut self, operand: &Expression) -> Option<u8> {
        let index = self
            .condition_float_cache
            .reusable
            .iter()
            .position(|value| same_direct_float_memory_load(&value.expression, operand))?;
        Some(self.condition_float_cache.reusable.remove(index).register)
    }

    pub(crate) fn record_condition_float_value(&mut self, operand: &Expression, register: u8) {
        if !self.condition_float_cache.active
            || !self.condition_float_cache.recording_allowed
            || !is_direct_float_memory_load(operand)
            // MWCC keeps an entry-parameter member live here, but reloads the
            // same shape through a local pointer alias (measured in Melee's
            // CaptureWaitKirby guard). Preserve that alias boundary instead of
            // treating two syntactically equal addresses as proven identical.
            || direct_memory_base_name(operand).is_none_or(|name| {
                self.known_locals.contains(name) || !self.locations.contains_key(name)
            })
            || self
                .condition_float_cache
                .observed
                .iter()
                .any(|value| same_direct_float_memory_load(&value.expression, operand))
        {
            return;
        }
        self.condition_float_cache
            .observed
            .push(ConditionFloatValue {
                expression: operand.clone(),
                register,
            });
    }
}

fn direct_memory_base_name(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Member { base, .. }
        | Expression::Dereference { pointer: base }
        | Expression::Index { base, .. } => match base.as_ref() {
            Expression::Variable(name) => Some(name),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn is_direct_float_memory_load(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Member {
            member_type: mwcc_syntax_trees::Type::Float | mwcc_syntax_trees::Type::Double,
            ..
        } | Expression::Dereference { .. }
            | Expression::Index { .. }
    )
}

pub(crate) fn same_direct_float_memory_load(left: &Expression, right: &Expression) -> bool {
    match (left, right) {
        (
            Expression::Member {
                base: left_base,
                offset: left_offset,
                member_type: left_type,
                index_stride: left_stride,
            },
            Expression::Member {
                base: right_base,
                offset: right_offset,
                member_type: right_type,
                index_stride: right_stride,
            },
        ) => {
            left_offset == right_offset
                && left_type == right_type
                && left_stride == right_stride
                && same_address_expression(left_base, right_base)
        }
        (Expression::Dereference { pointer: left }, Expression::Dereference { pointer: right }) => {
            same_address_expression(left, right)
        }
        (
            Expression::Index {
                base: left_base,
                index: left_index,
            },
            Expression::Index {
                base: right_base,
                index: right_index,
            },
        ) => {
            same_address_expression(left_base, right_base)
                && same_address_expression(left_index, right_index)
        }
        _ => false,
    }
}

fn same_address_expression(left: &Expression, right: &Expression) -> bool {
    match (left, right) {
        (Expression::Variable(left), Expression::Variable(right)) => left == right,
        (Expression::IntegerLiteral(left), Expression::IntegerLiteral(right)) => left == right,
        _ => same_direct_float_memory_load(left, right),
    }
}

fn expression_has_value_barrier(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::PostStep { .. }
        | Expression::Assign { .. } => true,
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            expression_has_value_barrier(left) || expression_has_value_barrier(right)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_has_value_barrier(condition)
                || expression_has_value_barrier(when_true)
                || expression_has_value_barrier(when_false)
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
        } => expression_has_value_barrier(base),
        Expression::AggregateLiteral(values) => values.iter().any(expression_has_value_barrier),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => false,
    }
}

fn pure_prefix_contains(expression: &Expression, target: &Expression, barrier: &mut bool) -> bool {
    if *barrier {
        return false;
    }
    if same_direct_float_memory_load(expression, target) {
        return true;
    }
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::PostStep { .. }
        | Expression::Assign { .. } => {
            *barrier = true;
            false
        }
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            pure_prefix_contains(left, target, barrier)
                || pure_prefix_contains(right, target, barrier)
        }
        Expression::Conditional { condition, .. } => {
            let found = pure_prefix_contains(condition, target, barrier);
            *barrier = true;
            found
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
        } => pure_prefix_contains(base, target, barrier),
        Expression::AggregateLiteral(values) => values
            .iter()
            .any(|value| pure_prefix_contains(value, target, barrier)),
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
    use mwcc_syntax_trees::{BinaryOperator, Type};

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset,
            member_type: Type::Float,
            index_stride: None,
        }
    }

    #[test]
    fn finds_repeated_load_before_a_trailing_call() {
        let target = member(0);
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::LessEqual,
                left: Box::new(target.clone()),
                right: Box::new(member(8)),
            }),
            right: Box::new(Expression::Call {
                name: "check".into(),
                arguments: vec![],
            }),
        };
        assert!(pure_prefix_contains(&condition, &target, &mut false));
    }

    #[test]
    fn rejects_a_load_after_a_call() {
        let target = member(0);
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Call {
                name: "check".into(),
                arguments: vec![],
            }),
            right: Box::new(target.clone()),
        };
        assert!(!pure_prefix_contains(&condition, &target, &mut false));
    }
}
