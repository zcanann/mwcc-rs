//! Register mirrors for frame scalars whose source address never escapes.
//!
//! Decompilation sources sometimes retain `T **REF = &value` declarations to
//! reproduce MWCC's stack image even though `REF` is never read. The address
//! still requires the frame slot, but it cannot expose a mutation. Treating
//! such values like arbitrary escaped scalars loses MWCC's saved-register
//! mirror and introduces a reload before nearly every use.

use super::*;

pub(super) struct Plan {
    names: std::collections::HashSet<String>,
}

impl Plan {
    pub(super) fn recognize(function: &Function) -> Option<Self> {
        let declared: std::collections::HashSet<&str> = function
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .chain(function.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let mut names = std::collections::HashSet::new();
        for alias in &function.locals {
            let Some(Expression::AddressOf { operand }) = alias.initializer.as_ref() else {
                continue;
            };
            let Expression::Variable(target) = operand.as_ref() else {
                continue;
            };
            if !declared.contains(target.as_str())
                || local_is_observed(function, &alias.name)
                || address_use_count(function, target) != 1
                || statements_assign_name(&function.statements, target)
            {
                continue;
            }
            names.insert(target.clone());
        }
        (!names.is_empty()).then_some(Self { names })
    }

    pub(super) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub(super) fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(String::as_str)
    }

    /// Passive frame images do not shorten the lifetime of their register
    /// values. Keep the complete saved-value transaction in source-allocation
    /// order at the bottom of the nonvolatile window; allowing an unpreferred
    /// home here can color a call-spanning mirror into a volatile argument
    /// register merely because its frame image is also available.
    pub(super) fn home_preference(&self, home_index: usize, first_saved: usize) -> Option<u8> {
        let preferred = first_saved.checked_add(home_index)?;
        (preferred >= 14 && preferred < 32).then(|| preferred as u8)
    }
}

fn local_is_observed(function: &Function, name: &str) -> bool {
    super::structured_locals::body_uses_local(&function.statements, name)
        || function
            .locals
            .iter()
            .filter_map(|local| local.initializer.as_ref())
            .any(|initializer| crate::analysis::expression_reads_name(initializer, name))
        || function
            .return_expression
            .as_ref()
            .is_some_and(|value| crate::analysis::expression_reads_name(value, name))
        || function.guards.iter().any(|guard| {
            crate::analysis::expression_reads_name(&guard.condition, name)
                || crate::analysis::expression_reads_name(&guard.value, name)
        })
}

fn address_use_count(function: &Function, name: &str) -> usize {
    let mut count = 0usize;
    let mut visit = |expression: &Expression| {
        count += usize::from(matches!(
            expression,
            Expression::AddressOf { operand }
                if matches!(operand.as_ref(), Expression::Variable(target) if target == name)
        ));
    };
    for initializer in function
        .locals
        .iter()
        .filter_map(|local| local.initializer.as_ref())
    {
        super::structured_expression_visit::visit_expression(initializer, &mut visit);
    }
    for statement in &function.statements {
        super::structured_expression_visit::visit_statement(statement, &mut visit);
    }
    if let Some(value) = &function.return_expression {
        super::structured_expression_visit::visit_expression(value, &mut visit);
    }
    for guard in &function.guards {
        super::structured_expression_visit::visit_expression(&guard.condition, &mut visit);
        super::structured_expression_visit::visit_expression(&guard.value, &mut visit);
    }
    count
}

fn statements_assign_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || crate::analysis::expression_assigns_name(value, name),
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            crate::analysis::expression_assigns_name(expression, name)
        }
        Statement::Store { target, value } => {
            crate::analysis::expression_assigns_name(target, name)
                || crate::analysis::expression_assigns_name(value, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            crate::analysis::expression_assigns_name(condition, name)
                || statements_assign_name(then_body, name)
                || statements_assign_name(else_body, name)
        }
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            [initializer, condition, step]
                .into_iter()
                .flatten()
                .any(|expression| crate::analysis::expression_assigns_name(expression, name))
                || statements_assign_name(body, name)
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            crate::analysis::expression_assigns_name(scrutinee, name)
                || arms.iter().any(|arm| arm_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_assigns_name(body, name))
        }
        Statement::InlineAsm(_) => true,
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    })
}

fn arm_assigns_name(body: &mwcc_syntax_trees::ArmBody, name: &str) -> bool {
    match body {
        mwcc_syntax_trees::ArmBody::Return(value) => {
            crate::analysis::expression_assigns_name(value, name)
        }
        mwcc_syntax_trees::ArmBody::Statements(statements) => {
            statements_assign_name(statements, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: if name.starts_with("alias") {
                Type::Pointer(mwcc_syntax_trees::Pointee::Int)
            } else {
                Type::Int
            },
            name: name.into(),
            initializer,
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

    fn address(name: &str) -> Expression {
        Expression::AddressOf {
            operand: Box::new(Expression::Variable(name.into())),
        }
    }

    fn function(mut locals: Vec<LocalDeclaration>, statements: Vec<Statement>) -> Function {
        locals.insert(0, local("value", Some(Expression::IntegerLiteral(7))));
        Function {
            return_type: Type::Int,
            name: "frame_mirror".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals,
            statements,
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("value".into())),
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
    fn recognizes_an_unobserved_address_alias() {
        let source = function(vec![local("alias", Some(address("value")))], Vec::new());
        let plan = Plan::recognize(&source).expect("dead alias should retain a register mirror");

        assert!(plan.contains("value"));
    }

    #[test]
    fn rejects_an_alias_that_exposes_the_address() {
        let source = function(
            vec![local("alias", Some(address("value")))],
            vec![Statement::Expression(Expression::Variable("alias".into()))],
        );

        assert!(Plan::recognize(&source).is_none());
    }

    #[test]
    fn rejects_a_second_address_or_a_later_assignment() {
        let twice = function(
            vec![
                local("alias_a", Some(address("value"))),
                local("alias_b", Some(address("value"))),
            ],
            Vec::new(),
        );
        let assigned = function(
            vec![local("alias", Some(address("value")))],
            vec![Statement::Assign {
                name: "value".into(),
                value: Expression::IntegerLiteral(9),
            }],
        );

        assert!(Plan::recognize(&twice).is_none());
        assert!(Plan::recognize(&assigned).is_none());
    }
}
