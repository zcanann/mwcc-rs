//! Cost-free saved-home influence from short-lived indirect-call operands.
//!
//! MWCC lets a deferred operand participate in whole-body coloring when it can
//! reuse an entry value's expired saved home.  The operand may still lower into
//! a volatile register at its terminal assignment, but its lifetime changes
//! entry-home ordering and scheduling.  Do not promote it when doing so would
//! allocate another callee-saved register.

use super::structured_eager_home_reuse::StructuredEagerHomeReuse;
use super::structured_expression_visit::visit_statement;
use super::structured_locals::plan_deferred_saved_homes;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) fn promote_cost_free_indirect_call_locals<'a>(
    function: &'a Function,
    survivors: &std::collections::HashSet<&str>,
    saved_parameters: &[&Parameter],
    saved_locals: &[&'a LocalDeclaration],
) -> Vec<&'a LocalDeclaration> {
    let mut selected: std::collections::HashSet<&str> = saved_locals
        .iter()
        .filter(|local| class_of(local.declared_type).ok() == Some(ValueClass::General))
        .map(|local| local.name.as_str())
        .collect();
    let Some(mut fresh_count) = fresh_home_count(function, saved_parameters, &selected) else {
        return Vec::new();
    };
    let mut promoted = Vec::new();

    for local in &function.locals {
        if local.initializer.is_some()
            || local.array_length.is_some()
            || local.is_static
            || local.is_volatile
            || survivors.contains(local.name.as_str())
            || class_of(local.declared_type).ok() != Some(ValueClass::General)
            || !indirect_call_reads(function, &local.name)
        {
            continue;
        }
        selected.insert(&local.name);
        let Some(trial_count) = fresh_home_count(function, saved_parameters, &selected) else {
            selected.remove(local.name.as_str());
            continue;
        };
        if trial_count <= fresh_count {
            fresh_count = trial_count;
            promoted.push(local);
        } else {
            selected.remove(local.name.as_str());
        }
    }
    promoted
}

fn fresh_home_count(
    function: &Function,
    saved_parameters: &[&Parameter],
    selected: &std::collections::HashSet<&str>,
) -> Option<usize> {
    let selected_locals: Vec<_> = function
        .locals
        .iter()
        .filter(|local| selected.contains(local.name.as_str()))
        .collect();
    let (eager, deferred): (Vec<_>, Vec<_>) = selected_locals
        .into_iter()
        .partition(|local| local.initializer.is_some());
    let deferred = plan_deferred_saved_homes(function, &deferred)?;
    let eager_reuse = StructuredEagerHomeReuse::plan(function, &eager, &deferred);
    Some(
        StructuredParameterHomeReuse::plan(
            function,
            eager.len(),
            saved_parameters,
            &deferred,
            &eager_reuse,
        )
        .fresh_group_count,
    )
}

fn indirect_call_reads(function: &Function, name: &str) -> bool {
    let mut found = false;
    let mut inspect = |expression: &Expression| {
        if matches!(expression, Expression::CallThrough { .. })
            && expression_reads_name(expression, name)
        {
            found = true;
        }
    };
    for statement in &function.statements {
        visit_statement(statement, &mut inspect);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::StructPointer { element_size: 16 },
            name: name.into(),
            initializer,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    fn function(reads_entry_late: bool) -> Function {
        let mut statements = vec![
            Statement::Expression(Expression::Call {
                name: "prepare".into(),
                arguments: Vec::new(),
            }),
            Statement::Expression(Expression::Variable("entry".into())),
            Statement::Assign {
                name: "late".into(),
                value: Expression::Member {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 4,
                    member_type: Type::StructPointer { element_size: 16 },
                    index_stride: None,
                },
            },
            Statement::Expression(Expression::CallThrough {
                target: Box::new(Expression::Variable("late".into())),
                arguments: vec![Expression::Variable("object".into())],
            }),
        ];
        if reads_entry_late {
            statements.push(Statement::Expression(Expression::Variable("entry".into())));
            statements.push(Statement::Expression(Expression::Variable("object".into())));
        }
        Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 16 },
                name: "object".into(),
            }],
            locals: vec![
                local(
                    "entry",
                    Some(Expression::Member {
                        base: Box::new(Expression::Variable("object".into())),
                        offset: 8,
                        member_type: Type::StructPointer { element_size: 16 },
                        index_stride: None,
                    }),
                ),
                local("late", None),
            ],
            statements,
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
    fn promotes_an_indirect_operand_into_an_expired_eager_home() {
        let function = function(false);
        let promoted = promote_cost_free_indirect_call_locals(
            &function,
            &std::collections::HashSet::from(["entry", "object"]),
            &[&function.parameters[0]],
            &[&function.locals[0]],
        );

        assert_eq!(
            promoted
                .iter()
                .map(|local| local.name.as_str())
                .collect::<Vec<_>>(),
            ["late"]
        );
    }

    #[test]
    fn declines_an_indirect_operand_that_needs_a_new_saved_home() {
        let function = function(true);
        let promoted = promote_cost_free_indirect_call_locals(
            &function,
            &std::collections::HashSet::from(["entry", "object"]),
            &[&function.parameters[0]],
            &[&function.locals[0]],
        );

        assert!(promoted.is_empty());
    }
}
