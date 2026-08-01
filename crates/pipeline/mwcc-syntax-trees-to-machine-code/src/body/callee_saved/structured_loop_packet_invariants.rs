//! Repeated source-proven packet words that are invariant across a loop.
//!
//! Display-list macros often spell the same computed command word more than
//! once in one packet run. MWCC retains that word across the loop rather than
//! rebuilding its arithmetic for every store. This pass deliberately owns only
//! whole unsigned-word stores made from pure integer arithmetic over locals
//! that are assigned before, and never written by, the loop.

#[allow(unused_imports)]
use super::*;

pub(super) fn hoist_repeated_packet_words(function: &Function) -> Option<Function> {
    let address_taken = crate::frame::collect_address_taken(function);
    let stable_names: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(
            function
                .locals
                .iter()
                .filter(|local| !local.is_volatile)
                .map(|local| local.name.clone()),
        )
        .filter(|name| !address_taken.contains(name))
        .collect();
    let mut assigned: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(
            function
                .locals
                .iter()
                .filter(|local| local.initializer.is_some())
                .map(|local| local.name.clone()),
        )
        .collect();
    let mut used_names: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let (statements, changed) = hoist_in_sequence(
        &function.statements,
        &mut assigned,
        &stable_names,
        &mut used_names,
        &mut declarations,
        &mut next_name,
    );
    changed.then(|| {
        let mut hoisted = function.clone();
        hoisted.locals.extend(declarations);
        hoisted.statements = statements;
        hoisted
    })
}

fn hoist_in_sequence(
    statements: &[Statement],
    assigned: &mut std::collections::HashSet<String>,
    stable_names: &std::collections::HashSet<String>,
    used_names: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, bool) {
    let mut output = Vec::with_capacity(statements.len());
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Assign { name, .. } => {
                output.push(statement.clone());
                assigned.insert(name.clone());
            }
            Statement::Loop {
                kind,
                initializer: None,
                condition,
                step,
                body,
            } => {
                let mut body_assigned = assigned.clone();
                let (body, nested_changed) = hoist_in_sequence(
                    body,
                    &mut body_assigned,
                    stable_names,
                    used_names,
                    declarations,
                    next_name,
                );
                let (prefix, body, loop_changed) = hoist_loop_words(
                    &body,
                    assigned,
                    stable_names,
                    used_names,
                    declarations,
                    next_name,
                );
                output.extend(prefix);
                output.push(Statement::Loop {
                    kind: *kind,
                    initializer: None,
                    condition: condition.clone(),
                    step: step.clone(),
                    body,
                });
                changed |= nested_changed || loop_changed;
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let mut then_assigned = assigned.clone();
                let mut else_assigned = assigned.clone();
                let (then_body, then_changed) = hoist_in_sequence(
                    then_body,
                    &mut then_assigned,
                    stable_names,
                    used_names,
                    declarations,
                    next_name,
                );
                let (else_body, else_changed) = hoist_in_sequence(
                    else_body,
                    &mut else_assigned,
                    stable_names,
                    used_names,
                    declarations,
                    next_name,
                );
                *assigned = then_assigned
                    .intersection(&else_assigned)
                    .cloned()
                    .collect();
                output.push(Statement::If {
                    condition: condition.clone(),
                    then_body,
                    else_body,
                });
                changed |= then_changed || else_changed;
            }
            _ => output.push(statement.clone()),
        }
    }
    (output, changed)
}

