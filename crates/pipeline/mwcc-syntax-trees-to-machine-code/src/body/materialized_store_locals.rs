//! Register-backed pointer locals feeding a straight run of stores.
//!
//! Copy propagation must not duplicate a memory-loaded pointer used by many
//! stores. Materialize that value once in a virtual register, while folding
//! address-only aliases into their consumers so member displacements remain
//! available to ordinary load/store selection.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn try_materialized_store_pointer_locals(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.locals.is_empty()
            || function_makes_call(function)
            || !function.guards.is_empty()
            || function
                .locals
                .iter()
                .any(|local| {
                    local.is_static
                        || local.array_length.is_some()
                        || local.initializer.is_none()
                        || !matches!(
                            local.declared_type,
                            Type::Pointer(_) | Type::StructPointer { .. }
                        )
                })
            || !function
                .statements
                .iter()
                .all(|statement| matches!(statement, Statement::Store { .. }))
            || !has_supported_result(function)
        {
            return Ok(false);
        }

        let mut aliases = std::collections::HashMap::new();
        let mut materialized = 0usize;
        for local in &function.locals {
            let initializer = crate::value_tracking::substitute(
                local.initializer.as_ref().expect("eligibility checked"),
                &aliases,
            );
            if matches!(initializer, Expression::AddressOf { .. }) {
                aliases.insert(local.name.clone(), initializer);
                continue;
            }

            let first_home: u8 = if function.return_type == Type::Void { 3 } else { 4 };
            let preferred =
                first_home.saturating_add(u8::try_from(materialized).unwrap_or(8));
            let home = self.fresh_virtual_general_preferring(preferred);
            self.evaluate(&initializer, local.declared_type, home)?;
            self.locations.insert(
                local.name.clone(),
                Location {
                    class: ValueClass::General,
                    register: home,
                    signed: false,
                    width: 32,
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
            materialized += 1;
        }
        if materialized == 0 {
            return Ok(false);
        }

        for statement in &function.statements {
            let statement = substitute_statement(statement, &aliases);
            self.emit_statement(&statement)?;
        }
        if let Some(returned) = &function.return_expression {
            let result = Eabi::general_result().number;
            let returned = crate::value_tracking::substitute(returned, &aliases);
            self.evaluate(&returned, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn has_supported_result(function: &Function) -> bool {
    match function.return_type {
        Type::Void => function.return_expression.is_none(),
        Type::Pointer(_) | Type::StructPointer { .. } => function.return_expression.is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(return_type: Type, return_expression: Option<Expression>) -> Function {
        Function {
            return_type,
            name: "stores".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn accepts_void_and_pointer_store_runs_with_consistent_results() {
        assert!(has_supported_result(&function(Type::Void, None)));
        assert!(has_supported_result(&function(
            Type::Pointer(mwcc_syntax_trees::Pointee::Int),
            Some(Expression::Variable("result".into())),
        )));
        assert!(!has_supported_result(&function(
            Type::Void,
            Some(Expression::IntegerLiteral(0)),
        )));
    }
}
