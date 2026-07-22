//! Register planning for short-lived locals in structured non-leaf bodies.
//!
//! Callee-saved homes and ephemeral values are separate lifetime classes. This
//! module computes the latter before emission so the structured body can decline
//! atomically when a local needs a call result, stack storage, or another
//! unmodeled lifetime.

#[allow(unused_imports)]
use super::*;

pub(super) fn plan_ephemeral_locals<'a>(
    function: &'a Function,
    survivors: &std::collections::HashSet<&str>,
) -> Option<Vec<&'a LocalDeclaration>> {
    let mut live: std::collections::HashSet<&str> = function
        .locals
        .iter()
        .filter(|local| body_uses_local(&function.statements, &local.name))
        .map(|local| local.name.as_str())
        .collect();

    // A used initializer can depend on an earlier local. Walk declarations in
    // reverse source order to recover that transitive lifetime without emitting
    // unused, side-effect-free initializers.
    for local in function.locals.iter().rev() {
        if live.contains(local.name.as_str()) {
            if let Some(initializer) = &local.initializer {
                for dependency in &function.locals {
                    if expression_reads_name(initializer, &dependency.name) {
                        live.insert(dependency.name.as_str());
                    }
                }
            }
        }
    }

    let ephemeral: Vec<_> = function
        .locals
        .iter()
        .filter(|local| {
            live.contains(local.name.as_str()) && !survivors.contains(local.name.as_str())
        })
        .collect();
    if ephemeral.iter().any(|local| {
        local.is_static
            || local.is_volatile
            || local.array_length.is_some()
            || local.initializer.is_none()
            || class_of(local.declared_type).ok() != Some(ValueClass::General)
            || expression_has_call(local.initializer.as_ref().expect("checked above"))
    }) {
        return None;
    }
    Some(ephemeral)
}

fn body_uses_local(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || expression_reads_name(value, name),
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            expression_reads_name(expression, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_reads_name(condition, name)
                || body_uses_local(then_body, name)
                || body_uses_local(else_body, name)
        }
        Statement::Switch { .. } | Statement::Loop { .. } => true,
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, initializer: Expression) -> LocalDeclaration {
        LocalDeclaration {
            name: name.into(),
            declared_type: Type::Int,
            initializer: Some(initializer),
            is_static: false,
            is_volatile: false,
            is_const: false,
            array_length: None,
            row_bytes: None,
            data_bytes: None,
            data_relocations: Vec::new(),
        }
    }

    #[test]
    fn retains_transitive_ephemeral_initializer_dependencies() {
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![
                local("first", Expression::IntegerLiteral(3)),
                local("second", Expression::Variable("first".into())),
            ],
            statements: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("second".into())],
            })],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let survivors = std::collections::HashSet::new();
        let planned = plan_ephemeral_locals(&function, &survivors).unwrap();
        assert_eq!(
            planned
                .iter()
                .map(|local| local.name.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }
}