fn hoist_loop_words(
    body: &[Statement],
    assigned: &std::collections::HashSet<String>,
    stable_names: &std::collections::HashSet<String>,
    used_names: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, Vec<Statement>, bool) {
    let written = assigned_names(body);
    let candidates: Vec<&Expression> = body
        .iter()
        .filter_map(|statement| {
            let Statement::Store { target, value } = statement else {
                return None;
            };
            matches!(
                target,
                Expression::Member {
                    member_type: Type::UnsignedInt,
                    index_stride: None,
                    ..
                }
            )
            .then_some(value)
        })
        .collect();
    let mut groups: Vec<&Expression> = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let Some((reads, operation_count)) = pure_integer_arithmetic(candidate) else {
            continue;
        };
        if reads.is_empty()
            || operation_count < 8
            || !reads.iter().all(|name| assigned.contains(name))
            || !reads.iter().all(|name| stable_names.contains(name))
            || reads.iter().any(|name| written.contains(name))
            || groups
                .iter()
                .any(|existing| crate::analysis::structurally_equal(existing, candidate))
            || !candidates[index + 1..]
                .iter()
                .any(|other| crate::analysis::structurally_equal(candidate, other))
        {
            continue;
        }
        groups.push(candidate);
    }

    let mut prefix = Vec::with_capacity(groups.len());
    let mut replacements = Vec::with_capacity(groups.len());
    for value in groups {
        let name = fresh_name(used_names, next_name);
        declarations.push(unsigned_local(&name));
        prefix.push(Statement::Assign {
            name: name.clone(),
            value: super::structured_loop_packet_algebra::simplify(value),
        });
        replacements.push((value, name));
    }
    let body: Vec<_> = body
        .iter()
        .map(|statement| match statement {
            Statement::Store { target, value } => Statement::Store {
                target: target.clone(),
                value: super::structured_loop_packet_invariant_rewrite::replace(
                    value,
                    &replacements,
                ),
            },
            _ => statement.clone(),
        })
        .collect();

    let eligible = |expression: &Expression| {
        let Some((reads, operation_count)) = pure_integer_arithmetic(expression) else {
            return false;
        };
        if !reads.iter().all(|name| assigned.contains(name))
            || !reads.iter().all(|name| stable_names.contains(name))
            || reads.iter().any(|name| written.contains(name))
        {
            return false;
        }
        !reads.is_empty() && operation_count >= 2
    };
    let mut fragments: Vec<&Expression> = Vec::new();
    for statement in &body {
        let Statement::Store { target, value } = statement else {
            continue;
        };
        if !matches!(
            target,
            Expression::Member {
                member_type: Type::UnsignedInt,
                index_stride: None,
                ..
            }
        ) {
            continue;
        }
        let mut maximal = Vec::new();
        if crate::analysis::constant_value(value)
            .is_some_and(|constant| constant != 0 && i16::try_from(constant).is_err())
        {
            maximal.push(value);
        } else {
            super::structured_loop_packet_invariant_rewrite::collect_maximal(
                value,
                &eligible,
                &mut maximal,
            );
        }
        for candidate in maximal {
            if !fragments
                .iter()
                .any(|existing| crate::analysis::structurally_equal(existing, candidate))
            {
                fragments.push(candidate);
            }
        }
    }
    let mut fragment_replacements = Vec::with_capacity(fragments.len());
    let common = repeated_invariant_subexpressions(&fragments);
    let mut common_replacements = Vec::with_capacity(common.len());
    for value in common {
        let name = fresh_name(used_names, next_name);
        declarations.push(unsigned_local(&name));
        prefix.push(Statement::Assign {
            name: name.clone(),
            value: value.clone(),
        });
        common_replacements.push((value, name));
    }
    for value in fragments {
        let name = fresh_name(used_names, next_name);
        declarations.push(unsigned_local(&name));
        prefix.push(Statement::Assign {
            name: name.clone(),
            value: super::structured_loop_packet_invariant_rewrite::replace(
                value,
                &common_replacements,
            ),
        });
        fragment_replacements.push((value, name));
    }
    let body: Vec<_> = body
        .iter()
        .map(|statement| match statement {
            Statement::Store { target, value } => Statement::Store {
                target: target.clone(),
                value: super::structured_loop_packet_invariant_rewrite::replace(
                    value,
                    &fragment_replacements,
                ),
            },
            _ => statement.clone(),
        })
        .collect();
    let (body, dynamic_changed) =
        name_dynamic_shallow_fragments(&body, &written, used_names, declarations, next_name);
    let (body, zero_changed) = if super::structured_loop_packet_zero::has_repeated_zero_words(&body)
    {
        let name = fresh_name(used_names, next_name);
        declarations.push(unsigned_local(&name));
        (
            super::structured_loop_packet_zero::rewrite(&body, &name),
            true,
        )
    } else {
        (body, false)
    };
    let changed = !prefix.is_empty();
    (prefix, body, changed || dynamic_changed || zero_changed)
}

fn repeated_invariant_subexpressions<'a>(fragments: &[&'a Expression]) -> Vec<&'a Expression> {
    let mut candidates: Vec<(&Expression, usize)> = Vec::new();
    for (index, fragment) in fragments.iter().enumerate() {
        for candidate in crate::analysis::computed_subexpressions(fragment) {
            let Some((_, operations)) = pure_integer_arithmetic(candidate) else {
                continue;
            };
            if operations < 2
                || candidates
                    .iter()
                    .any(|(existing, _)| crate::analysis::structurally_equal(existing, candidate))
                || !fragments[index + 1..].iter().any(|other| {
                    crate::analysis::computed_subexpressions(other)
                        .iter()
                        .any(|nested| crate::analysis::structurally_equal(candidate, nested))
                })
            {
                continue;
            }
            candidates.push((candidate, operations));
        }
    }
    candidates.sort_by_key(|(_, operations)| std::cmp::Reverse(*operations));

    let mut selected: Vec<&'a Expression> = Vec::new();
    for (candidate, _) in candidates {
        if selected.iter().any(|existing| {
            computation_contains(existing, candidate) || computation_contains(candidate, existing)
        }) {
            continue;
        }
        selected.push(candidate);
    }
    selected
}

