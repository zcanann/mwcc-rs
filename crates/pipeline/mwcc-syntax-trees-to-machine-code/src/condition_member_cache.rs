//! Scoped reuse of full-width integer member loads inside short-circuit conditions.
//!
//! Each later term is reached only through the previous term's fallthrough, so
//! a repeated direct member load remains available when no store, call, or
//! register definition intervenes. This is true for the false edge of OR and
//! the true edge of AND; both edges are dominated by the earlier member load.
//! Narrow loads remain term-local: MWCC reloads their byte or halfword storage
//! rather than carrying the scratch value into the next term.

use crate::generator::Generator;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression, Type};

#[derive(Clone)]
struct ConditionMemberValue {
    expression: Expression,
    register: u8,
    instruction_index: usize,
}

#[derive(Clone, Default)]
pub(crate) struct ConditionMemberCache {
    active: bool,
    values: Vec<ConditionMemberValue>,
    assignment_reuse: Option<Expression>,
    derived_minus_one: Option<ConditionMemberValue>,
}

impl Generator {
    pub(crate) fn begin_condition_member_cache(
        &mut self,
        condition: &Expression,
    ) -> ConditionMemberCache {
        self.begin_condition_member_cache_with_edge_reuse(condition, false)
    }

    /// Open the same scoped cache for a single comparison when its loaded
    /// member is explicitly planned to feed the first statement on either
    /// outgoing edge.
    pub(crate) fn begin_condition_member_cache_with_edge_reuse(
        &mut self,
        condition: &Expression,
        edge_reuse: bool,
    ) -> ConditionMemberCache {
        let previous = std::mem::take(&mut self.condition_member_cache);
        self.condition_member_cache.active = edge_reuse || is_short_circuit_chain(condition);
        previous
    }

    pub(crate) fn begin_assignment_condition_member_cache(
        &mut self,
        member: Expression,
    ) -> ConditionMemberCache {
        let previous = std::mem::take(&mut self.condition_member_cache);
        self.condition_member_cache.active = true;
        self.condition_member_cache.assignment_reuse = Some(member);
        previous
    }

    pub(crate) fn restore_condition_member_cache(
        &mut self,
        previous: ConditionMemberCache,
    ) {
        self.condition_member_cache = previous;
    }

    pub(crate) fn condition_member_cache_rebased(
        &self,
        cache: &ConditionMemberCache,
    ) -> ConditionMemberCache {
        let mut cache = cache.clone();
        for value in &mut cache.values {
            value.instruction_index = self.output.instructions.len();
        }
        cache
    }

    pub(crate) fn fix_condition_member_value_register(
        &mut self,
        operand: &Expression,
        register: u8,
    ) -> bool {
        let Some(value) = self
            .condition_member_cache
            .values
            .iter_mut()
            .rev()
            .find(|value| same_member(&value.expression, operand))
        else {
            return false;
        };
        let Some(load_index) = value.instruction_index.checked_sub(1) else {
            return false;
        };
        let old = value.register;
        let Some(Instruction::LoadWord { d, .. }) =
            self.output.instructions.get_mut(load_index)
        else {
            return false;
        };
        if *d != old {
            return false;
        }
        *d = register;
        let Some(
            Instruction::CompareWordImmediate { a, .. }
            | Instruction::CompareLogicalWordImmediate { a, .. },
        ) = self.output.instructions.get_mut(load_index + 1)
        else {
            return false;
        };
        if *a != old {
            return false;
        }
        *a = register;
        value.register = register;
        true
    }

    pub(crate) fn condition_member_register(
        &self,
        operand: &Expression,
    ) -> Option<u8> {
        if !self.condition_member_cache.active || !cacheable_member(operand, self) {
            return None;
        }
        self.condition_member_cache
            .values
            .iter()
            .rev()
            .find(|value| {
                same_member(&value.expression, operand)
                    && self.condition_member_value_is_live(value)
            })
            .map(|value| value.register)
    }

    pub(crate) fn record_condition_member_value(
        &mut self,
        operand: &Expression,
        register: u8,
    ) {
        if !self.condition_member_cache.active || !cacheable_member(operand, self) {
            return;
        }
        self.condition_member_cache
            .values
            .retain(|value| !same_member(&value.expression, operand));
        self.condition_member_cache
            .values
            .push(ConditionMemberValue {
                expression: operand.clone(),
                register,
                instruction_index: self.output.instructions.len(),
            });
    }

    pub(crate) fn assignment_condition_reuses_member(
        &self,
        operand: &Expression,
    ) -> bool {
        self.condition_member_cache
            .assignment_reuse
            .as_ref()
            .is_some_and(|planned| same_member(planned, operand))
    }

