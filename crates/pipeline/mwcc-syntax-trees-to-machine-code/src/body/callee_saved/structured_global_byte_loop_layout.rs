//! Saved-register roles for a retained global-byte decoder loop.
//!
//! The address-invariant and byte-cursor normalizations expose all semantic
//! lifetimes, but generic first-definition order does not reproduce MWCC's
//! dense loop window.  This planner recognizes the normalized source shape and
//! assigns homes by role: global base, cursor, count address, call result,
//! induction variable, input cursors, source parameter, and invariant address.

use super::*;

pub(super) struct StructuredGlobalByteLoopLayout {
    preferences: std::collections::HashMap<String, u8>,
}

impl StructuredGlobalByteLoopLayout {
    pub(super) fn plan(
        function: &Function,
        saved_parameters: &[&mwcc_syntax_trees::Parameter],
        deferred: &[&LocalDeclaration],
    ) -> Option<Self> {
        let [parameter] = saved_parameters else {
            return None;
        };
        if deferred.len() != 6 {
            return None;
        }
        let deferred_names = deferred
            .iter()
            .map(|local| local.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let address = unique_prefixed_name(deferred, "__mwcc_loop_address_")?;
        let cursor = unique_prefixed_name(deferred, "__mwcc_global_byte_cursor_")?;

        let mut ordinary = Vec::new();
        let mut loop_statement = None;
        let mut cursor_global = None;
        for statement in &function.statements {
            match statement {
                Statement::Assign { name, value } if name == cursor => {
                    cursor_global = cursor_initializer_global(value);
                }
                Statement::Assign { name, value }
                    if deferred_names.contains(name.as_str())
                        && name != address
                        && name != cursor =>
                {
                    ordinary.push((name.as_str(), value));
                }
                Statement::Loop { .. } => {
                    loop_statement = Some(statement);
                    break;
                }
                _ => {}
            }
        }
        let [(first, first_value), (second, second_value), (result, result_value)] =
            ordinary.as_slice()
        else {
            return None;
        };
        if expression_contains_call(first_value)
            || expression_contains_call(second_value)
            || !expression_contains_call(result_value)
        {
            return None;
        }

        let global = cursor_global?;
        let index = normalized_loop_index(loop_statement?, cursor, global)?;
        if !deferred_names.contains(index)
            || [*first, *second, *result, index, address, cursor]
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != 6
        {
            return None;
        }

        let preferences = [
            (cursor.to_owned(), 30),
            ((*result).to_owned(), 28),
            (index.to_owned(), 27),
            ((*first).to_owned(), 26),
            ((*second).to_owned(), 25),
            (parameter.name.clone(), 24),
            (address.to_owned(), 23),
        ]
        .into_iter()
        .collect();
        Some(Self { preferences })
    }

    pub(super) fn preference(&self, name: &str) -> Option<u8> {
        self.preferences.get(name).copied()
    }

    pub(super) fn member_cache_preference(&self, index: usize) -> Option<u8> {
        (index == 0).then_some(29)
    }
}

fn unique_prefixed_name<'a>(
    deferred: &'a [&LocalDeclaration],
    prefix: &str,
) -> Option<&'a str> {
    let mut matches = deferred
        .iter()
        .filter(|local| local.name.starts_with(prefix))
        .map(|local| local.name.as_str());
    let name = matches.next()?;
    matches.next().is_none().then_some(name)
}

fn cursor_initializer_global(expression: &Expression) -> Option<&str> {
    let Expression::Cast { operand, .. } = expression else {
        return None;
    };
    let Expression::AddressOf { operand } = operand.as_ref() else {
        return None;
    };
    let Expression::Variable(global) = operand.as_ref() else {
        return None;
    };
    Some(global)
}

fn expression_contains_call(expression: &Expression) -> bool {
    match expression {
        Expression::Call { .. } => true,
        Expression::Cast { operand, .. } => expression_contains_call(operand),
        _ => false,
    }
}

fn normalized_loop_index<'a>(
    statement: &'a Statement,
    cursor: &str,
    global: &str,
) -> Option<&'a str> {
    let Statement::Loop {
        initializer: Some(Expression::Assign { target, value }),
        condition: Some(Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        }),
        body,
        ..
    } = statement
    else {
        return None;
    };
    let Expression::Variable(initialized) = target.as_ref() else {
        return None;
    };
    if constant_value(value) != Some(0)
        || !matches!(left.as_ref(), Expression::Variable(name) if name == initialized)
        || !matches!(right.as_ref(), Expression::Member { base, .. }
            if matches!(base.as_ref(), Expression::Variable(name) if name == global))
    {
        return None;
    }
    let Statement::Switch {
        scrutinee: Expression::Index { base, .. },
        ..
    } = body.first()?
    else {
        return None;
    };
    matches!(base.as_ref(), Expression::Variable(name) if name == cursor)
        .then_some(initialized.as_str())
}
