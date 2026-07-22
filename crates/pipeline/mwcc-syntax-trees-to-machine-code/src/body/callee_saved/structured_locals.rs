//! Register planning for short-lived locals in structured non-leaf bodies.
//!
//! Callee-saved homes and ephemeral values are separate lifetime classes. This
//! module computes the latter before emission so the structured body can decline
//! atomically when a local needs a call result, stack storage, or another
//! unmodeled lifetime.

#[allow(unused_imports)]
use super::*;

pub(super) struct DeferredSavedHomePlan {
    group_by_name: std::collections::HashMap<String, usize>,
    pub(super) group_count: usize,
}

impl DeferredSavedHomePlan {
    pub(super) fn group(&self, name: &str) -> usize {
        self.group_by_name[name]
    }
}

/// Color single-assignment locals whose initialization remains in the body.
/// Two locals may share a callee-saved home only when the first one's final
/// textual read precedes the second one's sole assignment. Structured bodies
/// have no loops or backward branches, so that source-order proof is also a
/// control-flow-safe non-overlap proof.
pub(super) fn plan_deferred_saved_homes(
    function: &Function,
    locals: &[&LocalDeclaration],
) -> Option<DeferredSavedHomePlan> {
    let mut intervals = Vec::with_capacity(locals.len());
    for local in locals {
        let mut cursor = 0usize;
        let mut interval = DeferredInterval::default();
        collect_deferred_interval(
            &function.statements,
            &local.name,
            &mut cursor,
            &mut interval,
        )?;
        if interval.assignment_count != 1 {
            return None;
        }
        let first_assignment = interval.first_assignment?;
        let last_read = interval.last_read.unwrap_or(first_assignment);
        if last_read < first_assignment {
            return None;
        }
        intervals.push((local.name.as_str(), first_assignment, last_read));
    }
    intervals.sort_by_key(|(_, first_assignment, _)| *first_assignment);

    let mut group_last_reads = Vec::<usize>::new();
    let mut group_by_name = std::collections::HashMap::new();
    for (name, first_assignment, last_read) in intervals {
        let group = group_last_reads
            .iter()
            .position(|previous_last_read| *previous_last_read < first_assignment)
            .unwrap_or_else(|| {
                group_last_reads.push(0);
                group_last_reads.len() - 1
            });
        group_last_reads[group] = last_read;
        group_by_name.insert(name.to_owned(), group);
    }
    Some(DeferredSavedHomePlan {
        group_count: group_last_reads.len(),
        group_by_name,
    })
}

#[derive(Default)]
struct DeferredInterval {
    first_assignment: Option<usize>,
    last_read: Option<usize>,
    assignment_count: usize,
}

fn collect_deferred_interval(
    statements: &[Statement],
    name: &str,
    cursor: &mut usize,
    interval: &mut DeferredInterval,
) -> Option<()> {
    for statement in statements {
        *cursor += 1;
        let position = *cursor;
        let reads = match statement {
            Statement::Store { target, value } => {
                expression_reads_name(target, name) || expression_reads_name(value, name)
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => expression_reads_name(value, name),
            Statement::If { condition, .. } => expression_reads_name(condition, name),
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => false,
            Statement::Switch { .. } | Statement::Loop { .. } => return None,
        };
        if reads {
            interval.last_read = Some(position);
        }
        if matches!(statement, Statement::Assign { name: assigned, .. } if assigned == name) {
            interval.assignment_count += 1;
            interval.first_assignment.get_or_insert(position);
        }
        if let Statement::If {
            then_body,
            else_body,
            ..
        } = statement
        {
            collect_deferred_interval(then_body, name, cursor, interval)?;
            collect_deferred_interval(else_body, name, cursor, interval)?;
        }
    }
    Some(())
}

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
            || !matches!(
                class_of(local.declared_type),
                Ok(ValueClass::General | ValueClass::Float)
            )
            || local.initializer.as_ref().is_some_and(expression_has_call)
            || (local.initializer.is_none()
                && !is_definitely_assigned_before_reads(&function.statements, &local.name))
    }) {
        return None;
    }
    Some(ephemeral)
}

/// Return floating ephemeral locals whose lexical lifetime has ended before
/// `remaining_statements`. Structured lowering uses this only at the function
/// body's top level: nested blocks may still flow into an enclosing suffix.
pub(super) fn dead_ephemeral_float_locals<'a>(
    ephemeral_locals: &[&'a LocalDeclaration],
    remaining_statements: &[Statement],
) -> Vec<&'a str> {
    ephemeral_locals
        .iter()
        .filter(|local| {
            class_of(local.declared_type).ok() == Some(ValueClass::Float)
                && !body_uses_local(remaining_statements, &local.name)
        })
        .map(|local| local.name.as_str())
        .collect()
}

#[derive(Clone, Copy)]
struct AssignmentFlow {
    initialized: bool,
    assigned: bool,
    falls_through: bool,
}

/// Validate an uninitialized scalar whose first value is supplied by a later
/// assignment. This is the shape introduced by call-site inline expansion:
/// declarations live in the function table, while their initialization must
/// remain inside the branch containing the original inline call.
pub(super) fn is_definitely_assigned_before_reads(statements: &[Statement], name: &str) -> bool {
    assignment_flow(statements, name, false).is_some_and(|flow| flow.assigned)
}