    pub(crate) fn record_assignment_condition_minus_one(
        &mut self,
        member: &Expression,
        register: u8,
    ) {
        if !self.assignment_condition_reuses_member(member) {
            return;
        }
        self.condition_member_cache.derived_minus_one = Some(ConditionMemberValue {
            expression: member.clone(),
            register,
            instruction_index: self.output.instructions.len(),
        });
    }

    pub(crate) fn assignment_condition_minus_one_register(
        &self,
        expression: &Expression,
    ) -> Option<u8> {
        let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } = expression
        else {
            return None;
        };
        if !matches!(right.as_ref(), Expression::IntegerLiteral(1)) {
            return None;
        }
        self.condition_member_cache
            .derived_minus_one
            .as_ref()
            .filter(|value| {
                same_member(&value.expression, left)
                    && self.condition_member_value_is_live(value)
            })
            .map(|value| value.register)
    }

    fn condition_member_value_is_live(&self, value: &ConditionMemberValue) -> bool {
        self.output.instructions[value.instruction_index..]
            .iter()
            .all(|instruction| {
                !is_memory_or_call_barrier(instruction)
                    && !mwcc_vreg::register_operands(instruction)
                        .iter()
                        .any(|operand| {
                            operand.role == mwcc_vreg::RegisterRole::Define
                                && operand.class == mwcc_vreg::Class::General
                                && operand.register == value.register
                        })
            })
    }
}

fn is_short_circuit_chain(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
            ..
        }
    )
}

pub(crate) fn cacheable_member(expression: &Expression, generator: &Generator) -> bool {
    let Expression::Member {
        base,
        member_type,
        index_stride: None,
        ..
    } = expression
    else {
        return false;
    };
    let Expression::Variable(base) = base.as_ref() else {
        return false;
    };
    !generator.volatile_globals.contains(base) && cacheable_member_type(*member_type)
}

fn cacheable_member_type(member_type: Type) -> bool {
    matches!(
        member_type,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}

pub(crate) fn same_member(left: &Expression, right: &Expression) -> bool {
    let (
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
    ) = (left, right)
    else {
        return false;
    };
    left_offset == right_offset
        && left_type == right_type
        && left_stride == right_stride
        && matches!(
            (left_base.as_ref(), right_base.as_ref()),
            (Expression::Variable(left), Expression::Variable(right)) if left == right
        )
}

fn is_memory_or_call_barrier(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::StoreWord { .. }
            | Instruction::StoreByte { .. }
            | Instruction::StoreHalfword { .. }
            | Instruction::StoreWordWithUpdate { .. }
            | Instruction::StoreByteWithUpdate { .. }
            | Instruction::StoreWordIndexed { .. }
            | Instruction::StoreByteIndexed { .. }
            | Instruction::StoreHalfwordIndexed { .. }
            | Instruction::StoreFloatSingle { .. }
            | Instruction::StoreFloatDouble { .. }
            | Instruction::StoreFloatSingleWithUpdate { .. }
            | Instruction::StoreFloatDoubleWithUpdate { .. }
            | Instruction::StoreFloatSingleIndexed { .. }
            | Instruction::StoreFloatDoubleIndexed { .. }
            | Instruction::PairedSingleQuantizedStore { .. }
            | Instruction::StoreMultipleWord { .. }
            | Instruction::BranchAndLink { .. }
            | Instruction::BranchExternal { .. }
            | Instruction::BranchToCountRegisterAndLink
            | Instruction::BranchToLinkRegisterAndLink
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(name: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(name.into())),
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    fn narrow_member(name: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(name.into())),
            offset,
            member_type: Type::UnsignedChar,
            index_stride: None,
        }
    }

    #[test]
    fn member_identity_includes_base_and_offset() {
        assert!(same_member(&member("fp", 12), &member("fp", 12)));
        assert!(!same_member(&member("fp", 12), &member("fp", 16)));
        assert!(!same_member(&member("fp", 12), &member("other", 12)));
    }

    #[test]
    fn short_circuit_chains_open_the_dominated_edge_cache() {
        let value = member("fp", 12);
        let and = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(value.clone()),
            right: Box::new(value.clone()),
        };
        let or = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(value.clone()),
            right: Box::new(value),
        };
        assert!(is_short_circuit_chain(&and));
        assert!(is_short_circuit_chain(&or));
        assert!(!is_short_circuit_chain(&member("fp", 12)));
    }

    #[test]
    fn narrow_members_are_not_retained_between_short_circuit_terms() {
        assert!(cacheable_member_type(Type::UnsignedInt));
        let Expression::Member { member_type, .. } = narrow_member("fp", 12) else {
            unreachable!()
        };
        assert!(!cacheable_member_type(member_type));
    }
}
