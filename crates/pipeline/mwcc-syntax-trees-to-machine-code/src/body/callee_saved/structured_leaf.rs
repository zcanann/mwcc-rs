//! Frame-free entry point for the shared structured-control-flow lowerer.
//!
//! Structured lowering originally lived behind the callee-saved frame owner,
//! even though its branch graph is equally useful for leaf functions. This
//! adapter owns no prologue or allocation policy: it admits only frame-free
//! bodies and delegates their nested regions to the common emitter.

#[allow(unused_imports)]
use super::*;
use super::structured::structured_hidden_label_count;

impl Generator {
    pub(crate) fn try_leaf_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function_makes_call(function)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !leaf_return_shape_is_supported(function)
            || !contains_nested_or_else_if(&function.statements)
            || !supports_leaf_structured_statements(&function.statements)
            || function.locals.iter().any(|local| {
                local.is_static
                    || local.array_length.is_some()
                    || !matches!(
                        class_of(local.declared_type),
                        Ok(ValueClass::General | ValueClass::Float)
                    )
            })
        {
            return Ok(false);
        }

        for local in &function.locals {
            let class = class_of(local.declared_type).expect("eligibility checked");
            let home = match class {
                ValueClass::General => self.fresh_virtual_general_preferring(4),
                ValueClass::Float => self.fresh_virtual_float_preferring(1),
            };
            if let Some(initializer) = &local.initializer {
                self.evaluate(initializer, local.declared_type, home)?;
            }
            self.locations.insert(
                local.name.clone(),
                Location {
                    class,
                    register: home,
                    signed: self.signed_of(local.declared_type),
                    width: local.declared_type.width(),
                    pointee: match local.declared_type {
                        Type::Pointer(pointee) => Some(pointee),
                        _ => None,
                    },
                    stride: pointer_stride(local.declared_type),
                },
            );
        }

        let mut return_branches = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut pending_gotos = Vec::new();
        self.emit_structured_statements(
            &function.statements,
            function,
            &[],
            false,
            &mut return_branches,
            &mut label_positions,
            &mut pending_gotos,
            &mut None,
        )?;
        debug_assert!(pending_gotos.is_empty());
        if let Some(return_expression) = &function.return_expression {
            let result = match function.return_type {
                Type::Float | Type::Double => Eabi::float_result().number,
                _ => Eabi::general_result().number,
            };
            self.evaluate(return_expression, function.return_type, result)?;
        }
        let epilogue = self.output.instructions.len();
        for branch in return_branches {
            if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                *target = epilogue;
            }
        }
        self.output.anonymous_label_bump += structured_hidden_label_count(&function.statements);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn contains_nested_or_else_if(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            !else_body.is_empty()
                || then_body.iter().any(|inner| matches!(inner, Statement::If { .. }))
                || contains_nested_or_else_if(then_body)
                || contains_nested_or_else_if(else_body)
        }
        _ => false,
    })
}

fn supports_leaf_structured_statements(statements: &[Statement]) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { .. } | Statement::Store { .. } | Statement::Return(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            supports_leaf_structured_statements(then_body)
                && supports_leaf_structured_statements(else_body)
        }
        _ => false,
    })
}

fn leaf_return_shape_is_supported(function: &Function) -> bool {
    (function.return_type == Type::Void && function.return_expression.is_none())
        || (matches!(
            function.return_type,
            Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Int
                | Type::UnsignedInt
                | Type::Pointer(_)
                | Type::StructPointer { .. }
                | Type::Float
                | Type::Double
        ) && (function.return_expression.is_some()
            || !leaf_statements_fall_through(&function.statements)))
}

/// Whether execution can reach the implicit tail after this structured list.
/// Eligibility excludes loops and gotos, so returns and complete if/else
/// diamonds are the only terminating edges that need modeling here.
fn leaf_statements_fall_through(statements: &[Statement]) -> bool {
    for statement in statements {
        match statement {
            Statement::Return(_) => return false,
            Statement::If {
                then_body,
                else_body,
                ..
            } if !else_body.is_empty()
                && !leaf_statements_fall_through(then_body)
                && !leaf_statements_fall_through(else_body) =>
            {
                return false;
            }
            _ => {}
        }
    }
    true
}