fn assignment_flow(
    statements: &[Statement],
    name: &str,
    mut initialized: bool,
) -> Option<AssignmentFlow> {
    let mut assigned = false;
    for statement in statements {
        let reads_before_assignment = match statement {
            Statement::Assign {
                name: assigned_name,
                value,
            } if assigned_name == name => expression_reads_name(value, name),
            Statement::Store { target, value } => {
                expression_reads_name(target, name) || expression_reads_name(value, name)
            }
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => expression_reads_name(value, name),
            Statement::If { condition, .. } => expression_reads_name(condition, name),
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => false,
            Statement::Switch { .. } | Statement::Loop { .. } => return None,
        };
        if reads_before_assignment && !initialized {
            return None;
        }

        match statement {
            Statement::Assign {
                name: assigned_name,
                ..
            } if assigned_name == name => {
                initialized = true;
                assigned = true;
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                let then_flow = assignment_flow(then_body, name, initialized)?;
                let else_flow = assignment_flow(else_body, name, initialized)?;
                assigned |= then_flow.assigned || else_flow.assigned;
                initialized = match (then_flow.falls_through, else_flow.falls_through) {
                    (true, true) => then_flow.initialized && else_flow.initialized,
                    (true, false) => then_flow.initialized,
                    (false, true) => else_flow.initialized,
                    (false, false) => {
                        return Some(AssignmentFlow {
                            initialized,
                            assigned,
                            falls_through: false,
                        });
                    }
                };
            }
            Statement::Return(_) => {
                return Some(AssignmentFlow {
                    initialized,
                    assigned,
                    falls_through: false,
                });
            }
            _ => {}
        }
    }
    Some(AssignmentFlow {
        initialized,
        assigned,
        falls_through: true,
    })
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

    #[test]
    fn accepts_branch_local_assignment_before_every_reachable_read() {
        let mut temporary = local("temporary", Expression::IntegerLiteral(0));
        temporary.declared_type = Type::Float;
        temporary.initializer = None;
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![temporary],
            statements: vec![Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: vec![
                    Statement::Assign {
                        name: "temporary".into(),
                        value: Expression::FloatLiteral(1.0),
                    },
                    Statement::Expression(Expression::Call {
                        name: "consume".into(),
                        arguments: vec![Expression::Variable("temporary".into())],
                    }),
                ],
                else_body: Vec::new(),
            }],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let planned = plan_ephemeral_locals(&function, &std::collections::HashSet::new())
            .expect("the branch-local float lifetime is valid");
        assert_eq!(planned.len(), 1);
    }

    #[test]
    fn expires_branch_local_float_before_the_following_statement() {
        let mut temporary = local("temporary", Expression::IntegerLiteral(0));
        temporary.declared_type = Type::Float;
        temporary.initializer = None;
        let later = Statement::Expression(Expression::Call {
            name: "consume_later".into(),
            arguments: Vec::new(),
        });

        assert_eq!(
            dead_ephemeral_float_locals(&[&temporary], std::slice::from_ref(&later)),
            ["temporary"]
        );
    }

    #[test]
    fn retains_branch_local_float_read_by_the_following_statement() {
        let mut temporary = local("temporary", Expression::IntegerLiteral(0));
        temporary.declared_type = Type::Float;
        temporary.initializer = None;
        let later = Statement::Expression(Expression::Call {
            name: "consume_later".into(),
            arguments: vec![Expression::Variable("temporary".into())],
        });

        assert!(dead_ephemeral_float_locals(
            &[&temporary],
            std::slice::from_ref(&later)
        )
        .is_empty());
    }

    #[test]
    fn rejects_a_read_reachable_before_the_first_assignment() {
        let mut temporary = local("temporary", Expression::IntegerLiteral(0));
        temporary.initializer = None;
        let mut function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![temporary],
            statements: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("temporary".into())],
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
        function.statements.push(Statement::Assign {
            name: "temporary".into(),
            value: Expression::IntegerLiteral(1),
        });

        assert!(plan_ephemeral_locals(&function, &std::collections::HashSet::new()).is_none());
    }

    #[test]
    fn coalesces_disjoint_deferred_saved_local_lifetimes() {
        let mut first = local("first", Expression::IntegerLiteral(0));
        first.initializer = None;
        let mut second = local("second", Expression::IntegerLiteral(0));
        second.initializer = None;
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![first, second],
            statements: vec![
                Statement::Assign {
                    name: "first".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("first".into())],
                }),
                Statement::Assign {
                    name: "second".into(),
                    value: Expression::IntegerLiteral(2),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("second".into())],
                }),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let locals: Vec<_> = function.locals.iter().collect();
        let plan = plan_deferred_saved_homes(&function, &locals).unwrap();
        assert_eq!(plan.group_count, 1);
        assert_eq!(plan.group("first"), plan.group("second"));
    }

    #[test]
    fn separates_overlapping_deferred_saved_local_lifetimes() {
        let mut first = local("first", Expression::IntegerLiteral(0));
        first.initializer = None;
        let mut second = local("second", Expression::IntegerLiteral(0));
        second.initializer = None;
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![first, second],
            statements: vec![
                Statement::Assign {
                    name: "first".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Assign {
                    name: "second".into(),
                    value: Expression::IntegerLiteral(2),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![
                        Expression::Variable("first".into()),
                        Expression::Variable("second".into()),
                    ],
                }),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let locals: Vec<_> = function.locals.iter().collect();
        let plan = plan_deferred_saved_homes(&function, &locals).unwrap();
        assert_eq!(plan.group_count, 2);
        assert_ne!(plan.group("first"), plan.group("second"));
    }
}
