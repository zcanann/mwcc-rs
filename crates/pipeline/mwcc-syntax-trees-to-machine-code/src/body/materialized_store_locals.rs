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
            || !matches!(
                function.return_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.return_expression.is_none()
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

            let preferred = 4u8.saturating_add(u8::try_from(materialized).unwrap_or(8));
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
        let result = Eabi::general_result().number;
        let returned = crate::value_tracking::substitute(
            function.return_expression.as_ref().expect("eligibility checked"),
            &aliases,
        );
        self.evaluate(&returned, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
