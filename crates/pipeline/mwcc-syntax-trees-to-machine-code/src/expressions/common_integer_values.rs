//! Expression-scoped common integer computations.
//!
//! Share only register/constant arithmetic inside one eagerly evaluated tree.
//! Each common value gets a virtual home; normal allocation then sees every
//! use. Memory reads, calls, assignments, pointers and short-circuit boundaries
//! are excluded, so this pass never needs cross-statement invalidation rules.

use super::*;

fn computed(expression: &Expression) -> bool {
    matches!(expression, Expression::Binary { operator, .. }
        if !is_comparison(*operator)
            && !matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr))
        || matches!(
            expression,
            Expression::Unary {
                operator: UnaryOperator::Negate | UnaryOperator::BitNot,
                ..
            }
        )
}

fn collect<'a>(expression: &'a Expression, values: &mut Vec<&'a Expression>) {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
            ..
        } => return,
        Expression::Binary { left, right, .. } => {
            collect(left, values);
            collect(right, values);
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            collect(operand, values)
        }
        _ => {}
    }
    if computed(expression) && constant_value(expression).is_none() {
        values.push(expression);
    }
}

/// Search one eager arithmetic scope. A nested comma, call or conditional is
/// lowered in its own value context, without sharing computations across it.
pub(super) fn has_repeated_values(expression: &Expression) -> bool {
    let mut values = Vec::new();
    collect(expression, &mut values);
    values.iter().enumerate().any(|(i, value)| {
        values[i + 1..]
            .iter()
            .any(|other| structurally_equal(value, other))
    })
}

fn substitute(expression: &Expression, bindings: &[(Expression, String)]) -> Expression {
    if let Some((_, name)) = bindings
        .iter()
        .rev()
        .find(|(value, _)| structurally_equal(value, expression))
    {
        return Expression::Variable(name.clone());
    }
    match expression {
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute(left, bindings)),
            right: Box::new(substitute(right, bindings)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(substitute(operand, bindings)),
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(substitute(operand, bindings)),
        },
        other => other.clone(),
    }
}

impl Generator {
    fn common_integer_tree(&self, expression: &Expression) -> bool {
        match expression {
            Expression::IntegerLiteral(value) => {
                i32::try_from(*value).is_ok() || u32::try_from(*value).is_ok()
            }
            Expression::Variable(name) => {
                !self.frame_slots.contains_key(name)
                    && !self.volatile_globals.contains(name)
                    && self.locations.get(name).is_some_and(|location| {
                        location.class == ValueClass::General
                            && location.width <= 32
                            && location.pointee.is_none()
                            && location.stride.is_none()
                    })
            }
            Expression::Binary {
                operator,
                left,
                right,
            } => {
                !matches!(
                    operator,
                    BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                ) && self.common_integer_tree(left)
                    && self.common_integer_tree(right)
            }
            Expression::Unary {
                operator: UnaryOperator::Negate | UnaryOperator::BitNot | UnaryOperator::LogicalNot,
                operand,
            } => self.common_integer_tree(operand),
            Expression::Cast {
                target_type,
                operand,
            } => {
                matches!(
                    target_type,
                    Type::Int
                        | Type::UnsignedInt
                        | Type::Char
                        | Type::UnsignedChar
                        | Type::Short
                        | Type::UnsignedShort
                ) && self.common_integer_tree(operand)
            }
            _ => false,
        }
    }