fn computation_contains(expression: &Expression, candidate: &Expression) -> bool {
    crate::analysis::computed_subexpressions(expression)
        .into_iter()
        .any(|nested| crate::analysis::structurally_equal(nested, candidate))
}

fn name_dynamic_shallow_fragments(
    body: &[Statement],
    written: &std::collections::HashSet<String>,
    used_names: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, bool) {
    let mut output = Vec::with_capacity(body.len());
    let mut named: Vec<(&Expression, String, Vec<String>, usize)> = Vec::new();
    let mut changed = false;
    for (statement_index, statement) in body.iter().enumerate() {
        let Statement::Store { target, value } = statement else {
            output.push(statement.clone());
            continue;
        };
        if !matches!(
            target,
            Expression::Member {
                member_type: Type::UnsignedInt,
                index_stride: None,
                ..
            }
        ) {
            output.push(statement.clone());
            continue;
        }
        let eligible = |expression: &Expression| {
            let Some((reads, _)) = pure_integer_arithmetic(expression) else {
                return false;
            };
            !reads.is_empty()
                && reads.iter().any(|name| written.contains(name))
                && crate::expressions::is_shallow_packed_shift_mask_expression(expression)
        };
        let mut fragments = Vec::new();
        super::structured_loop_packet_invariant_rewrite::collect_maximal(
            value,
            &eligible,
            &mut fragments,
        );
        if fragments.is_empty() {
            output.push(statement.clone());
            continue;
        }
        let mut replacements = Vec::with_capacity(fragments.len());
        for fragment in fragments {
            if let Some((_, name, _, _)) = named.iter().find(|(candidate, _, reads, named_at)| {
                if !crate::analysis::structurally_equal(candidate, fragment) {
                    return false;
                }
                let between = if *named_at < statement_index {
                    &body[*named_at + 1..statement_index]
                } else {
                    &[]
                };
                let reassigned = assigned_names(between);
                !reads.iter().any(|read| reassigned.contains(read))
            }) {
                replacements.push((fragment, name.clone()));
                continue;
            }
            let name = fresh_name(used_names, next_name);
            let reads = pure_integer_arithmetic(fragment)
                .map(|(reads, _)| reads)
                .unwrap_or_default();
            declarations.push(unsigned_local(&name));
            output.push(Statement::Assign {
                name: name.clone(),
                value: fragment.clone(),
            });
            named.push((fragment, name.clone(), reads, statement_index));
            replacements.push((fragment, name));
        }
        output.push(Statement::Store {
            target: target.clone(),
            value: super::structured_loop_packet_invariant_rewrite::replace(value, &replacements),
        });
        changed = true;
    }
    (output, changed)
}

fn pure_integer_arithmetic(expression: &Expression) -> Option<(Vec<String>, usize)> {
    fn visit(
        expression: &Expression,
        reads: &mut std::collections::HashSet<String>,
    ) -> Option<usize> {
        match expression {
            Expression::IntegerLiteral(_) => Some(0),
            Expression::Variable(name) => {
                reads.insert(name.clone());
                Some(0)
            }
            Expression::Binary {
                operator:
                    BinaryOperator::Add
                    | BinaryOperator::Subtract
                    | BinaryOperator::Multiply
                    | BinaryOperator::BitAnd
                    | BinaryOperator::BitOr
                    | BinaryOperator::BitXor,
                left,
                right,
            } => Some(visit(left, reads)? + visit(right, reads)? + 1),
            Expression::Binary {
                operator: BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight,
                left,
                right,
            } if matches!(right.as_ref(), Expression::IntegerLiteral(0..=31)) => {
                Some(visit(left, reads)? + 1)
            }
            Expression::Unary {
                operator: UnaryOperator::BitNot,
                operand,
            } => Some(visit(operand, reads)? + 1),
            Expression::Cast {
                target_type:
                    Type::Char
                    | Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Int
                    | Type::UnsignedInt,
                operand,
            } => Some(visit(operand, reads)? + 1),
            _ => None,
        }
    }

    let mut reads = std::collections::HashSet::new();
    let operations = visit(expression, &mut reads)?;
    Some((reads.into_iter().collect(), operations))
}

