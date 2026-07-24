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
    group_first_assignments: Vec<usize>,
    pub(super) group_count: usize,
}

impl DeferredSavedHomePlan {
    pub(super) fn group(&self, name: &str) -> usize {
        self.group_by_name[name]
    }

    pub(super) fn first_assignment(&self, group: usize) -> usize {
        self.group_first_assignments[group]
    }

    pub(super) fn member_count(&self, group: usize) -> usize {
        self.group_by_name
            .values()
            .filter(|candidate| **candidate == group)
            .count()
    }

    pub(super) fn contains_value_version(&self, group: usize) -> bool {
        self.group_by_name
            .iter()
            .any(|(name, candidate)| *candidate == group && name.starts_with("__mwcc_value_"))
    }
}

pub(super) fn structured_name_last_read(function: &Function, name: &str) -> Option<usize> {
    let mut cursor = 0;
    let mut interval = DeferredInterval::default();
    collect_deferred_interval(&function.statements, name, &mut cursor, &mut interval)?;
    interval.last_read
}

/// Color deferred locals whose initialization remains in the body.
/// Two locals may share a callee-saved home only when the first one's final
/// textual read precedes the second one's first assignment. Structured bodies
/// have no loops or backward branches, so that source-order proof is also a
/// control-flow-safe non-overlap proof. A local may be updated repeatedly after
/// its first definition; the eligibility pass separately proves that every read
/// is dominated by that definition.
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
        let first_assignment = if interval.assignment_count == 0
            && local.initializer.is_some()
        {
            // An entry initializer defines the value before statement zero.
            // Its interval otherwise follows the same overlap rules as a
            // deferred assignment, but it can never reuse a home whose value
            // expires later in the body.
            0
        } else {
            interval.first_assignment?
        };
        let last_read = interval.last_read.unwrap_or(first_assignment);
        if last_read < first_assignment {
            return None;
        }
        intervals.push((local.name.as_str(), first_assignment, last_read));
    }
    intervals.sort_by_key(|(_, first_assignment, _)| *first_assignment);

    let mut group_last_reads = Vec::<usize>::new();
    let mut group_first_assignments = Vec::<usize>::new();
    let mut group_by_name = std::collections::HashMap::new();
    for (name, first_assignment, last_read) in intervals {
        // MWCC reuses the most recently expired local home. This is a LIFO
        // lifetime discipline, not first-fit coloring: when several homes are
        // free, a new deferred local takes the one whose previous value died
        // latest (for example a status result reuses the just-dead length home,
        // not an older answer home).
        let starts_load_batch = starts_deferred_load_batch(function, name);
        let group = (!name.starts_with("__mwcc_value_") && !starts_load_batch)
            .then(|| {
                group_last_reads
                    .iter()
                    .enumerate()
                    .filter(|(_, previous_last_read)| **previous_last_read < first_assignment)
                    .max_by_key(|(_, previous_last_read)| **previous_last_read)
                    .map(|(group, _)| group)
            })
            .flatten()
            .unwrap_or_else(|| {
                group_last_reads.push(0);
                group_first_assignments.push(first_assignment);
                group_last_reads.len() - 1
            });
        group_last_reads[group] = last_read;
        group_by_name.insert(name.to_owned(), group);
    }
    Some(DeferredSavedHomePlan {
        group_count: group_last_reads.len(),
        group_by_name,
        group_first_assignments,
    })
}

fn starts_deferred_load_batch(function: &Function, candidate: &str) -> bool {
    function
        .statements
        .iter()
        .enumerate()
        .any(|(index, statement)| match statement {
            Statement::Assign { name, value } => name == candidate
                && is_direct_load(value)
                && function.statements.get(index + 1).is_some_and(
                    |next| matches!(next, Statement::Assign { value, .. } if is_direct_load(value)),
                )
                && index.checked_sub(1).is_none_or(|previous| {
                    !matches!(
                        &function.statements[previous],
                        Statement::Assign { value, .. } if is_direct_load(value)
                    )
                }),
            _ => false,
        })
}

