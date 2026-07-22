//! Scoped reuse of pointer globals within side-effect-free branch conditions.
//!
//! Legacy MWCC retains a nonvolatile global pointer while a short-circuit
//! condition reads several of its members. The cache is deliberately owned by
//! the condition emitter: it never survives into a guarded body or across a
//! call, keeping this a local value-numbering rule rather than global CSE.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Expression, Type};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub(crate) enum ConditionGlobalValue {
    Pending,
    Register(u8),
}

impl Generator {
    pub(crate) fn begin_condition_global_cache(
        &mut self,
        condition: &Expression,
    ) -> HashMap<String, ConditionGlobalValue> {
        let previous = std::mem::take(&mut self.condition_global_values);
        self.condition_global_values = self.condition_global_cache_for(condition, None);
        previous
    }

    /// Advance a cache carried along the fallthrough edge of a prior early-
    /// return guard. Eligible names keep their existing register; names first
    /// used by this condition begin pending as usual.
    pub(crate) fn continue_condition_global_cache(&mut self, condition: &Expression) {
        let previous = std::mem::take(&mut self.condition_global_values);
        self.condition_global_values = self.condition_global_cache_for(condition, Some(&previous));
    }

    fn condition_global_cache_for(
        &self,
        condition: &Expression,
        reusable: Option<&HashMap<String, ConditionGlobalValue>>,
    ) -> HashMap<String, ConditionGlobalValue> {
        cacheable_member_pointer_bases(condition)
            .into_iter()
            .filter(|(name, count)| {
                *count >= 2
                    && !self.volatile_globals.contains(name.as_str())
                    && matches!(
                        self.globals.get(name.as_str()),
                        Some(Type::Pointer(_) | Type::StructPointer { .. })
                    )
            })
            .map(|(name, _)| {
                let value = reusable
                    .and_then(|values| values.get(&name))
                    .copied()
                    .unwrap_or(ConditionGlobalValue::Pending);
                (name, value)
            })
            .collect()
    }

    pub(crate) fn restore_condition_global_cache(
        &mut self,
        previous: HashMap<String, ConditionGlobalValue>,
    ) {
        self.condition_global_values = previous;
    }

    /// Materialize cacheable bases before evaluating the first comparison.
    /// MWCC hoists these independent pointer loads in source encounter order,
    /// even when the first member access occurs on a later `&&` term.
    pub(crate) fn preload_condition_global_cache(
        &mut self,
        condition: &Expression,
    ) -> Compilation<()> {
        let mut names = Vec::new();
        collect_member_pointer_base_order(condition, &mut names, &mut HashSet::new());
        for name in names {
            if matches!(
                self.condition_global_values.get(&name),
                Some(ConditionGlobalValue::Pending)
            ) {
                self.condition_global_base(&name)?;
            }
        }
        Ok(())
    }

    pub(crate) fn condition_global_base(&mut self, name: &str) -> Compilation<Option<u8>> {
        match self.condition_global_values.get(name).copied() {
            None => Ok(None),
            Some(ConditionGlobalValue::Register(register)) => Ok(Some(register)),
            Some(ConditionGlobalValue::Pending) => {
                let register = self.fresh_virtual_general();
                self.emit_global_load_value(name, register)?;
                self.condition_global_values
                    .insert(name.to_owned(), ConditionGlobalValue::Register(register));
                Ok(Some(register))
            }
        }
    }
}

