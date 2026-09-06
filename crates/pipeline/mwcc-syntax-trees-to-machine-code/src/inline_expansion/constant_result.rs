//! Small register-bank effects whose scalar result is known at every exit.
//!
//! A polling loop needs statement composition; a straight-line update also
//! has a comma-expression summary for calls nested in conditions or values.

use mwcc_syntax_trees::{Expression, Function, LoopKind, Statement, Type};

#[cfg(test)]
#[path = "constant_result_tests.rs"]
mod tests;

pub(super) fn is_memory_transaction(function: &Function) -> bool {
    eligible(function)
        && !function.statements.is_empty()
        && function.statements.len() <= 4
        && function.statements.iter().all(|statement| match statement {
            Statement::Assign { name, value } => {
                function.locals.iter().any(|local| local.name == *name)
                    && scalar_expression(value, function)
            }
            Statement::Store { target, value } => {
                bank_element(target, function) && scalar_expression(value, function)
            }
            _ => false,
        })
        && function
            .statements
            .iter()
            .any(|statement| matches!(statement, Statement::Store { .. }))
}

pub(super) fn is_poll(function: &Function) -> bool {
    eligible(function)
        && function.locals.is_empty()
        && function.parameters.is_empty()
        && matches!(function.statements.as_slice(), [Statement::Loop {
            kind: LoopKind::While,
            initializer: None,
            condition: Some(condition),
            step: None,
            body,
        }] if body.is_empty() && scalar_expression(condition, function)
            && contains_bank_element(condition, function))
}

fn eligible(function: &Function) -> bool {
    matches!(function.return_type, Type::Int | Type::UnsignedInt)
        && matches!(
            function.return_expression,
            Some(Expression::IntegerLiteral(_))
        )
        && function.guards.is_empty()
        && function.asm_body.is_none()
        && function.inline_asm_blocks.is_empty()
        && function.parameters.len() <= 1
        && function
            .parameters
            .iter()
            .all(|parameter| matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt))
        && function.locals.len() <= 1
        && function.locals.iter().all(|local| {
            matches!(local.declared_type, Type::Int | Type::UnsignedInt)
                && !local.is_static
                && !local.is_volatile
                && local.array_length.is_none()
                && local
                    .initializer
                    .as_ref()
                    .is_some_and(|value| scalar_expression(value, function))
        })
}

fn bank_element(expression: &Expression, function: &Function) -> bool {
    matches!(expression, Expression::Index { base, index }
        if matches!(base.as_ref(), Expression::Variable(name)
            if !function.locals.iter().any(|local| local.name == *name)
                && !function.parameters.iter().any(|parameter| parameter.name == *name))
            && crate::analysis::constant_value(index).is_some())
}

fn scalar_expression(expression: &Expression, function: &Function) -> bool {
    if bank_element(expression, function) {
        return true;
    }
    match expression {
        Expression::IntegerLiteral(_) => true,
        Expression::Variable(name) => {
            function.locals.iter().any(|local| local.name == *name)
                || function
                    .parameters
                    .iter()
                    .any(|parameter| parameter.name == *name)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand } => scalar_expression(operand, function),
        Expression::Binary { left, right, .. } => {
            scalar_expression(left, function) && scalar_expression(right, function)
        }
        _ => false,
    }
}

fn contains_bank_element(expression: &Expression, function: &Function) -> bool {
    bank_element(expression, function)
        || match expression {
            Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => {
                contains_bank_element(operand, function)
            }
            Expression::Binary { left, right, .. } => {
                contains_bank_element(left, function) || contains_bank_element(right, function)
            }
            _ => false,
        }
}

/// Inserting an ordinary global name into a caller with a same-named local
/// would change its binding in the compact AST. Such calls stay out of line.
pub(super) fn captures_names(
    function: &Function,
    names: &std::collections::HashSet<String>,
) -> bool {
    fn expression(value: &Expression, names: &std::collections::HashSet<String>) -> bool {
        match value {
            Expression::Index { base, .. } => {
                matches!(base.as_ref(), Expression::Variable(name) if names.contains(name))
            }
            Expression::Binary { left, right, .. } => {
                expression(left, names) || expression(right, names)
            }
            Expression::Unary { operand, .. }
            | Expression::Cast { operand, .. }
            | Expression::IndexedUpdateValue { value: operand } => expression(operand, names),
            _ => false,
        }
    }
    function.locals.iter().any(|local| {
        local
            .initializer
            .as_ref()
            .is_some_and(|value| expression(value, names))
    }) || function.statements.iter().any(|statement| match statement {
        Statement::Store { target, value } => expression(target, names) || expression(value, names),
        Statement::Assign { value, .. } => expression(value, names),
        Statement::Loop {
            condition: Some(condition),
            ..
        } => expression(condition, names),
        _ => false,
    })
}

