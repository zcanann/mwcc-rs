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

    fn strip_artificial_leaf_linkage(&mut self) -> Compilation<()> {
        let len = self.output.instructions.len();
        let valid = matches!(
            self.output.instructions.as_slice(),
            [
                Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                Instruction::MoveFromLinkRegister { d: 0 },
                Instruction::StoreWord { s: 0, a: 1, .. },
                ..,
                Instruction::LoadWord { d: 0, a: 1, .. },
                Instruction::MoveToLinkRegister { s: 0 },
                Instruction::AddImmediate { d: 1, a: 1, .. },
                Instruction::BranchToLinkRegister,
            ]
        );
        if !valid || len < 7 {
            return Err(Diagnostic::error(
                "materialized float leaf has an unexpected linkage frame",
            ));
        }

        crate::remove_instruction_retargeting_to_next(self, len - 3);
        crate::remove_instruction_retargeting_to_next(self, len - 4);
        crate::remove_instruction_retargeting_to_next(self, 2);
        crate::remove_instruction_retargeting_to_next(self, 1);
        Ok(())
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