fn assigned_names(statements: &[Statement]) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    collect_assigned_names(statements, &mut names);
    names
}

fn collect_assigned_names(statements: &[Statement], names: &mut std::collections::HashSet<String>) {
    for statement in statements {
        match statement {
            Statement::Assign { name, value } => {
                names.insert(name.clone());
                collect_expression_assignments(value, names);
            }
            Statement::Store { target, value } => {
                if let Expression::Variable(name) = target {
                    names.insert(name.clone());
                }
                collect_expression_assignments(target, names);
                collect_expression_assignments(value, names);
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                collect_expression_assignments(condition, names);
                collect_assigned_names(then_body, names);
                collect_assigned_names(else_body, names);
            }
            Statement::Loop {
                initializer,
                condition,
                step,
                body,
                ..
            } => {
                for expression in [initializer, condition, step].into_iter().flatten() {
                    collect_expression_assignments(expression, names);
                }
                collect_assigned_names(body, names);
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                collect_expression_assignments(scrutinee, names);
                for arm in arms {
                    match &arm.body {
                        mwcc_syntax_trees::ArmBody::Return(expression) => {
                            collect_expression_assignments(expression, names);
                        }
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            collect_assigned_names(statements, names);
                        }
                    }
                }
                match default {
                    Some(mwcc_syntax_trees::ArmBody::Return(expression)) => {
                        collect_expression_assignments(expression, names);
                    }
                    Some(mwcc_syntax_trees::ArmBody::Statements(statements)) => {
                        collect_assigned_names(statements, names);
                    }
                    None => {}
                }
            }
            Statement::Expression(expression) | Statement::Return(Some(expression)) => {
                collect_expression_assignments(expression, names);
            }
            _ => {}
        }
    }
}

fn collect_expression_assignments(
    expression: &Expression,
    names: &mut std::collections::HashSet<String>,
) {
    match expression {
        Expression::Assign { target, value } => {
            if let Expression::Variable(name) = target.as_ref() {
                names.insert(name.clone());
            }
            collect_expression_assignments(target, names);
            collect_expression_assignments(value, names);
        }
        Expression::PostStep { target, .. } => {
            if let Expression::Variable(name) = target.as_ref() {
                names.insert(name.clone());
            }
            collect_expression_assignments(target, names);
        }
        Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
            collect_expression_assignments(left, names);
            collect_expression_assignments(right, names);
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::IndexedUpdateValue { value: operand } => {
            collect_expression_assignments(operand, names);
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            collect_expression_assignments(condition, names);
            collect_expression_assignments(when_true, names);
            collect_expression_assignments(when_false, names);
        }
        Expression::BitFieldRead {
            extracted, storage, ..
        } => {
            collect_expression_assignments(extracted, names);
            collect_expression_assignments(storage, names);
        }
        Expression::Index { base, index } => {
            collect_expression_assignments(base, names);
            collect_expression_assignments(index, names);
        }
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            collect_expression_assignments(base, names);
        }
        Expression::Call { arguments, .. } => {
            for argument in arguments {
                collect_expression_assignments(argument, names);
            }
        }
        Expression::CallThrough { target, arguments } => {
            collect_expression_assignments(target, names);
            for argument in arguments {
                collect_expression_assignments(argument, names);
            }
        }
        Expression::VirtualCall {
            object, arguments, ..
        } => {
            collect_expression_assignments(object, names);
            for argument in arguments {
                collect_expression_assignments(argument, names);
            }
        }
        Expression::ConstructedNew {
            allocation,
            arguments,
            ..
        } => {
            collect_expression_assignments(allocation, names);
            for argument in arguments {
                collect_expression_assignments(argument, names);
            }
        }
        Expression::AggregateLiteral(elements) => {
            for element in elements {
                collect_expression_assignments(element, names);
            }
        }
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_)
        | Expression::Variable(_)
        | Expression::CompoundLiteral { .. } => {}
    }
}

fn fresh_name(used_names: &mut std::collections::HashSet<String>, next_name: &mut usize) -> String {
    loop {
        let name = format!("__mwcc_packet_word_{}", *next_name);
        *next_name += 1;
        if used_names.insert(name.clone()) {
            return name;
        }
    }
}

fn unsigned_local(name: &str) -> LocalDeclaration {
    LocalDeclaration {
        declared_type: Type::UnsignedInt,
        name: name.to_owned(),
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

#[cfg(test)]
#[path = "structured_loop_packet_invariants_tests.rs"]
mod tests;