/// Expose a constant-result helper's effects at their original evaluation point, leaving the
/// known result in the surrounding scalar expression. The statement composer
/// still owns insertion of the loop and accounting for the eliminated call.
pub(super) fn expose_values(
    function: &Function,
    bodies: &std::collections::HashMap<String, Function>,
) -> Option<Function> {
    use mwcc_syntax_trees::{BinaryOperator as B, UnaryOperator as U};
    let bound: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|p| p.name.clone())
        .chain(function.locals.iter().map(|l| l.name.clone()))
        .collect();
    let words: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .filter(|p| matches!(p.parameter_type, Type::Int | Type::UnsignedInt))
        .map(|p| p.name.clone())
        .chain(
            function
                .locals
                .iter()
                .filter(|l| {
                    matches!(l.declared_type, Type::Int | Type::UnsignedInt) && !l.is_volatile
                })
                .map(|l| l.name.clone()),
        )
        .collect();
    fn extract(
        value: &Expression,
        bodies: &std::collections::HashMap<String, Function>,
        bound: &std::collections::HashSet<String>,
        words: &std::collections::HashSet<String>,
    ) -> Option<(Expression, Expression)> {
        match value {
            Expression::Call { name, arguments } => {
                let callee = bodies.get(name)?;
                if !(is_poll(callee) || is_memory_transaction(callee))
                    || arguments.len() != callee.parameters.len()
                    || captures_names(callee, bound)
                {
                    return None;
                }
                let result = crate::analysis::constant_value(callee.return_expression.as_ref()?)?;
                let result = crate::analysis::convert_integer_constant(result, callee.return_type)?;
                Some((value.clone(), Expression::IntegerLiteral(result)))
            }
            Expression::Unary {
                operator: U::LogicalNot,
                operand,
            } => {
                let (call, result) = extract(operand, bodies, bound, words)?;
                let value = crate::analysis::constant_value(&result)?;
                Some((call, Expression::IntegerLiteral(i64::from(value == 0))))
            }
            Expression::Binary {
                operator: operator @ (B::BitOr | B::BitAnd | B::BitXor | B::Add | B::Subtract),
                left,
                right,
            } => {
                // These operands are eager. Short-circuit operators and memory
                // reads remain in their original expression lowering path.
                let atom = |value: &Expression| {
                    matches!(value, Expression::IntegerLiteral(_))
                        || matches!(value, Expression::Variable(name) if words.contains(name))
                };
                let (call, left, right) = if atom(left) {
                    let (call, result) = extract(right, bodies, bound, words)?;
                    (call, left.as_ref().clone(), result)
                } else if atom(right) {
                    let (call, result) = extract(left, bodies, bound, words)?;
                    (call, result, right.as_ref().clone())
                } else {
                    return None;
                };
                let expression = if matches!(operator, B::BitOr | B::BitXor | B::Add | B::Subtract)
                    && crate::analysis::constant_value(&right) == Some(0)
                {
                    left
                } else {
                    Expression::Binary {
                        operator: *operator,
                        left: Box::new(left),
                        right: Box::new(right),
                    }
                };
                Some((call, expression))
            }
            _ => None,
        }
    }
    fn statements(
        source: &[Statement],
        bodies: &std::collections::HashMap<String, Function>,
        bound: &std::collections::HashSet<String>,
        words: &std::collections::HashSet<String>,
        changed: &mut bool,
    ) -> Vec<Statement> {
        let mut output = Vec::new();
        for statement in source {
            if let Statement::If {
                condition,
                then_body,
                else_body,
            } = statement
            {
                if let Some((call, condition)) = extract(condition, bodies, bound, words) {
                    *changed = true;
                    output.push(Statement::Expression(call));
                    if let Some(value) = crate::analysis::constant_value(&condition) {
                        let arm = if value == 0 { else_body } else { then_body };
                        output.extend(statements(arm, bodies, bound, words, changed));
                    } else {
                        output.push(Statement::If {
                            condition,
                            then_body: statements(then_body, bodies, bound, words, changed),
                            else_body: statements(else_body, bodies, bound, words, changed),
                        });
                    }
                    continue;
                }
            }
            let value = match statement {
                Statement::Assign { value, .. } | Statement::Return(Some(value)) => Some(value),
                _ => None,
            };
            if let Some((call, value)) =
                value.and_then(|value| extract(value, bodies, bound, words))
            {
                *changed = true;
                output.push(Statement::Expression(call));
                match statement {
                    Statement::Assign { name, .. } => {
                        if !matches!(&value, Expression::Variable(read) if read == name) {
                            output.push(Statement::Assign {
                                name: name.clone(),
                                value,
                            });
                        }
                    }
                    _ => output.push(Statement::Return(Some(value))),
                }
                continue;
            }
            output.push(match statement {
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => Statement::If {
                    condition: condition.clone(),
                    then_body: statements(then_body, bodies, bound, words, changed),
                    else_body: statements(else_body, bodies, bound, words, changed),
                },
                Statement::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body: statements(body, bodies, bound, words, changed),
                },
                other => other.clone(),
            });
        }
        output
    }
    let mut changed = false;
    let mut expanded = function.clone();
    expanded.statements = statements(&function.statements, bodies, &bound, &words, &mut changed);
    let exposed_return = function
        .return_expression
        .as_ref()
        .and_then(|value| extract(value, bodies, &bound, &words));
    let exposes_guard = function.guards.iter().any(|guard| {
        extract(&guard.condition, bodies, &bound, &words).is_some()
            || extract(&guard.value, bodies, &bound, &words).is_some()
    });
    if !function.guards.is_empty() && (exposes_guard || exposed_return.is_some()) {
        // Guards are the ordered exit chain after ordinary statements. Keep
        // every preceding exit in front of any newly exposed helper effect.
        let guards: Vec<Statement> = function
            .guards
            .iter()
            .map(|guard| Statement::If {
                condition: guard.condition.clone(),
                then_body: vec![Statement::Return(Some(guard.value.clone()))],
                else_body: Vec::new(),
            })
            .collect();
        expanded
            .statements
            .extend(statements(&guards, bodies, &bound, &words, &mut changed));
        expanded.guards.clear();
        changed = true;
    }
    if expanded.guards.is_empty() {
        if let Some((call, value)) = exposed_return {
            expanded.statements.push(Statement::Expression(call));
            expanded.return_expression = Some(value);
            changed = true;
        }
    }
    if changed {
        if let Some(index) = expanded
            .statements
            .iter()
            .position(|statement| matches!(statement, Statement::Return(_)))
        {
            let Statement::Return(value) = expanded.statements[index].clone() else {
                unreachable!()
            };
            expanded.return_expression = value;
            expanded.statements.truncate(index);
            expanded.guards.clear();
        }
    }
    if changed && expanded.inline_asm_blocks.is_empty() && expanded.asm_body.is_none() {
        // A status accumulator can become immutable after its no-op update
        // disappears. Reuse the composer's escape proof before replacing it.
        let stable = super::safety::stable_local_values(&expanded);
        let constants: std::collections::HashMap<String, Expression> = expanded
            .locals
            .iter()
            .filter(|local| {
                local.declared_type == Type::Int
                    && !local.is_volatile
                    && !local.is_static
                    && local.array_length.is_none()
                    && stable.contains(&local.name)
            })
            .filter_map(|local| match &local.initializer {
                Some(Expression::IntegerLiteral(value)) => Some((
                    local.name.clone(),
                    Expression::IntegerLiteral(*value as i32 as i64),
                )),
                _ => None,
            })
            .collect();
        if !constants.is_empty() {
            expanded
                .locals
                .retain(|local| !constants.contains_key(&local.name));
            for local in &mut expanded.locals {
                if let Some(value) = &local.initializer {
                    local.initializer = Some(super::substitution::substitute_expression(
                        value, &constants,
                    ));
                }
            }
            expanded.statements = expanded
                .statements
                .iter()
                .map(|statement| super::substitution::substitute_statement(statement, &constants))
                .collect();
            for guard in &mut expanded.guards {
                guard.condition =
                    super::substitution::substitute_expression(&guard.condition, &constants);
                guard.value = super::substitution::substitute_expression(&guard.value, &constants);
            }
            if let Some(value) = &expanded.return_expression {
                let mut value = super::substitution::substitute_expression(value, &constants);
                if let Expression::Unary {
                    operator: U::LogicalNot,
                    operand,
                } = &value
                {
                    if let Some(constant) = crate::analysis::constant_value(operand) {
                        value = Expression::IntegerLiteral(i64::from(constant == 0));
                    }
                }
                expanded.return_expression = Some(value);
            }
        }
    }
    changed.then_some(expanded)
}

/// At caller entry, an inline initializer can keep its declaration home. This
/// preserves the register-bank transaction recognizers' load-once input form.
pub(super) fn restore_entry_initializers(
    function: &mut Function,
    callees: &[String],
    original_names: &std::collections::HashSet<String>,
) {
    while let Some(Statement::Assign { name, value }) = function.statements.first() {
        if original_names.contains(name)
            || !callees
                .iter()
                .any(|callee| name.starts_with(&format!("__mwcc_inline_{callee}_")))
        {
            break;
        }
        let Some(local) = function
            .locals
            .iter_mut()
            .find(|local| local.name == *name && local.initializer.is_none())
        else {
            break;
        };
        local.initializer = Some(value.clone());
        function.statements.remove(0);
    }
}
