//! Float memory values retained along side-effect-free condition edges.
//!
//! MWCC can keep a loaded float live within the selected arm of a condition or
//! when an early-return guard falls through into the next condition. This cache
//! records those condition loads until the structured owner has emitted the
//! relevant edge. Calls or mutations make a condition ineligible to feed a
//! later one.

use crate::generator::{float_compare_literal_key, FloatCompareLiteralKey, Generator};
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::Expression;

mod edge;

#[derive(Clone)]
pub(crate) struct ConditionFloatValue {
    expression: Expression,
    register: u8,
    instruction_index: usize,
}

#[derive(Clone, Copy)]
struct ConditionFloatRegisterValue {
    register: u8,
    instruction_index: usize,
}

#[derive(Clone, Copy)]
struct ConditionFloatLiteralValue {
    key: FloatCompareLiteralKey,
    register: u8,
    instruction_index: usize,
}

#[derive(Clone, Default)]
pub(crate) struct ConditionFloatCache {
    active: bool,
    guarded_edge: bool,
    comparison_followup: bool,
    recording_allowed: bool,
    condition: Option<Expression>,
    guarded_followup: Option<Expression>,
    reusable: Vec<ConditionFloatValue>,
    observed: Vec<ConditionFloatValue>,
    /// Direct loads available on the selected edge of this condition.
    ///
    /// Unlike `observed`, this includes frame-local memory. It may only feed an
    /// immediately nested condition: no source statement has run between the
    /// load and that edge, so local-memory aliasing cannot intervene.
    edge_observed: Vec<ConditionFloatValue>,
    intra_condition: Vec<ConditionFloatValue>,
    zero_register: Option<ConditionFloatRegisterValue>,
    literals: Vec<ConditionFloatLiteralValue>,
}

impl Generator {
    pub(crate) fn begin_condition_float_cache(
        &mut self,
        condition: &Expression,
    ) -> ConditionFloatCache {
        let previous = std::mem::take(&mut self.condition_float_cache);
        self.condition_float_cache.active = true;
        self.condition_float_cache.recording_allowed = !expression_has_value_barrier(condition);
        self.condition_float_cache.condition = Some(condition.clone());
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
        self.condition_float_cache.condition = Some(condition.clone());
        self.condition_float_cache.reusable = previous
            .observed
            .into_iter()
            .filter(|value| pure_prefix_contains(condition, &value.expression, &mut false))
            .collect();
    }

    /// Enter a nested condition produced while composing one source-level
    /// side-effect expression. Local-memory values are safe to carry across
    /// this edge because no intervening source statement can mutate them.
    pub(crate) fn begin_composed_condition_float_cache(
        &mut self,
        condition: &Expression,
    ) -> ConditionFloatCache {
        self.begin_composed_condition_float_cache_with_followup(condition, None)
    }

    pub(crate) fn begin_composed_condition_float_cache_with_followup(
        &mut self,
        condition: &Expression,
        guarded_followup: Option<&Expression>,
    ) -> ConditionFloatCache {
        let previous = std::mem::take(&mut self.condition_float_cache);
        self.condition_float_cache.active = true;
        self.condition_float_cache.recording_allowed = !expression_has_value_barrier(condition);
        self.condition_float_cache.condition = Some(condition.clone());
        self.condition_float_cache.guarded_followup = guarded_followup.cloned();
        if previous.active && self.condition_float_cache.recording_allowed {
            self.condition_float_cache.reusable = previous
                .intra_condition
                .iter()
                .filter(|value| pure_prefix_contains(condition, &value.expression, &mut false))
                .cloned()
                .collect();
            self.condition_float_cache.zero_register = previous.zero_register;
            self.condition_float_cache.literals = previous.literals.clone();
        }
        previous
    }

    /// A directly guarded store may consume a comparison operand itself. This
    /// is intentionally distinct from nested-condition retention: legacy MWCC
    /// does not generally carry arbitrary comparison operands into a nested
    /// guard even when the source expressions happen to match.
    pub(crate) fn begin_composed_condition_float_cache_with_value_followup(
        &mut self,
        condition: &Expression,
        guarded_followup: &Expression,
    ) -> ConditionFloatCache {
        let previous = self.begin_composed_condition_float_cache_with_followup(
            condition,
            Some(guarded_followup),
        );
        self.condition_float_cache.comparison_followup = true;
        previous
    }

