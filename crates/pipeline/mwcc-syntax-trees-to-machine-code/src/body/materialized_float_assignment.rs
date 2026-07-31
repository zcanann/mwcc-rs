//! Straight-line floating assignments whose computed locals must stay materialized.

use super::*;

fn assigned_float_local<'a>(function: &'a Function, name: &str) -> Option<&'a LocalDeclaration> {
    function.locals.iter().find(|local| {
        local.name == name
            && local.initializer.is_none()
            && local.array_length.is_none()
            && matches!(local.declared_type, Type::Float | Type::Double)
    })
}

pub(crate) fn materialized_float_assignment_names<'a>(
    function: &'a Function,
) -> std::collections::HashSet<&'a str> {
    function
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Assign { name, .. } if assigned_float_local(function, name).is_some() => {
                Some(name.as_str())
            }
            _ => None,
        })
        .collect()
}

fn has_computed_float_assignment(function: &Function) -> bool {
    function.statements.iter().any(|statement| {
        let Statement::Assign { name, value } = statement else {
            return false;
        };
        assigned_float_local(function, name).is_some()
            && !matches!(value, Expression::Variable(_) | Expression::FloatLiteral(_))
    })
}

fn is_contextual_float_literal(expression: &Expression) -> bool {
    match expression {
        Expression::FloatLiteral(_) => true,
        Expression::Cast {
            target_type: Type::Float | Type::Double,
            operand,
        } => matches!(operand.as_ref(), Expression::FloatLiteral(_)),
        _ => false,
    }
}

/// Count the persistent temporaries selected by the materialized-float
/// scheduler.  This differs from ordinary Sethi-Ullman pressure when MWCC
/// deliberately loads a literal before its complex partner: that literal owns
/// one additional descending window home for the partner's entire evaluation.
fn materialized_float_temporary_count(expression: &Expression) -> u32 {
    match expression {
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let left_computed = is_complex(left);
            let right_computed = is_complex(right);
            let retained_literal = (is_contextual_float_literal(left) && right_computed)
                || (is_contextual_float_literal(right) && left_computed);
            // Additive parents retain a completed subtree while evaluating its
            // sibling. Products instead reuse the f2 operand lane internally;
            // their final destination is owned by the additive parent and is
            // already represented by ordinary expression pressure.
            let retained_subtree = left_computed
                && right_computed
                && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract);
            u32::from(retained_literal || retained_subtree)
                + materialized_float_temporary_count(left)
                + materialized_float_temporary_count(right)
        }
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
            materialized_float_temporary_count(operand)
        }
        Expression::Conditional {
            when_true,
            when_false,
            ..
        } => materialized_float_temporary_count(when_true)
            .max(materialized_float_temporary_count(when_false)),
        _ => 0,
    }
}

fn is_float_assignment_statement(function: &Function, statement: &Statement) -> bool {
    match statement {
        Statement::Assign { name, .. } => assigned_float_local(function, name).is_some(),
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            !then_body.is_empty()
                && then_body
                    .iter()
                    .chain(else_body)
                    .all(|statement| is_float_assignment_statement(function, statement))
        }
        _ => false,
    }
}

impl Generator {
    fn next_general_parameter_home(&self) -> Option<u8> {
        self.locations
            .values()
            .filter(|location| location.class == ValueClass::General)
            .filter_map(|location| {
                match mwcc_vreg::Reg::from_field(
                    location.register,
                    mwcc_vreg::Class::General,
                ) {
                    mwcc_vreg::Reg::Physical(register) => Some(register),
                    mwcc_vreg::Reg::Virtual(_) => None,
                }
            })
            .max()
            .and_then(|register| register.checked_add(1))
            .filter(|register| *register <= 12)
    }

