//! Path-sensitive reuse of eager saved homes by branch-local values.
//!
//! A value initialized at entry and used in only one arm of a conditional does
//! not interfere with a value defined and consumed exclusively in the other
//! arm. MWCC colors those values into one callee-saved home. Keep the
//! control-flow proof separate from physical-register layout and emission.

use super::structured_locals::{
    structured_name_last_read, structured_name_occurs_in_loop,
    DeferredSavedHomePlan,
};
#[allow(unused_imports)]
use super::*;

pub(super) struct StructuredEagerHomeReuse {
    home_index_by_group: Vec<Option<usize>>,
    expired_read_by_group: Vec<Option<usize>>,
}

impl StructuredEagerHomeReuse {
    pub(super) fn plan(
        function: &Function,
        eager_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
    ) -> Self {
        let mut home_index_by_group = vec![None; deferred.group_count];
        let mut expired_read_by_group = vec![None; deferred.group_count];
        let mut occupied_eager_homes = std::collections::HashSet::new();
        for group in 0..deferred.group_count {
            let members: Vec<_> = deferred.members(group).collect();
            let branch_reuse = eager_locals
                .iter()
                .enumerate()
                .find_map(|(home, eager)| {
                    (!occupied_eager_homes.contains(&home)
                        && branch_exclusive(function, &eager.name, &members))
                        .then_some(home)
                });
            let expired_reuse = if branch_reuse.is_none() {
                eager_locals.iter().enumerate().find_map(|(home, eager)| {
                    if occupied_eager_homes.contains(&home) {
                        None
                    } else {
                        expiration_before_group(
                            function,
                            &eager.name,
                            deferred.first_assignment(group),
                        )
                        .map(|last_read| (home, last_read))
                    }
                })
            } else {
                None
            };
            if let Some(home) = branch_reuse {
                occupied_eager_homes.insert(home);
                home_index_by_group[group] = Some(home);
            } else if let Some((home, last_read)) = expired_reuse {
                occupied_eager_homes.insert(home);
                home_index_by_group[group] = Some(home);
                expired_read_by_group[group] = Some(last_read);
            }
        }
        Self {
            home_index_by_group,
            expired_read_by_group,
        }
    }

    pub(super) fn home_index(&self, group: usize) -> Option<usize> {
        self.home_index_by_group[group]
    }

    /// The final eager read behind a sequential reuse. Branch-exclusive reuse
    /// has no single expiration point and therefore cannot be superseded by a
    /// parameter interval.
    pub(super) fn expired_last_read(&self, group: usize) -> Option<usize> {
        self.expired_read_by_group[group]
    }
}

fn expiration_before_group(
    function: &Function,
    eager: &str,
    first_assignment: usize,
) -> Option<usize> {
    (!structured_name_occurs_in_loop(function, eager)
        && !statement_assigns_name(&function.statements, eager)
        && function
            .return_expression
            .as_ref()
            .is_none_or(|expression| !expression_reads_name(expression, eager)))
    .then(|| structured_name_last_read(function, eager))
    .flatten()
    .filter(|last_read| first_assignment >= *last_read)
}

fn branch_exclusive(function: &Function, eager: &str, deferred: &[&str]) -> bool {
    if statement_assigns_name(&function.statements, eager) {
        return false;
    }
    let names: std::collections::HashSet<_> = std::iter::once(eager)
        .chain(deferred.iter().copied())
        .collect();
    let mut occurrences = std::collections::HashMap::<&str, Vec<Vec<(usize, bool)>>>::new();
    let mut next_branch = 0;
    if collect_occurrences(
        &function.statements,
        &names,
        &mut Vec::new(),
        &mut next_branch,
        &mut occurrences,
    )
    .is_none()
    {
        return false;
    }
    let Some(eager_occurrences) = occurrences.get(eager).filter(|paths| !paths.is_empty()) else {
        return false;
    };
    let deferred_occurrences: Vec<_> = deferred
        .iter()
        .filter_map(|name| occurrences.get(name))
        .flatten()
        .collect();
    if deferred_occurrences.is_empty()
        || deferred
            .iter()
            .any(|name| occurrences.get(name).is_none_or(Vec::is_empty))
    {
        return false;
    }

    eager_occurrences[0].iter().any(|(branch, eager_arm)| {
        eager_occurrences
            .iter()
            .all(|path| path.contains(&(*branch, *eager_arm)))
            && deferred_occurrences
                .iter()
                .all(|path| path.contains(&(*branch, !*eager_arm)))
    })
}