    pub(crate) fn restore_condition_float_cache(&mut self, previous: ConditionFloatCache) {
        self.condition_float_cache = previous;
    }

    /// Retain only immutable pool literals on a selected condition edge.
    ///
    /// Memory-derived values require alias proof before crossing body
    /// statements. Pool literals do not: their register remains reusable until
    /// an emitted instruction overwrites it or a call clobbers its volatile
    /// FPR.
    pub(crate) fn condition_float_literal_edge_cache(&self) -> ConditionFloatCache {
        ConditionFloatCache {
            active: self.condition_float_cache.active,
            recording_allowed: self.condition_float_cache.recording_allowed,
            literals: self.condition_float_cache.literals.clone(),
            ..ConditionFloatCache::default()
        }
    }

    pub(crate) fn condition_float_register(&mut self, operand: &Expression) -> Option<u8> {
        if self.non_leaf {
            if let Some(value) = self
                .condition_float_cache
                .intra_condition
                .iter()
                .find(|value| same_retained_float_expression(&value.expression, operand))
                .cloned()
            {
                if self.condition_float_value_is_live(&value) {
                    return Some(value.register);
                }
            }
        }
        let index = self
            .condition_float_cache
            .reusable
            .iter()
            .position(|value| same_retained_float_expression(&value.expression, operand))?;
        let value = self.condition_float_cache.reusable.remove(index);
        self.condition_float_value_is_live(&value)
            .then_some(value.register)
    }

    /// Consume a value proven live into the first statement on a condition's
    /// true edge. Keeping this separate from intra-condition reuse prevents a
    /// nested arithmetic evaluator from changing the established comparison
    /// schedule merely because it happens to see the same memory load.
    pub(crate) fn condition_float_guarded_edge_register(
        &mut self,
        operand: &Expression,
    ) -> Option<u8> {
        if !self.condition_float_cache.guarded_edge {
            return None;
        }
        let index = self
            .condition_float_cache
            .intra_condition
            .iter()
            .position(|value| {
                same_retained_float_expression(&value.expression, operand)
            })?;
        let value = self.condition_float_cache.intra_condition.remove(index);
        self.condition_float_value_is_live(&value)
            .then_some(value.register)
    }

    pub(crate) fn record_condition_float_value(&mut self, operand: &Expression, register: u8) {
        if !self.condition_float_cache.active
            || !self.condition_float_cache.recording_allowed
            || !is_direct_float_memory_load(operand)
        {
            return;
        }
        if self.non_leaf && self.condition_repeats_float_value(operand) {
            self.invalidate_condition_float_register(register);
            self.condition_float_cache
                .intra_condition
                .push(ConditionFloatValue {
                    expression: operand.clone(),
                    register,
                    instruction_index: self.output.instructions.len(),
                });
        }

        if self.condition_float_value_is_retained_by_guarded_followup(operand) {
            self.condition_float_cache
                .edge_observed
                .retain(|value| !same_retained_float_expression(&value.expression, operand));
            self.condition_float_cache
                .edge_observed
                .push(ConditionFloatValue {
                    expression: operand.clone(),
                    register,
                    instruction_index: self.output.instructions.len(),
                });
        }

        if
            // MWCC keeps an entry-parameter member live here, but reloads the
            // same shape through a local pointer alias (measured in Melee's
            // CaptureWaitKirby guard). Preserve that alias boundary instead of
            // treating two syntactically equal addresses as proven identical.
            direct_memory_base_name(operand).is_none_or(|name| {
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
                instruction_index: self.output.instructions.len(),
            });
    }

    /// Record a side-effect-free computed compare operand that is consumed
    /// unchanged by the first statement on the condition's true edge.
    pub(crate) fn record_condition_float_computed_value(
        &mut self,
        operand: &Expression,
        register: u8,
    ) {
        if !self.condition_float_cache.active
            || !self.condition_float_cache.recording_allowed
            || !is_retained_float_expression(operand)
            || !self.condition_float_value_is_retained_by_guarded_followup(operand)
        {
            return;
        }
        self.condition_float_cache
            .edge_observed
            .retain(|value| !same_retained_float_expression(&value.expression, operand));
        self.condition_float_cache
            .edge_observed
            .push(ConditionFloatValue {
                expression: operand.clone(),
                register,
                instruction_index: self.output.instructions.len(),
            });
    }

