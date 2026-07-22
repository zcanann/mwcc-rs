//! Scoped reuse of pointer globals within side-effect-free branch conditions.
//!
//! Legacy MWCC retains a nonvolatile global pointer while a short-circuit
//! condition reads several of its members. The cache is deliberately owned by
//! the condition emitter: it never survives into a guarded body or across a
//! call, keeping this a local value-numbering rule rather than global CSE.

use crate::generator::Generator;
use mwcc_core::Compilation;
use mwcc_syntax_trees::{Expression, Type};
use std::collections::HashMap;

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
        let mut counts = HashMap::new();
        if collect_member_pointer_bases(condition, &mut counts) {
            self.condition_global_values = counts
                .into_iter()
                .filter(|(name, count)| {
                    *count >= 2
                        && !self.volatile_globals.contains(name.as_str())
                        && matches!(
                            self.globals.get(name.as_str()),
                            Some(Type::Pointer(_) | Type::StructPointer { .. })
                        )
                })
                .map(|(name, _)| (name, ConditionGlobalValue::Pending))
                .collect();
        }
        previous
    }

    pub(crate) fn restore_condition_global_cache(
        &mut self,
        previous: HashMap<String, ConditionGlobalValue>,
    ) {
        self.condition_global_values = previous;
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

/// Count global-pointer member bases in evaluation order. `false` rejects the
/// entire condition when a call or mutation occurs; those are cache barriers.
fn collect_member_pointer_bases(
    expression: &Expression,
    counts: &mut HashMap<String, usize>,
) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::PostStep { .. }
        | Expression::Assign { .. } => false,
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            if let Expression::Variable(name) = base.as_ref() {
                *counts.entry(name.clone()).or_default() += 1;
            }
            collect_member_pointer_bases(base, counts)
        }
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            collect_member_pointer_bases(left, counts)
                && collect_member_pointer_bases(right, counts)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_member_pointer_bases(condition, counts)
                && collect_member_pointer_bases(when_true, counts)
                && collect_member_pointer_bases(when_false, counts)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::IndexedUpdateValue { value: operand } => {
            collect_member_pointer_bases(operand, counts)
        }
        Expression::BitFieldRead { extracted, .. } => {
            collect_member_pointer_bases(extracted, counts)
        }
        Expression::AggregateLiteral(values) => values
            .iter()
            .all(|value| collect_member_pointer_bases(value, counts)),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => true,
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
        let mut counts = HashMap::new();
        assert!(collect_member_pointer_bases(&condition, &mut counts));
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
        let mut counts = HashMap::new();
        assert!(!collect_member_pointer_bases(&condition, &mut counts));
    }
}