fn collect_occurrences<'a>(
    statements: &[Statement],
    names: &std::collections::HashSet<&'a str>,
    path: &mut Vec<(usize, bool)>,
    next_branch: &mut usize,
    occurrences: &mut std::collections::HashMap<&'a str, Vec<Vec<(usize, bool)>>>,
) -> Option<()> {
    for statement in statements {
        match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                record_expression(condition, names, path, occurrences);
                let branch = *next_branch;
                *next_branch += 1;
                path.push((branch, true));
                collect_occurrences(then_body, names, path, next_branch, occurrences)?;
                path.pop();
                path.push((branch, false));
                collect_occurrences(else_body, names, path, next_branch, occurrences)?;
                path.pop();
            }
            Statement::Store { target, value } => {
                record_expression(target, names, path, occurrences);
                record_expression(value, names, path, occurrences);
            }
            Statement::Assign { name, value } => {
                if let Some(name) = names.get(name.as_str()) {
                    occurrences
                        .entry(*name)
                        .or_default()
                        .push(path.clone());
                }
                record_expression(value, names, path, occurrences);
            }
            Statement::Expression(expression) | Statement::Return(Some(expression)) => {
                record_expression(expression, names, path, occurrences);
            }
            Statement::Loop { .. } | Statement::Switch { .. } => return None,
            Statement::InlineAsm(_)
            | Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => {}
        }
    }
    Some(())
}

fn record_expression<'a>(
    expression: &Expression,
    names: &std::collections::HashSet<&'a str>,
    path: &[(usize, bool)],
    occurrences: &mut std::collections::HashMap<&'a str, Vec<Vec<(usize, bool)>>>,
) {
    for name in names {
        if expression_reads_name(expression, name) {
            occurrences.entry(*name).or_default().push(path.to_vec());
        }
    }
}

fn statement_assigns_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: assigned, .. } => assigned == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statement_assigns_name(then_body, name) || statement_assigns_name(else_body, name)
        }
        Statement::Loop { body, .. } => statement_assigns_name(body, name),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::super::structured_locals::plan_deferred_saved_homes;
    use super::*;

    fn local(name: &str, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type: Type::Int,
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

    fn consume(name: &str) -> Statement {
        Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![Expression::Variable(name.into())],
        })
    }

    fn function(uses_eager_after_branch: bool) -> Function {
        let mut statements = vec![Statement::If {
            condition: Expression::Variable("condition".into()),
            then_body: vec![
                Statement::Assign {
                    name: "branch_value".into(),
                    value: Expression::IntegerLiteral(2),
                },
                consume("branch_value"),
            ],
            else_body: vec![consume("entry_value")],
        }];
        if uses_eager_after_branch {
            statements.push(consume("entry_value"));
        }
        Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![
                local("entry_value", Some(Expression::IntegerLiteral(1))),
                local("branch_value", None),
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

    fn sequential_function() -> Function {
        let mut function = function(false);
        function.statements = vec![
            consume("entry_value"),
            Statement::Assign {
                name: "branch_value".into(),
                value: Expression::IntegerLiteral(2),
            },
            consume("branch_value"),
        ];
        function
    }

    #[test]
    fn reuses_an_eager_home_across_mutually_exclusive_arms() {
        let function = function(false);
        let deferred = plan_deferred_saved_homes(
            &function,
            &[function.locals.iter().find(|local| local.name == "branch_value").unwrap()],
        )
        .unwrap();
        let reuse =
            StructuredEagerHomeReuse::plan(&function, &[&function.locals[0]], &deferred);

        assert_eq!(reuse.home_index(deferred.group("branch_value")), Some(0));
    }

    #[test]
    fn keeps_distinct_homes_when_the_eager_value_reaches_the_join() {
        let function = function(true);
        let deferred = plan_deferred_saved_homes(
            &function,
            &[function.locals.iter().find(|local| local.name == "branch_value").unwrap()],
        )
        .unwrap();
        let reuse =
            StructuredEagerHomeReuse::plan(&function, &[&function.locals[0]], &deferred);

        assert_eq!(reuse.home_index(deferred.group("branch_value")), None);
    }

    #[test]
    fn reuses_an_eager_home_after_its_final_read() {
        let function = sequential_function();
        let deferred = plan_deferred_saved_homes(
            &function,
            &[function.locals.iter().find(|local| local.name == "branch_value").unwrap()],
        )
        .unwrap();
        let reuse =
            StructuredEagerHomeReuse::plan(&function, &[&function.locals[0]], &deferred);

        assert_eq!(reuse.home_index(deferred.group("branch_value")), Some(0));
    }
}
