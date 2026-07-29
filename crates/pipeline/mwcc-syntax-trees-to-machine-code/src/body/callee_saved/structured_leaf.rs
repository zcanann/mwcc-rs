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
            || !requires_structured_branch_graph(&function.statements)
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
        self.retain_guarded_nested_member_base();
        self.reuse_guarded_narrow_member_update();
        self.schedule_volatile_bitset_hint_tail();
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

fn requires_structured_branch_graph(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            then_body.len() > 1
                || !else_body.is_empty()
                || then_body.iter().any(|inner| matches!(inner, Statement::If { .. }))
                || requires_structured_branch_graph(then_body)
                || requires_structured_branch_graph(else_body)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multi_statement_guard_uses_the_structured_branch_graph() {
        let statements = vec![Statement::If {
            condition: Expression::Variable("enabled".into()),
            then_body: vec![
                Statement::Store {
                    target: Expression::Variable("first".into()),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Store {
                    target: Expression::Variable("second".into()),
                    value: Expression::IntegerLiteral(2),
                },
            ],
            else_body: Vec::new(),
        }];
        assert!(requires_structured_branch_graph(&statements));
    }
}