fn is_direct_load(expression: &Expression) -> bool {
    match expression {
        Expression::Dereference { .. } => true,
        Expression::Cast { operand, .. } => is_direct_load(operand),
        _ => false,
    }
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
        let expression = match statement {
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => Some(value),
            Statement::If { condition, .. } => Some(condition),
            _ => None,
        };
        if let Some(expression) = expression {
            collect_expression_interval(expression, name, position, interval);
        }
        let reads = match statement {
            Statement::Store { target, value } => {
                expression_reads_name(target, name) || expression_reads_name(value, name)
            }
            Statement::Assign { .. }
            | Statement::Expression(_)
            | Statement::Return(Some(_))
            | Statement::If { .. } => false,
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => false,
            Statement::Loop {
                initializer,
                condition,
                step,
                ..
            } => initializer.as_ref().is_some_and(|expression| {
                match expression {
                    Expression::Assign { target, value }
                        if matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name) =>
                    {
                        expression_reads_name(value, name)
                    }
                    _ => expression_reads_name(expression, name),
                }
            }) || condition
                .iter()
                .chain(step)
                .any(|expression| expression_reads_name(expression, name)),
            Statement::Switch { .. } => return None,
        };
        if reads {
            interval.last_read = Some(position);
        }
        if matches!(statement, Statement::Assign { name: assigned, .. } if assigned == name) {
            interval.assignment_count += 1;
            interval.first_assignment.get_or_insert(position);
        }
        if let Statement::Loop {
            initializer,
            body,
            step,
            ..
        } = statement
        {
            for expression in initializer.iter().chain(step) {
                collect_expression_interval(expression, name, position, interval);
            }
            collect_deferred_interval(body, name, cursor, interval)?;
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

fn collect_expression_interval(
    expression: &Expression,
    name: &str,
    position: usize,
    interval: &mut DeferredInterval,
) {
    if expression_reads_name(expression, name) {
        interval.last_read = Some(position);
    }
    let assignments = expression_assignment_count(expression, name);
    if assignments != 0 {
        interval.assignment_count += assignments;
        interval.first_assignment.get_or_insert(position);
    }
}

fn expression_assignment_count(expression: &Expression, name: &str) -> usize {
    match expression {
        Expression::Assign { target, value } => {
            usize::from(
                matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name),
            ) + expression_assignment_count(target, name)
                + expression_assignment_count(value, name)
        }
        Expression::AggregateLiteral(elements) => elements
            .iter()
            .map(|element| expression_assignment_count(element, name))
            .sum(),
        Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
            expression_assignment_count(left, name) + expression_assignment_count(right, name)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_assignment_count(condition, name)
                + expression_assignment_count(when_true, name)
                + expression_assignment_count(when_false, name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::PostStep {
            target: operand, ..
        }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand } => expression_assignment_count(operand, name),
        Expression::Index { base, index } => {
            expression_assignment_count(base, name) + expression_assignment_count(index, name)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_assignment_count(base, name)
        }
        Expression::Call { arguments, .. } => arguments
            .iter()
            .map(|argument| expression_assignment_count(argument, name))
            .sum(),
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            expression_assignment_count(allocation, name)
                + arguments
                    .iter()
                    .map(|argument| expression_assignment_count(argument, name))
                    .sum::<usize>()
        }
        Expression::CallThrough { target, arguments } => {
            expression_assignment_count(target, name)
                + arguments
                    .iter()
                    .map(|argument| expression_assignment_count(argument, name))
                    .sum::<usize>()
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            expression_assignment_count(object, name)
                + arguments
                    .iter()
                    .map(|argument| expression_assignment_count(argument, name))
                    .sum::<usize>()
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => 0,
    }
}