    /// A loaded value paired with a computed subtree needs one reusable lane
    /// between the retained subtree homes and f0/f1. Ordinary register pressure
    /// does not see that lane because the load itself is a leaf.
    fn materialized_float_has_located_computed_pair(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Binary { left, right, .. } => {
                (self.is_float_located(left) && is_complex(right))
                    || (is_complex(left) && self.is_float_located(right))
                    || self.materialized_float_has_located_computed_pair(left)
                    || self.materialized_float_has_located_computed_pair(right)
            }
            Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
                self.materialized_float_has_located_computed_pair(operand)
            }
            Expression::Conditional {
                when_true,
                when_false,
                ..
            } => {
                self.materialized_float_has_located_computed_pair(when_true)
                    || self.materialized_float_has_located_computed_pair(when_false)
            }
            _ => false,
        }
    }

    /// Evaluate one call-free float assignment inside MWCC's descending
    /// volatile-FPR expression window. Incoming float leaves occupy the low
    /// homes; persistent subtree results are assigned above them from the
    /// outside of the expression tree inward.
    pub(crate) fn evaluate_materialized_float_assignment_value(
        &mut self,
        value: &Expression,
        value_type: Type,
        destination: u8,
    ) -> Compilation<()> {
        if self.behavior.optimization == mwcc_versions::Optimization::O0
            && self.structured_constant_address_home.is_none()
        {
            self.structured_constant_address_home = self.next_general_parameter_home();
        }
        let highest_input = self
            .locations
            .iter()
            .filter(|(name, location)| {
                location.class == ValueClass::Float
                    && crate::analysis::expression_reads_name(value, name)
            })
            .filter_map(|(_, location)| {
                match mwcc_vreg::Reg::from_field(location.register, mwcc_vreg::Class::Float) {
                    mwcc_vreg::Reg::Physical(register) if register <= 13 => Some(register),
                    _ => None,
                }
            })
            .max()
            .unwrap_or(0);
        let demand = crate::analysis::register_need(value)
            .max(materialized_float_temporary_count(value))
            .saturating_add(u32::from(
                self.materialized_float_has_located_computed_pair(value),
            ));
        let demand = u8::try_from(demand).unwrap_or(14);
        let top = highest_input.saturating_add(demand);
        if demand < 2 || top > 13 {
            return self.evaluate_register_store_value(value, value_type, destination);
        }
        let previous = self.materialized_float_window.replace((top, demand));
        let previous_active = self.materialized_float_assignment_active;
        self.materialized_float_assignment_active = true;
        let result = self.evaluate_register_store_value(value, value_type, destination);
        self.materialized_float_assignment_active = previous_active;
        self.materialized_float_window = previous;
        result
    }

    pub(crate) fn materialized_float_window_active(&self) -> bool {
        self.materialized_float_window.is_some()
    }

    pub(crate) fn fresh_materialized_float_temporary(&mut self) -> u8 {
        let Some((preferred, remaining)) = self.materialized_float_window else {
            return self.fresh_virtual_float();
        };
        self.materialized_float_window = if remaining > 1 {
            Some((preferred.saturating_sub(1), remaining - 1))
        } else {
            None
        };
        self.fresh_virtual_float_preferring(preferred)
    }

    /// Route a call-free computed-float body through the structured
    /// virtual-register allocator after copy propagation declines it.
    pub(crate) fn try_materialized_float_assignment_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function_makes_call(function)
            || !function.guards.is_empty()
            || function.statements.is_empty()
            || !function
                .statements
                .iter()
                .all(|statement| is_float_assignment_statement(function, statement))
            || !matches!(
                function.return_expression.as_ref(),
                Some(Expression::Variable(name))
                    if assigned_float_local(function, name).is_some()
            )
            || !has_computed_float_assignment(function)
        {
            return Ok(false);
        }

        let claimed = self.try_callee_saved_structured_body(function)?;
        if claimed {
            self.strip_artificial_leaf_linkage()?;
        }
        Ok(claimed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }

    fn binary(operator: BinaryOperator, left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn recognizes_a_single_computed_float_assignment() {
        let function = Function {
            return_type: Type::Float,
            name: "polynomial".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Float,
                name: "temporary".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements: vec![Statement::Assign {
                name: "temporary".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Subtract,
                    left: Box::new(Expression::FloatLiteral(1.0)),
                    right: Box::new(Expression::FloatLiteral(0.5)),
                },
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("temporary".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        assert!(has_computed_float_assignment(&function));
    }

    #[test]
    fn counts_preloaded_literals_in_the_descending_temporary_window() {
        let multiply = |left, right| binary(BinaryOperator::Multiply, left, right);
        let add = |left, right| binary(BinaryOperator::Add, left, right);
        let subtract = |left, right| binary(BinaryOperator::Subtract, left, right);
        let expression = multiply(
            Expression::FloatLiteral(2.0),
            add(
                multiply(variable("arg8"), variable("argB")),
                add(
                    multiply(
                        subtract(variable("arg8"), Expression::FloatLiteral(1.0)),
                        variable("arg9"),
                    ),
                    multiply(
                        subtract(
                            Expression::FloatLiteral(1.0),
                            multiply(Expression::FloatLiteral(2.0), variable("arg8")),
                        ),
                        variable("argA"),
                    ),
                ),
            ),
        );

        assert_eq!(crate::analysis::register_need(&expression), 3);
        assert_eq!(materialized_float_temporary_count(&expression), 4);
    }

    #[test]
    fn product_operand_lanes_do_not_expand_the_persistent_window() {
        let subtract = |left, right| binary(BinaryOperator::Subtract, left, right);
        let multiply = |left, right| binary(BinaryOperator::Multiply, left, right);
        let add = |left, right| binary(BinaryOperator::Add, left, right);
        let squared = |left: &str, right: &str| {
            multiply(
                subtract(variable(left), variable(right)),
                subtract(variable(left), variable(right)),
            )
        };
        let expression = add(
            squared("az", "bz"),
            add(squared("ax", "bx"), squared("ay", "by")),
        );

        assert_eq!(crate::analysis::register_need(&expression), 4);
        assert_eq!(materialized_float_temporary_count(&expression), 2);
    }
}