    fn condition_float_value_is_live(&self, value: &ConditionFloatValue) -> bool {
        self.condition_float_register_value_is_live(value.register, value.instruction_index)
    }

    fn condition_float_register_value_is_live(
        &self,
        register: u8,
        instruction_index: usize,
    ) -> bool {
        float_register_value_is_live(
            &self.output.instructions,
            register,
            instruction_index,
        )
    }

    pub(crate) fn invalidate_condition_float_register(&mut self, register: u8) {
        self.condition_float_cache
            .intra_condition
            .retain(|value| value.register != register);
        self.condition_float_cache
            .observed
            .retain(|value| value.register != register);
        self.condition_float_cache
            .edge_observed
            .retain(|value| value.register != register);
        self.condition_float_cache
            .literals
            .retain(|value| value.register != register);
        if self
            .condition_float_cache
            .zero_register
            .is_some_and(|value| value.register == register)
        {
            self.condition_float_cache.zero_register = None;
        }
    }

    pub(crate) fn condition_float_zero_register(&self) -> Option<u8> {
        if !self.condition_float_cache.active
            || !self.non_leaf
            || !self.has_virtual_float_location()
        {
            return None;
        }
        let value = self.condition_float_cache.zero_register?;
        self.condition_float_register_value_is_live(value.register, value.instruction_index)
            .then_some(value.register)
    }

    pub(crate) fn record_condition_float_zero(&mut self, register: u8) {
        if !self.condition_float_cache.active
            || !self.condition_float_cache.recording_allowed
            || !self.non_leaf
            || !self.has_virtual_float_location()
        {
            return;
        }
        self.invalidate_condition_float_register(register);
        self.condition_float_cache.zero_register = Some(ConditionFloatRegisterValue {
            register,
            instruction_index: self.output.instructions.len(),
        });
    }

    pub(crate) fn observed_condition_float_register(
        &self,
        operand: &Expression,
    ) -> Option<u8> {
        self.condition_float_cache
            .observed
            .iter()
            .find(|value| same_direct_float_memory_load(&value.expression, operand))
            .map(|value| value.register)
    }

    pub(crate) fn condition_repeats_float_value(&self, operand: &Expression) -> bool {
        self.condition_float_cache
            .condition
            .as_ref()
            .is_some_and(|condition| direct_float_memory_load_count(condition, operand) > 1)
    }

    pub(crate) fn record_condition_float_literal(
        &mut self,
        operand: &Expression,
        double: bool,
        register: u8,
    ) {
        if !self.condition_float_cache.active
            || !self.condition_float_cache.recording_allowed
        {
            return;
        }
        let Some(key) = float_compare_literal_key(operand, double) else {
            return;
        };
        self.condition_float_cache
            .literals
            .retain(|value| value.key != key);
        self.condition_float_cache
            .literals
            .push(ConditionFloatLiteralValue {
                key,
                register,
                instruction_index: self.output.instructions.len(),
            });
    }

    pub(crate) fn condition_float_literal_register(
        &self,
        operand: &Expression,
        double: bool,
    ) -> Option<u8> {
        let key = float_compare_literal_key(operand, double)?;
        let value = self
            .condition_float_cache
            .literals
            .iter()
            .find(|value| value.key == key)?;
        self.condition_float_register_value_is_live(value.register, value.instruction_index)
            .then_some(value.register)
    }
}

