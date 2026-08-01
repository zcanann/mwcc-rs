//! Source proof for narrow locals that always contain full-word booleans.

#[allow(unused_imports)]
use super::*;

pub(crate) fn source_proven_canonical_boolean_locals(
    function: &Function,
) -> std::collections::HashSet<String> {
    let address_taken = crate::frame::collect_address_taken(function);
    function
        .locals
        .iter()
        .filter(|local| {
            local.declared_type == Type::UnsignedChar
                && local.array_length.is_none()
                && !local.is_static
                && !address_taken.contains(local.name.as_str())
        })
        .filter(|local| {
            let mut saw_assignment = false;
            if let Some(initializer) = &local.initializer {
                if !is_boolean_constant(initializer) {
                    return false;
                }
                saw_assignment = true;
            }
            statements_assign_only_booleans(
                &function.statements,
                &local.name,
                &mut saw_assignment,
            ) && function.return_expression.as_ref().is_none_or(|value| {
                !crate::analysis::expression_assigns_name(value, &local.name)
            }) && saw_assignment
        })
        .map(|local| local.name.clone())
        .collect()
}

fn statements_assign_only_booleans(
    statements: &[Statement],
    name: &str,
    saw_assignment: &mut bool,
) -> bool {
    statements
        .iter()
        .all(|statement| statement_assigns_only_booleans(statement, name, saw_assignment))
}

fn statement_assigns_only_booleans(
    statement: &Statement,
    name: &str,
    saw_assignment: &mut bool,
) -> bool {
    match statement {
        Statement::Assign {
            name: assigned,
            value,
        } if assigned == name => {
            *saw_assignment = true;
            is_boolean_constant(value)
                && !crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Assign { value, .. }
        | Statement::Expression(value)
        | Statement::Return(Some(value)) => {
            !crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Store { target, value } => {
            !crate::analysis::expression_assigns_name(target, name)
                && !crate::analysis::expression_assigns_name(value, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            !crate::analysis::expression_assigns_name(condition, name)
                && statements_assign_only_booleans(then_body, name, saw_assignment)
                && statements_assign_only_booleans(else_body, name, saw_assignment)
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .as_ref()
                .is_none_or(|value| !crate::analysis::expression_assigns_name(value, name))
                && condition
                    .as_ref()
                    .is_none_or(|value| !crate::analysis::expression_assigns_name(value, name))
                && step
                    .as_ref()
                    .is_none_or(|value| !crate::analysis::expression_assigns_name(value, name))
                && statements_assign_only_booleans(body, name, saw_assignment)
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            let arm_is_safe = |body: &mwcc_syntax_trees::ArmBody,
                               saw_assignment: &mut bool| match body {
                mwcc_syntax_trees::ArmBody::Return(value) => {
                    !crate::analysis::expression_assigns_name(value, name)
                }
                mwcc_syntax_trees::ArmBody::Statements(statements) => {
                    statements_assign_only_booleans(statements, name, saw_assignment)
                }
            };
            !crate::analysis::expression_assigns_name(scrutinee, name)
                && arms
                    .iter()
                    .all(|arm| arm_is_safe(&arm.body, saw_assignment))
                && default
                    .as_ref()
                    .is_none_or(|body| arm_is_safe(body, saw_assignment))
        }
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => true,
        Statement::InlineAsm(_) => false,
    }
}

fn is_boolean_constant(expression: &Expression) -> bool {
    matches!(crate::analysis::constant_value(expression), Some(0 | 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::UnsignedChar,
            name: "flag".into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        }
    }

    fn function(then_value: i64) -> Function {
        Function {
            return_type: Type::Void,
            name: "choose".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![local()],
            statements: vec![Statement::If {
                condition: Expression::Variable("condition".into()),
                then_body: vec![Statement::Assign {
                    name: "flag".into(),
                    value: Expression::IntegerLiteral(then_value),
                }],
                else_body: vec![Statement::Assign {
                    name: "flag".into(),
                    value: Expression::IntegerLiteral(0),
                }],
            }],
            guards: Vec::new(),
            return_expression: None,
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
    fn accepts_a_boolean_assigned_zero_or_one_in_both_arms() {
        assert!(
            source_proven_canonical_boolean_locals(&function(1)).contains("flag")
        );
    }

    #[test]
    fn rejects_a_narrow_local_with_a_non_boolean_assignment() {
        assert!(source_proven_canonical_boolean_locals(&function(2)).is_empty());
    }
}