pub(super) fn plan_ephemeral_locals<'a>(
    function: &'a Function,
    survivors: &std::collections::HashSet<&str>,
    frame_locals: &std::collections::HashSet<String>,
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
            local.array_length.is_none()
                && live.contains(local.name.as_str())
                && !survivors.contains(local.name.as_str())
                && !frame_locals.contains(local.name.as_str())
        })
        .collect();
    let unsupported: Vec<_> = ephemeral
        .iter()
        .copied()
        .filter(|local| {
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
        })
        .collect();
    if !unsupported.is_empty() {
        if std::env::var_os("MWCC_CAPTURE_FUNCTION")
            .is_some_and(|name| name == std::ffi::OsStr::new(&function.name))
        {
            eprintln!(
                "structured ephemeral planning rejected: {}",
                unsupported
                    .iter()
                    .map(|local| local.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
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
    let mut pending_gotos = std::collections::HashMap::<String, Vec<bool>>::new();
    let mut seen_labels = std::collections::HashSet::<String>::new();
    assignment_flow(
        statements,
        name,
        false,
        &mut pending_gotos,
        &mut seen_labels,
    )
    .is_some_and(|flow| flow.assigned && pending_gotos.is_empty())
}

fn assignment_flow(
    statements: &[Statement],
    name: &str,
    mut initialized: bool,
    pending_gotos: &mut std::collections::HashMap<String, Vec<bool>>,
    seen_labels: &mut std::collections::HashSet<String>,
) -> Option<AssignmentFlow> {
    let mut assigned = false;
    let mut falls_through = true;
    for statement in statements {
        if let Statement::Label(label) = statement {
            seen_labels.insert(label.clone());
            let incoming = pending_gotos.remove(label).unwrap_or_default();
            if falls_through || !incoming.is_empty() {
                initialized =
                    (!falls_through || initialized) && incoming.into_iter().all(|state| state);
                falls_through = true;
            }
            continue;
        }
        if let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        {
            let flow = loop_assignment_flow(
                *kind,
                initializer.as_ref(),
                condition.as_ref(),
                step.as_ref(),
                body,
                name,
                initialized,
                pending_gotos,
                seen_labels,
            )?;
            initialized = flow.initialized;
            assigned |= flow.assigned;
            continue;
        }
        if !falls_through {
            continue;
        }
        let embedded_expression = match statement {
            Statement::Assign { value, .. }
            | Statement::Expression(value)
            | Statement::Return(Some(value)) => Some(value),
            Statement::If { condition, .. } => Some(condition),
            _ => None,
        };
        if let Some(expression) = embedded_expression {
            let (next_initialized, expression_assigned) =
                expression_initialization_flow(expression, name, initialized)?;
            initialized = next_initialized;
            assigned |= expression_assigned;
        }
        let reads_before_assignment = match statement {
            Statement::Assign {
                name: assigned_name,
                value,
            } if assigned_name == name => expression_reads_name(value, name),
            Statement::Store { target, value } => {
                expression_reads_name(target, name) || expression_reads_name(value, name)
            }
            Statement::Assign { .. }
            | Statement::Expression(_)
            | Statement::Return(Some(_))
            | Statement::If { .. } => false,
            Statement::Return(None)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => false,
            Statement::Loop {
                initializer,
                condition,
                step,
                ..
            } => {
                let initialized_here = initializer
                    .as_ref()
                    .is_some_and(|expression| expression_assigns_name(expression, name));
                initializer.as_ref().is_some_and(|expression| match expression {
                    Expression::Assign { target, value }
                        if matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name) =>
                    {
                        expression_reads_name(value, name)
                    }
                    _ => expression_reads_name(expression, name),
                }) || (!initialized_here
                    && condition
                        .iter()
                        .chain(step)
                        .any(|expression| expression_reads_name(expression, name)))
            }
            Statement::Switch { .. } => return None,
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
                let then_flow =
                    assignment_flow(then_body, name, initialized, pending_gotos, seen_labels)?;
                let else_flow =
                    assignment_flow(else_body, name, initialized, pending_gotos, seen_labels)?;
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
            Statement::Goto(label) => {
                if seen_labels.contains(label) {
                    return None;
                }
                pending_gotos
                    .entry(label.clone())
                    .or_default()
                    .push(initialized);
                falls_through = false;
            }
            Statement::Return(_) | Statement::Break | Statement::Continue => falls_through = false,
            _ => {}
        }
    }
    Some(AssignmentFlow {
        initialized,
        assigned,
        falls_through,
    })
}

/// Validate one lexical iteration while retaining the conservative state that
/// can flow past a possibly-zero-iteration loop. Definitions inside the body
/// still count for local planning when all of their reads are dominated within
/// that same body; they simply do not become definitely initialized afterward.
#[allow(clippy::too_many_arguments)]
fn loop_assignment_flow(
    kind: LoopKind,
    initializer: Option<&Expression>,
    condition: Option<&Expression>,
    step: Option<&Expression>,
    body: &[Statement],
    name: &str,
    initialized: bool,
    pending_gotos: &mut std::collections::HashMap<String, Vec<bool>>,
    seen_labels: &mut std::collections::HashSet<String>,
) -> Option<AssignmentFlow> {
    let mut entry_initialized = initialized;
    let mut assigned = false;
    if let Some(initializer) = initializer {
        let (next, initializer_assigned) =
            expression_initialization_flow(initializer, name, entry_initialized)?;
        entry_initialized = next;
        assigned |= initializer_assigned;
    }

    // A pre-test condition executes even when the body does not. Definitions
    // in it therefore dominate the loop exit just like a `for` initializer.
    if kind != LoopKind::DoWhile {
        if let Some(condition) = condition {
            let (next, condition_assigned) =
                expression_initialization_flow(condition, name, entry_initialized)?;
            entry_initialized = next;
            assigned |= condition_assigned;
        }
    }

    let body_flow = assignment_flow(
        body,
        name,
        entry_initialized,
        pending_gotos,
        seen_labels,
    )?;
    assigned |= body_flow.assigned;

    // `continue` can reach the step without the body's fallthrough state. A
    // step that needs this local must already be safe at loop entry; otherwise
    // validating the normal fallthrough is sufficient for its assignments.
    if let Some(step) = step {
        expression_initialization_flow(step, name, entry_initialized)?;
        if body_flow.falls_through {
            let (_, step_assigned) =
                expression_initialization_flow(step, name, body_flow.initialized)?;
            assigned |= step_assigned;
        }
    }

    if kind == LoopKind::DoWhile {
        if let Some(condition) = condition {
            let condition_entry = if body_flow.falls_through {
                body_flow.initialized
            } else {
                entry_initialized
            };
            let (_, condition_assigned) =
                expression_initialization_flow(condition, name, condition_entry)?;
            assigned |= condition_assigned;
        }
    }

    Some(AssignmentFlow {
        initialized: entry_initialized,
        assigned,
        falls_through: true,
    })
}

/// Track assignments embedded in the expression forms introduced by inline
/// composition. Comma evaluates left-to-right; a conditional guarantees a
/// definition afterward only when both arms define it. Other expressions do
/// not define locals and retain the existing conservative read check.
fn expression_initialization_flow(
    expression: &Expression,
    name: &str,
    initialized: bool,
) -> Option<(bool, bool)> {
    fn sequence<'a>(
        expressions: impl IntoIterator<Item = &'a Expression>,
        name: &str,
        mut initialized: bool,
    ) -> Option<(bool, bool)> {
        let mut assigned = false;
        for expression in expressions {
            let (next_initialized, expression_assigned) =
                expression_initialization_flow(expression, name, initialized)?;
            initialized = next_initialized;
            assigned |= expression_assigned;
        }
        Some((initialized, assigned))
    }

    match expression {
        Expression::Variable(variable) if variable == name && !initialized => None,
        Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name) =>
        {
            expression_initialization_flow(value, name, initialized)?;
            Some((true, true))
        }
        Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
            sequence([left.as_ref(), right.as_ref()], name, initialized)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            let (initialized, condition_assigned) =
                expression_initialization_flow(condition, name, initialized)?;
            let (true_initialized, true_assigned) =
                expression_initialization_flow(when_true, name, initialized)?;
            let (false_initialized, false_assigned) =
                expression_initialization_flow(when_false, name, initialized)?;
            Some((
                true_initialized && false_initialized,
                condition_assigned || true_assigned || false_assigned,
            ))
        }
        Expression::AggregateLiteral(elements) => sequence(elements, name, initialized),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::IndexedUpdateValue { value: operand }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand } => {
            expression_initialization_flow(operand, name, initialized)
        }
        Expression::PostStep { target, .. } => {
            let (initialized, assigned) =
                expression_initialization_flow(target, name, initialized)?;
            Some((initialized, assigned))
        }
        Expression::Index { base, index } => {
            sequence([base.as_ref(), index.as_ref()], name, initialized)
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_initialization_flow(base, name, initialized)
        }
        Expression::Call { arguments, .. } => sequence(arguments, name, initialized),
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            let (initialized, allocation_assigned) =
                expression_initialization_flow(allocation, name, initialized)?;
            let (initialized, arguments_assigned) = sequence(arguments, name, initialized)?;
            Some((initialized, allocation_assigned || arguments_assigned))
        }
        Expression::CallThrough { target, arguments } => {
            let (initialized, target_assigned) =
                expression_initialization_flow(target, name, initialized)?;
            let (initialized, arguments_assigned) = sequence(arguments, name, initialized)?;
            Some((initialized, target_assigned || arguments_assigned))
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            let (initialized, object_assigned) =
                expression_initialization_flow(object, name, initialized)?;
            let (initialized, arguments_assigned) = sequence(arguments, name, initialized)?;
            Some((initialized, object_assigned || arguments_assigned))
        }
        Expression::Assign { target, value } => {
            sequence([target.as_ref(), value.as_ref()], name, initialized)
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => Some((initialized, false)),
    }
}