fn float_register_value_is_live(
    instructions: &[Instruction],
    register: u8,
    instruction_index: usize,
) -> bool {
    !instructions[instruction_index..].iter().any(|instruction| {
        instruction.float_destination() == Some(register)
            || (register <= 13
                && matches!(
                    instruction,
                    Instruction::BranchAndLink { .. }
                        | Instruction::BranchToCountRegisterAndLink
                ))
    })
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

fn direct_float_memory_load_count(expression: &Expression, target: &Expression) -> usize {
    if same_direct_float_memory_load(expression, target) {
        return 1;
    }
    match expression {
        Expression::Binary { left, right, .. }
        | Expression::Index {
            base: left,
            index: right,
        }
        | Expression::Comma { left, right } => {
            direct_float_memory_load_count(left, target)
                + direct_float_memory_load_count(right, target)
        }
        Expression::Assign {
            target: left,
            value: right,
        } => {
            direct_float_memory_load_count(left, target)
                + direct_float_memory_load_count(right, target)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            direct_float_memory_load_count(condition, target)
                + direct_float_memory_load_count(when_true, target)
                + direct_float_memory_load_count(when_false, target)
        }
        Expression::Call { arguments, .. }
        | Expression::CallThrough { arguments, .. }
        | Expression::VirtualCall { arguments, .. }
        | Expression::ConstructedNew { arguments, .. }
        | Expression::AggregateLiteral(arguments) => arguments
            .iter()
            .map(|argument| direct_float_memory_load_count(argument, target))
            .sum(),
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
            direct_float_memory_load_count(base, target)
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => 0,
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

pub(crate) fn is_retained_float_expression(expression: &Expression) -> bool {
    is_direct_float_memory_load(expression)
        || matches!(
            expression,
            Expression::Unary {
                operator: mwcc_syntax_trees::UnaryOperator::Negate,
                operand,
            } if is_retained_float_expression(operand)
        )
}

pub(crate) fn same_retained_float_expression(
    left: &Expression,
    right: &Expression,
) -> bool {
    if same_direct_float_memory_load(left, right) {
        return true;
    }
    matches!(
        (left, right),
        (
            Expression::Unary {
                operator: mwcc_syntax_trees::UnaryOperator::Negate,
                operand: left,
            },
            Expression::Unary {
                operator: mwcc_syntax_trees::UnaryOperator::Negate,
                operand: right,
            },
        ) if same_retained_float_expression(left, right)
    )
}

fn same_address_expression(left: &Expression, right: &Expression) -> bool {
    match (left, right) {
        (Expression::Variable(left), Expression::Variable(right)) => left == right,
        (Expression::IntegerLiteral(left), Expression::IntegerLiteral(right)) => left == right,
        _ => same_direct_float_memory_load(left, right),
    }
}

pub(crate) fn expression_has_value_barrier(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. }
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
    if same_retained_float_expression(expression, target) {
        return true;
    }
    match expression {
        Expression::Call { .. }
        | Expression::CallThrough { .. }
        | Expression::VirtualCall { .. }
        | Expression::ConstructedNew { .. }
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
    use mwcc_syntax_trees::{BinaryOperator, Type, UnaryOperator};

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
    fn counts_a_float_load_repeated_across_short_circuit_groups() {
        let target = member(0);
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(target.clone()),
                right: Box::new(Expression::IntegerLiteral(-1)),
            }),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(target.clone()),
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        };

        assert_eq!(direct_float_memory_load_count(&condition, &target), 2);
    }

    #[test]
    fn matches_a_negated_memory_value_as_a_retained_expression() {
        let value = Expression::Unary {
            operator: UnaryOperator::Negate,
            operand: Box::new(member(12)),
        };
        let same = Expression::Unary {
            operator: UnaryOperator::Negate,
            operand: Box::new(member(12)),
        };

        assert!(is_retained_float_expression(&value));
        assert!(same_retained_float_expression(&value, &same));
        assert!(pure_prefix_contains(&same, &value, &mut false));
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

    #[test]
    fn calls_kill_volatile_but_not_nonvolatile_float_literals() {
        let instructions = vec![Instruction::BranchAndLink {
            target: "consume".into(),
        }];
        assert!(!float_register_value_is_live(&instructions, 2, 0));
        assert!(float_register_value_is_live(&instructions, 31, 0));
    }

    #[test]
    fn an_explicit_float_write_kills_a_retained_literal() {
        let instructions = vec![Instruction::FloatMove { d: 2, b: 1 }];
        assert!(!float_register_value_is_live(&instructions, 2, 0));
        assert!(float_register_value_is_live(&instructions, 3, 0));
    }
}
