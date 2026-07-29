//! Early pool-literal placement for legacy structured float conditions.
//!
//! Build 163 can schedule the constant from a leading `local < literal`
//! comparison before the independent memory load that initializes `local`.
//! This module owns the narrow look-ahead and leaves ordinary comparison
//! lowering unaware of source declaration order.

#[allow(unused_imports)]
use super::*;
use crate::condition_float_cache::{
    is_direct_float_memory_load, same_direct_float_memory_load,
};
use crate::generator::{
    float_compare_literal_key, FloatCompareLiteralKey, PreloadedFloatCompareLiteral,
    RetainedFloatCompareValue, StructuredFloatHandoff, FLOAT_SCRATCH,
};
use mwcc_syntax_trees::ArmBody;

impl Generator {
    pub(super) fn preload_condition_literal_reused_in_body(
        &mut self,
        condition: &Expression,
        body: &[Statement],
    ) {
        let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        else {
            return;
        };
        if !matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::Greater
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterEqual
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
        ) {
            return;
        }
        let (literal, value) = if matches!(
            right.as_ref(),
            Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
        ) {
            (right.as_ref(), left.as_ref())
        } else if matches!(
            left.as_ref(),
            Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
        ) {
            (left.as_ref(), right.as_ref())
        } else {
            return;
        };
        let double = self.is_double_value(value);
        let Some(key) = float_compare_literal_key(literal, double) else {
            return;
        };
        if !body_stores_float_literal(body, key)
            || self
                .preloaded_float_compare_literals
                .iter()
                .any(|preload| preload.key == key)
        {
            return;
        }

        let register = self.fresh_virtual_float_preferring(2);
        match key {
            FloatCompareLiteralKey::Single(bits) => {
                self.load_float_constant(register, f32::from_bits(bits));
            }
            FloatCompareLiteralKey::Double(bits) => {
                self.load_double_constant(register, bits);
            }
        }
        self.preloaded_float_compare_literals
            .push(PreloadedFloatCompareLiteral {
                key,
                register,
                remaining_uses: 1,
                reuse_for_following_value: false,
            });
    }

    /// Preferred home of the single ephemeral float lifetime. A later loaded
    /// comparison occupies build 163's f2 work register, so the initializer is
    /// born there and copied to f1 for its eventual call argument.
    pub(super) fn ephemeral_float_home_preference(
        &self,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
    ) -> u8 {
        if self.structured_float_handoff_local(function, ephemeral_locals).is_some() {
            2
        } else {
            1
        }
    }

    fn structured_float_handoff_local<'a>(
        &self,
        function: &Function,
        ephemeral_locals: &'a [&LocalDeclaration],
    ) -> Option<&'a LocalDeclaration> {
        if !self.behavior.preload_ephemeral_float_compare_literal
            || !function
                .statements
                .iter()
                .skip(1)
                .any(statement_has_loaded_float_literal_compare)
        {
            return None;
        }
        let [local] = ephemeral_locals else {
            return None;
        };
        (matches!(local.declared_type, Type::Float | Type::Double)
            && local
                .initializer
                .as_ref()
                .is_some_and(is_direct_float_memory_load))
        .then_some(*local)
    }

    pub(super) fn plan_structured_float_handoff(
        &mut self,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
    ) {
        let Some(local) = self.structured_float_handoff_local(function, ephemeral_locals) else {
            return;
        };
        let name = local.name.clone();
        let source = self
            .locations
            .get(&name)
            .expect("ephemeral handoff local was just placed")
            .register;
        let destination = self.fresh_virtual_float_preferring(1);
        let initializer = local
            .initializer
            .as_ref()
            .expect("handoff eligibility requires an initializer")
            .clone();
        self.retained_float_compare_value = Some(RetainedFloatCompareValue {
            expression: initializer,
            register: source,
        });
        self.structured_float_handoff = Some(StructuredFloatHandoff {
            name,
            source,
            destination,
            emitted: false,
        });
    }

    pub(crate) fn emit_structured_float_handoff_before_compare(&mut self) {
        let Some(handoff) = self.structured_float_handoff.as_mut() else {
            return;
        };
        if handoff.emitted {
            return;
        }
        self.output.instructions.push(Instruction::FloatMove {
            d: handoff.destination,
            b: handoff.source,
        });
        handoff.emitted = true;
    }

    pub(crate) fn retained_float_compare_register(
        &self,
        operand: &Expression,
    ) -> Option<u8> {
        self.retained_float_compare_value.as_ref().and_then(|retained| {
            same_direct_float_memory_load(&retained.expression, operand)
                .then_some(retained.register)
        })
    }

    pub(super) fn commit_structured_float_handoff(&mut self) {
        let Some(handoff) = self.structured_float_handoff.take() else {
            return;
        };
        if handoff.emitted {
            if let Some(constant_index) = self.output.constants.len().checked_sub(1) {
                // The alias-splitting optimizer node sits between this first
                // comparison literal and anonymous pool numbering.
                self.output.constant_number_gaps.push((constant_index, 1));
            }
            self.locations
                .get_mut(&handoff.name)
                .expect("handoff local remains live")
                .register = handoff.destination;
        }
    }

    pub(super) fn try_preload_ephemeral_float_compare_literal(
        &mut self,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
    ) -> Compilation<()> {
        if !self.behavior.preload_ephemeral_float_compare_literal {
            return Ok(());
        }

        // Restrict the schedule to one lifetime: with additional ephemeral
        // initializers, proving that f0 remains untouched needs a full local
        // dependency schedule rather than this focused look-ahead.
        let [local] = ephemeral_locals else {
            return Ok(());
        };
        // A later member-vs-literal guard needs its own overlapping FPR. In
        // that lifetime shape build 163 keeps the initializer in f2, copies it
        // to f1, and leaves the first literal beside the first comparison.
        if function
            .statements
            .iter()
            .skip(1)
            .any(statement_has_loaded_float_literal_compare)
        {
            return Ok(());
        }
        if !matches!(local.declared_type, Type::Float | Type::Double)
            || !local
                .initializer
                .as_ref()
                .is_some_and(is_direct_float_memory_load)
        {
            return Ok(());
        }

        let Some(Statement::If { condition, .. }) = function.statements.first() else {
            return Ok(());
        };
        let Expression::Binary {
            operator,
            left,
            right,
        } = condition
        else {
            return Ok(());
        };
        if !matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::Greater
                | BinaryOperator::LessEqual
                | BinaryOperator::GreaterEqual
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
        ) {
            return Ok(());
        }
        let literal = match (left.as_ref(), right.as_ref()) {
            (Expression::Variable(name), literal) | (literal, Expression::Variable(name))
                if name == &local.name
                    && matches!(
                        literal,
                        Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
                    ) => literal,
            _ => return Ok(()),
        };
        let double = local.declared_type == Type::Double;
        let Some(key) = float_compare_literal_key(literal, double) else {
            return Ok(());
        };

        match key {
            FloatCompareLiteralKey::Single(bits) => {
                self.load_float_constant(FLOAT_SCRATCH, f32::from_bits(bits));
            }
            FloatCompareLiteralKey::Double(bits) => {
                self.load_double_constant(FLOAT_SCRATCH, bits);
            }
        }
        let constant_index = self
            .output
            .constants
            .len()
            .checked_sub(1)
            .expect("a preload always interns a pool constant");
        // Build 163 retains one optimizer node between the structured-body
        // label block and this early-created literal.
        self.output.constant_number_gaps.push((constant_index, 1));
        self.preloaded_float_compare_literals
            .push(PreloadedFloatCompareLiteral {
                key,
                register: FLOAT_SCRATCH,
                remaining_uses: 1,
                reuse_for_following_value: false,
            });
        Ok(())
    }
}

