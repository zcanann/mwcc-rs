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
        let demand = u8::try_from(crate::analysis::register_need(value)).unwrap_or(14);
        let top = highest_input.saturating_add(demand);
        if demand < 2 || top > 13 {
            return self.evaluate_register_store_value(value, value_type, destination);
        }
        let previous = self.materialized_float_window.replace((top, demand));
        let result = self.evaluate_register_store_value(value, value_type, destination);
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
}