pub(super) fn body_uses_local(statements: &[Statement], name: &str) -> bool {
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
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .iter()
                .chain(condition)
                .chain(step)
                .any(|expression| expression_reads_name(expression, name))
                || body_uses_local(body, name)
        }
        Statement::Switch { .. } => true,
        Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    })
}

/// Recognize a pointer phi whose true arm selects a frame aggregate and whose
/// false arm selects null. Build 163 keeps this short-lived condition value in
/// r4, alongside frame-address call arguments, rather than the r3 result lane.
pub(super) fn is_frame_address_null_select(function: &Function, name: &str) -> bool {
    let frame_aggregates: std::collections::HashSet<&str> = function
        .locals
        .iter()
        .filter_map(|local| {
            matches!(local.declared_type, Type::Struct { .. }).then_some(local.name.as_str())
        })
        .collect();
    if frame_aggregates.is_empty() {
        return false;
    }
    function.statements.iter().any(|statement| {
        let Statement::If {
            then_body,
            else_body,
            ..
        } = statement
        else {
            return false;
        };
        let selects_frame = then_body.iter().any(|statement| {
            matches!(statement,
                Statement::Assign {
                    name: assigned,
                    value: Expression::AddressOf { operand },
                } if assigned == name
                    && matches!(operand.as_ref(), Expression::Variable(frame)
                        if frame_aggregates.contains(frame.as_str())))
        });
        let selects_null = else_body.iter().any(|statement| {
            matches!(statement,
                Statement::Assign { name: assigned, value }
                    if assigned == name && crate::analysis::is_zero_literal(value))
        });
        selects_frame && selects_null
    })
}