    pub(crate) fn scoped_integer_value(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                ..
            } => {
                self.common_integer_tree(condition)
                    && self.scoped_integer_value(when_true)
                    && self.scoped_integer_value(when_false)
            }
            _ => self.common_integer_tree(expression),
        }
    }

    pub(crate) fn emit_scoped_integer_pair(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u32,
    ) -> Compilation<()> {
        let mut trial = self.clone();
        let mut names = Vec::new();
        for operand in [left, right] {
            let signed = trial.signedness_of(operand)?;
            let home = trial.fresh_virtual_general();
            let mut name = format!("__mwcc_operand_{home}");
            while trial.locations.contains_key(&name)
                || trial.globals.contains_key(&name)
                || trial.known_locals.contains(&name)
            {
                name.push('_');
            }
            trial.with_reserved_inputs(right, |generator| {
                generator.evaluate_general(operand, home)
            })?;
            trial.locations.insert(
                name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed,
                    width: 32,
                    pointee: None,
                    stride: None,
                },
            );
            trial.reserved.insert(home);
            names.push(name);
        }
        let expression = Expression::Binary {
            operator,
            left: Box::new(Expression::Variable(names[0].clone())),
            right: Box::new(Expression::Variable(names[1].clone())),
        };
        trial.evaluate_general(&expression, destination)?;
        for name in names {
            let location = trial.locations.remove(&name).expect("scoped operand");
            trial.reserved.remove(&location.register);
        }
        *self = trial;
        Ok(())
    }

    pub(crate) fn try_common_integer_values(
        &mut self,
        expression: &Expression,
        destination: u32,
    ) -> Compilation<bool> {
        if !self.common_integer_tree(expression) {
            return Ok(false);
        }
        let mut values = Vec::new();
        collect(expression, &mut values);
        let mut repeated = Vec::new();
        for (index, value) in values.iter().enumerate() {
            if values[index + 1..]
                .iter()
                .any(|other| structurally_equal(value, other))
                && !repeated
                    .iter()
                    .any(|other| structurally_equal(value, other))
            {
                repeated.push((*value).clone());
            }
        }
        if repeated.is_empty() {
            return Ok(false);
        }
        let mut trial = self.clone();
        let mut bindings = Vec::new();
        // Postorder puts shared children before shared parents. Substitution
        // uses the source expression identity, even after a child gains a home.
        for value in repeated {
            let signed = trial.signedness_of(&value)?;
            let (home, name) = loop {
                let home = trial.fresh_virtual_general();
                let name = format!("__mwcc_cse_{home}");
                if !trial.locations.contains_key(&name)
                    && !trial.globals.contains_key(&name)
                    && !trial.known_locals.contains(&name)
                {
                    break (home, name);
                }
            };
            let rewritten = substitute(&value, &bindings);
            // Sharing a child does not make its source inputs dead: the
            // enclosing expression may still use them independently.
            trial.with_reserved_inputs(expression, |generator| {
                generator.evaluate_general(&rewritten, home)
            })?;
            trial.locations.insert(
                name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed,
                    width: 32,
                    pointee: None,
                    stride: None,
                },
            );
            trial.reserved.insert(home);
            bindings.push((value, name));
        }
        let rewritten = substitute(expression, &bindings);
        if let Expression::Binary {
            operator,
            left,
            right,
        } = &rewritten
        {
            if is_comparison(*operator) {
                trial.emit_scoped_integer_pair(*operator, left, right, destination)?;
            } else {
                trial.evaluate_general(&rewritten, destination)?;
            }
        } else {
            trial.evaluate_general(&rewritten, destination)?;
        }
        for (_, name) in bindings {
            let location = trial.locations.remove(&name).expect("scoped common value");
            trial.reserved.remove(&location.register);
        }
        *self = trial;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shifted() -> Expression {
        Expression::Binary {
            operator: BinaryOperator::ShiftRight,
            left: Box::new(Expression::Variable("value".into())),
            right: Box::new(Expression::IntegerLiteral(31)),
        }
    }

    fn pair(operator: BinaryOperator, value: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(value.clone()),
            right: Box::new(value),
        }
    }

    #[test]
    fn shares_nested_arithmetic_but_stops_at_short_circuit_boundaries() {
        let arithmetic = pair(BinaryOperator::Add, shifted());
        assert!(has_repeated_values(&arithmetic));
        assert!(!has_repeated_values(&pair(
            BinaryOperator::LogicalOr,
            shifted()
        )));
        let comma = Expression::Comma {
            left: Box::new(Expression::IntegerLiteral(0)),
            right: Box::new(arithmetic.clone()),
        };
        assert!(!has_repeated_values(&comma));
        let conditional = Expression::Conditional {
            condition: Box::new(Expression::Variable("flag".into())),
            when_true: Box::new(arithmetic),
            when_false: Box::new(Expression::IntegerLiteral(0)),
            origin: mwcc_syntax_trees::ConditionalOrigin::Ternary,
        };
        assert!(!has_repeated_values(&conditional));
    }

    #[test]
    fn substitutes_shared_children_before_parents_without_erasing_casts() {
        let child = shifted();
        let parent = pair(BinaryOperator::Multiply, child.clone());
        let rewritten = substitute(&parent, &[(child, "child".into())]);
        assert!(structurally_equal(
            &rewritten,
            &pair(
                BinaryOperator::Multiply,
                Expression::Variable("child".into())
            )
        ));
        let cast = Expression::Cast {
            target_type: Type::UnsignedShort,
            operand: Box::new(parent.clone()),
        };
        let rewritten = substitute(&cast, &[(parent, "parent".into())]);
        assert!(
            matches!(rewritten, Expression::Cast { target_type: Type::UnsignedShort, operand } if matches!(*operand, Expression::Variable(ref name) if name == "parent"))
        );
    }

    #[test]
    fn constants_do_not_create_runtime_bindings() {
        let constant = pair(BinaryOperator::ShiftLeft, Expression::IntegerLiteral(2));
        assert!(!has_repeated_values(&pair(
            BinaryOperator::Multiply,
            constant
        )));
    }
}