fn body_stores_float_literal(statements: &[Statement], key: FloatCompareLiteralKey) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Store { target, value } => {
            let double = match target {
                Expression::Member {
                    member_type: Type::Float,
                    ..
                } => false,
                Expression::Member {
                    member_type: Type::Double,
                    ..
                } => true,
                _ => return false,
            };
            float_compare_literal_key(value, double) == Some(key)
        }
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            body_stores_float_literal(then_body, key)
                || body_stores_float_literal(else_body, key)
        }
        Statement::Loop { body, .. } => body_stores_float_literal(body, key),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| match &arm.body {
                ArmBody::Statements(body) => body_stores_float_literal(body, key),
                ArmBody::Return(_) => false,
            }) || default.as_ref().is_some_and(|body| match body {
                ArmBody::Statements(body) => body_stores_float_literal(body, key),
                ArmBody::Return(_) => false,
            })
        }
        _ => false,
    })
}

fn statement_has_loaded_float_literal_compare(statement: &Statement) -> bool {
    let Statement::If { condition, .. } = statement else {
        return false;
    };
    expression_has_loaded_float_literal_compare(condition)
}

fn expression_has_loaded_float_literal_compare(expression: &Expression) -> bool {
    let Expression::Binary {
        operator,
        left,
        right,
    } = expression
    else {
        return false;
    };
    if matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr) {
        return expression_has_loaded_float_literal_compare(left)
            || expression_has_loaded_float_literal_compare(right);
    }
    (is_direct_float_memory_load(left)
        && matches!(
            right.as_ref(),
            Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
        ))
        || (is_direct_float_memory_load(right)
            && matches!(
                left.as_ref(),
                Expression::FloatLiteral(_) | Expression::IntegerLiteral(_)
            ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member_store(member_type: Type) -> Statement {
        Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset: 8,
                member_type,
                index_stride: None,
            },
            value: Expression::FloatLiteral(1.0),
        }
    }

    #[test]
    fn finds_a_float_literal_store_in_a_nested_arm() {
        let body = vec![Statement::If {
            condition: Expression::Variable("selected".into()),
            then_body: vec![member_store(Type::Float)],
            else_body: Vec::new(),
        }];
        assert!(body_stores_float_literal(
            &body,
            FloatCompareLiteralKey::Single(1.0f32.to_bits())
        ));
    }

    #[test]
    fn does_not_treat_an_integer_store_as_float_literal_reuse() {
        assert!(!body_stores_float_literal(
            &[member_store(Type::Int)],
            FloatCompareLiteralKey::Single(1.0f32.to_bits())
        ));
    }
}