fn collect_member_pointer_base_order(
    expression: &Expression,
    names: &mut Vec<String>,
    seen: &mut HashSet<String>,
) {
    match expression {
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            if let Expression::Variable(name) = base.as_ref() {
                if seen.insert(name.clone()) {
                    names.push(name.clone());
                }
            }
            collect_member_pointer_base_order(base, names, seen);
        }
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            collect_member_pointer_base_order(left, names, seen);
            collect_member_pointer_base_order(right, names, seen);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_member_pointer_base_order(condition, names, seen);
            collect_member_pointer_base_order(when_true, names, seen);
            collect_member_pointer_base_order(when_false, names, seen);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                collect_member_pointer_base_order(argument, names, seen);
            }
        }
        Expression::CallThrough {
            target,
            arguments,
            ..
        } => {
            collect_member_pointer_base_order(target, names, seen);
            for argument in arguments {
                collect_member_pointer_base_order(argument, names, seen);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect_member_pointer_base_order(object, names, seen);
            for argument in arguments {
                collect_member_pointer_base_order(argument, names, seen);
            }
        }
        Expression::ConstructedNew { arguments, .. } => {
            for argument in arguments {
                collect_member_pointer_base_order(argument, names, seen);
            }
        }
        Expression::PostStep { target, .. } => {
            collect_member_pointer_base_order(target, names, seen);
        }
        Expression::Assign { target, value, .. } => {
            collect_member_pointer_base_order(target, names, seen);
            collect_member_pointer_base_order(value, names, seen);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::BitFieldRead {
            extracted: operand, ..
        } => collect_member_pointer_base_order(operand, names, seen),
        Expression::AggregateLiteral(values) => {
            for value in values {
                collect_member_pointer_base_order(value, names, seen);
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

/// Count global-pointer member bases in the pure prefix of an expression.
/// Calls and mutations are evaluation-order barriers: a name read again after
/// one is removed entirely, while values used only before the barrier remain
/// safe to reuse. This models `a->x && a->y && call()` without extending `a`
/// across the call or allowing `call() && a->x` to consume a stale value.
fn cacheable_member_pointer_bases(expression: &Expression) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    let mut after_barrier = HashSet::new();
    let mut barrier_seen = false;
    collect_member_pointer_bases(
        expression,
        &mut counts,
        &mut after_barrier,
        &mut barrier_seen,
    );
    counts.retain(|name, _| !after_barrier.contains(name));
    counts
}

fn collect_member_pointer_bases(
    expression: &Expression,
    counts: &mut HashMap<String, usize>,
    after_barrier: &mut HashSet<String>,
    barrier_seen: &mut bool,
) {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. }
        | Expression::PostStep { .. }
        | Expression::Assign { .. } => *barrier_seen = true,
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            if let Expression::Variable(name) = base.as_ref() {
                if *barrier_seen {
                    after_barrier.insert(name.clone());
                } else {
                    *counts.entry(name.clone()).or_default() += 1;
                }
            }
            collect_member_pointer_bases(base, counts, after_barrier, barrier_seen);
        }
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            collect_member_pointer_bases(left, counts, after_barrier, barrier_seen);
            collect_member_pointer_bases(right, counts, after_barrier, barrier_seen);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_member_pointer_bases(condition, counts, after_barrier, barrier_seen);
            // The arms are mutually exclusive. Treat their join as a barrier
            // so no register value is inferred to flow from one arm to the other.
            *barrier_seen = true;
            collect_member_pointer_bases(when_true, counts, after_barrier, barrier_seen);
            collect_member_pointer_bases(when_false, counts, after_barrier, barrier_seen);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::IndexedUpdateValue { value: operand } => {
            collect_member_pointer_bases(operand, counts, after_barrier, barrier_seen);
        }
        Expression::BitFieldRead { extracted, .. } => {
            collect_member_pointer_bases(extracted, counts, after_barrier, barrier_seen);
        }
        Expression::AggregateLiteral(values) => {
            for value in values {
                collect_member_pointer_bases(value, counts, after_barrier, barrier_seen);
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(global: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(global.into())),
            offset,
            member_type: Type::Int,
            index_stride: None,
        }
    }

    #[test]
    fn finds_repeated_member_bases_in_a_pure_condition() {
        let condition = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(member("limits", 0)),
            right: Box::new(member("limits", 4)),
        };
        let counts = cacheable_member_pointer_bases(&condition);
        assert_eq!(counts.get("limits"), Some(&2));
    }

    #[test]
    fn a_call_rejects_condition_wide_reuse() {
        let condition = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(member("limits", 0)),
            right: Box::new(Expression::Call {
                name: "test".into(),
                arguments: vec![member("limits", 4)],
            }),
        };
        let counts = cacheable_member_pointer_bases(&condition);
        assert_eq!(counts.get("limits"), Some(&1));
    }

    #[test]
    fn retains_a_repeated_pure_prefix_before_a_trailing_call() {
        let pure_prefix = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(member("limits", 0)),
            right: Box::new(member("limits", 4)),
        };
        let condition = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(pure_prefix),
            right: Box::new(Expression::Call {
                name: "test".into(),
                arguments: vec![],
            }),
        };

        let counts = cacheable_member_pointer_bases(&condition);
        assert_eq!(counts.get("limits"), Some(&2));
    }

    #[test]
    fn excludes_a_name_read_again_after_a_call() {
        let before = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(member("limits", 0)),
            right: Box::new(member("limits", 4)),
        };
        let call_then_read = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Call {
                name: "test".into(),
                arguments: vec![],
            }),
            right: Box::new(member("limits", 8)),
        };
        let condition = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(before),
            right: Box::new(call_then_read),
        };

        let counts = cacheable_member_pointer_bases(&condition);
        assert!(!counts.contains_key("limits"));
    }

    #[test]
    fn records_first_member_base_occurrences_in_evaluation_order() {
        let condition = Expression::Binary {
            operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
            left: Box::new(member("later", 0)),
            right: Box::new(Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::LogicalAnd,
                left: Box::new(member("first", 0)),
                right: Box::new(member("later", 4)),
            }),
        };
        let mut names = Vec::new();
        collect_member_pointer_base_order(&condition, &mut names, &mut HashSet::new());
        assert_eq!(names, ["later", "first"]);
    }
}
