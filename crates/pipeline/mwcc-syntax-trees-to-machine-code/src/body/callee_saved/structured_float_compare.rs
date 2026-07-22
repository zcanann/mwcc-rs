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
    RetainedFloatCompareValue, StructuredFloatHandoff, FLOAT_SCRATCH,
};

impl Generator {
    /// Preferred home of the single ephemeral float lifetime. A later loaded
    /// comparison occupies build 163's f2 work register, so the initializer is
    /// born there and copied to f1 for its eventual call argument.
    pub(super) fn ephemeral_float_home_preference(&self, function: &Function) -> u8 {
        if self.behavior.preload_ephemeral_float_compare_literal
            && function
                .statements
                .iter()
                .skip(1)
                .any(statement_has_loaded_float_literal_compare)
        {
            2
        } else {
            1
        }
    }

    pub(super) fn plan_structured_float_handoff(&mut self, function: &Function) {
        if self.ephemeral_float_home_preference(function) != 2 {
            return;
        }
        let Some((name, source)) = self.locations.iter().find_map(|(name, location)| {
            (location.class == ValueClass::Float
                && mwcc_vreg::Reg::is_virtual_field(location.register))
            .then(|| (name.clone(), location.register))
        }) else {
            return;
        };
        let destination = self.fresh_virtual_float_preferring(1);
        let initializer = function
            .locals
            .iter()
            .find(|local| local.name == name)
            .and_then(|local| local.initializer.as_ref())
            .expect("structured float handoff local has an initializer")
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

fn same_direct_float_memory_load(left: &Expression, right: &Expression) -> bool {
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