fn expression_assigns_name(expression: &Expression, name: &str) -> bool {
    matches!(expression,
        Expression::Assign { target, .. }
            if matches!(target.as_ref(), Expression::Variable(assigned) if assigned == name))
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
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let survivors = std::collections::HashSet::new();
        let planned =
            plan_ephemeral_locals(&function, &survivors, &std::collections::HashSet::new())
                .unwrap();
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
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let planned = plan_ephemeral_locals(
            &function,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        )
        .expect("the branch-local float lifetime is valid");
        assert_eq!(planned.len(), 1);
    }

    #[test]
    fn accepts_a_value_defined_and_consumed_within_each_loop_iteration() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("cursor".into())),
                value: Box::new(Expression::Variable("head".into())),
            }),
            condition: Some(Expression::Variable("cursor".into())),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("cursor".into())),
                value: Box::new(Expression::Member {
                    base: Box::new(Expression::Variable("cursor".into())),
                    offset: 8,
                    member_type: Type::StructPointer { element_size: 0 },
                    index_stride: None,
                }),
            }),
            body: vec![
                Statement::Assign {
                    name: "temporary".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("temporary".into())],
                }),
            ],
        }];

        assert!(is_definitely_assigned_before_reads(&statements, "cursor"));
        assert!(is_definitely_assigned_before_reads(
            &statements,
            "temporary"
        ));
    }

    #[test]
    fn rejects_a_loop_local_read_before_its_iteration_assignment() {
        let statements = vec![Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(Expression::IntegerLiteral(1)),
            step: None,
            body: vec![
                Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("temporary".into())],
                }),
                Statement::Assign {
                    name: "temporary".into(),
                    value: Expression::IntegerLiteral(1),
                },
            ],
        }];

        assert!(!is_definitely_assigned_before_reads(
            &statements,
            "temporary"
        ));
    }

    #[test]
    fn accepts_an_embedded_comma_assignment_before_its_read() {
        let statements = vec![Statement::Expression(Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable("temporary".into())),
                    value: Box::new(Expression::FloatLiteral(1.0)),
                }),
                right: Box::new(Expression::Variable("temporary".into())),
            }),
            right: Box::new(Expression::FloatLiteral(2.0)),
        })];

        assert!(is_definitely_assigned_before_reads(
            &statements,
            "temporary"
        ));
    }

    #[test]
    fn plans_a_saved_home_for_an_embedded_comma_assignment() {
        let mut temporary = local("temporary", Expression::FloatLiteral(0.0));
        temporary.declared_type = Type::Float;
        temporary.initializer = None;
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![temporary],
            statements: vec![Statement::Expression(Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(Expression::Variable("temporary".into())),
                    value: Box::new(Expression::FloatLiteral(1.0)),
                }),
                right: Box::new(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::Variable("temporary".into())],
                }),
            })],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let locals: Vec<_> = function.locals.iter().collect();

        let plan = plan_deferred_saved_homes(&function, &locals)
            .expect("the embedded assignment establishes a saved value");
        assert_eq!(plan.group_count, 1);
        assert_eq!(plan.group("temporary"), 0);
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

        assert!(
            dead_ephemeral_float_locals(&[&temporary], std::slice::from_ref(&later)).is_empty()
        );
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
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        function.statements.push(Statement::Assign {
            name: "temporary".into(),
            value: Expression::IntegerLiteral(1),
        });

        assert!(plan_ephemeral_locals(
            &function,
            &std::collections::HashSet::new(),
            &std::collections::HashSet::new(),
        )
        .is_none());
    }

    #[test]
    fn accepts_an_assignment_after_a_goto_target() {
        let statements = vec![
            Statement::Goto("error".into()),
            Statement::Return(None),
            Statement::Label("error".into()),
            Statement::Assign {
                name: "callback".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::If {
                condition: Expression::Variable("callback".into()),
                then_body: Vec::new(),
                else_body: Vec::new(),
            },
        ];
        assert!(is_definitely_assigned_before_reads(&statements, "callback"));
    }

    #[test]
    fn retains_a_definition_shared_by_forward_gotos() {
        let statements = vec![
            Statement::Assign {
                name: "card".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::If {
                condition: Expression::IntegerLiteral(1),
                then_body: vec![Statement::Goto("error".into())],
                else_body: Vec::new(),
            },
            Statement::Return(None),
            Statement::Label("error".into()),
            Statement::Expression(Expression::Variable("card".into())),
        ];
        assert!(is_definitely_assigned_before_reads(&statements, "card"));
    }

    #[test]
    fn rejects_a_label_target_read_before_assignment() {
        let statements = vec![
            Statement::Goto("error".into()),
            Statement::Assign {
                name: "value".into(),
                value: Expression::IntegerLiteral(1),
            },
            Statement::Label("error".into()),
            Statement::Expression(Expression::Variable("value".into())),
        ];
        assert!(!is_definitely_assigned_before_reads(&statements, "value"));
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
            inline_asm_blocks: Vec::new(),
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
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let locals: Vec<_> = function.locals.iter().collect();
        let plan = plan_deferred_saved_homes(&function, &locals).unwrap();
        assert_eq!(plan.group_count, 2);
        assert_ne!(plan.group("first"), plan.group("second"));
    }

    #[test]
    fn reuses_the_most_recently_expired_deferred_home() {
        let mut early = local("early", Expression::IntegerLiteral(0));
        early.initializer = None;
        let mut late = local("late", Expression::IntegerLiteral(0));
        late.initializer = None;
        let mut reuse = local("reuse", Expression::IntegerLiteral(0));
        reuse.initializer = None;
        let function = Function {
            return_type: Type::Void,
            name: "compiled".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: vec![early, late, reuse],
            statements: vec![
                Statement::Assign {
                    name: "early".into(),
                    value: Expression::IntegerLiteral(1),
                },
                Statement::Assign {
                    name: "late".into(),
                    value: Expression::IntegerLiteral(2),
                },
                Statement::Expression(Expression::Variable("early".into())),
                Statement::Expression(Expression::Variable("late".into())),
                Statement::Assign {
                    name: "reuse".into(),
                    value: Expression::IntegerLiteral(3),
                },
                Statement::Expression(Expression::Variable("reuse".into())),
            ],
            guards: Vec::new(),
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };
        let locals: Vec<_> = function.locals.iter().collect();
        let plan = plan_deferred_saved_homes(&function, &locals).unwrap();
        assert_eq!(plan.group_count, 2);
        assert_eq!(plan.group("reuse"), plan.group("late"));
        assert_ne!(plan.group("reuse"), plan.group("early"));
    }
}
