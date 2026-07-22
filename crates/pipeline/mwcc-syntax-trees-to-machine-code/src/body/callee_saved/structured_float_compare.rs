//! Early pool-literal placement for legacy structured float conditions.
//!
//! Build 163 can schedule the constant from a leading `local < literal`
//! comparison before the independent memory load that initializes `local`.
//! This module owns the narrow look-ahead and leaves ordinary comparison
//! lowering unaware of source declaration order.

#[allow(unused_imports)]
use super::*;
use crate::generator::{
    float_compare_literal_key, FloatCompareLiteralKey, PreloadedFloatCompareLiteral,
    FLOAT_SCRATCH,
};

impl Generator {
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
        self.preloaded_float_compare_literal = Some(PreloadedFloatCompareLiteral {
            key,
            register: FLOAT_SCRATCH,
        });
        Ok(())
    }
}

fn is_direct_float_memory_load(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Member {
            member_type: Type::Float | Type::Double,
            ..
        } | Expression::Dereference { .. }
            | Expression::Index { .. }
    )
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
