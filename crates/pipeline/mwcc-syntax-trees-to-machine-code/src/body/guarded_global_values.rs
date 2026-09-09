//! Explicit value captures at guarded call entries.
//!
//! Optimized MWCC carries a scalar condition value into call-free arguments
//! evaluated before a nested call. A repeated input of a computed argument
//! shares that capture with the other argument positions. O0 reloads instead.
use super::*;
use std::collections::{HashMap, HashSet};

pub(super) fn capture(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile: &HashSet<String>,
    returns: &HashMap<String, Type>,
    behavior: &mwcc_versions::Behavior,
) -> Option<Function> {
    if !function.guards.is_empty() || !function.inline_asm_blocks.is_empty() {
        return None;
    }
    let mut result = function.clone();
    let mut names: HashSet<_> = globals.keys().chain(returns.keys()).cloned().collect();
    names.extend(function.locals.iter().map(|l| l.name.clone()));
    names.extend(function.parameters.iter().map(|p| p.name.clone()));
    let mut eligible = globals.clone();
    for name in function
        .locals
        .iter()
        .map(|l| &l.name)
        .chain(function.parameters.iter().map(|p| &p.name))
    {
        eligible.remove(name);
    }
    let mut locals = Vec::new();
    rewrite_sequence(
        &mut result.statements,
        &eligible,
        volatile,
        returns,
        &mut names,
        &mut locals,
        behavior,
        true,
    );
    if locals.is_empty() {
        return None;
    }
    result.locals.extend(locals);
    Some(result)
}

fn word(ty: Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
    )
}
fn leaf(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => leaf(operand),
        _ => None,
    }
}
fn call_type(expression: &Expression, returns: &HashMap<String, Type>) -> Option<Type> {
    match expression {
        Expression::Call { name, arguments } if arguments.is_empty() => {
            returns.get(name).copied().filter(|ty| word(*ty))
        }
        Expression::Cast {
            target_type,
            operand,
        } if word(*target_type) && call_type(operand, returns).is_some() => Some(*target_type),
        _ => None,
    }
}
fn replace(expression: &Expression, name: &str, replacement: &Expression) -> Expression {
    super::callee_saved::rewrite_structured_expression(expression, &mut |e| {
        matches!(e, Expression::Variable(n) if n == name).then(|| replacement.clone())
    })
}
fn local(ty: Type, names: &mut HashSet<String>, locals: &mut Vec<LocalDeclaration>) -> String {
    let mut i = locals.len();
    let name = loop {
        let n = format!("__mwcc_guard_value_{i}");
        if names.insert(n.clone()) {
            break n;
        }
        i += 1;
    };
    locals.push(LocalDeclaration {
        declared_type: ty,
        name: name.clone(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: vec![],
        is_const: false,
        attribute_alignment: None,
        row_bytes: None,
    });
    name
}

fn rewrite_sequence(
    statements: &mut Vec<Statement>,
    globals: &HashMap<String, Type>,
    volatile: &HashSet<String>,
    returns: &HashMap<String, Type>,
    names: &mut HashSet<String>,
    locals: &mut Vec<LocalDeclaration>,
    behavior: &mwcc_versions::Behavior,
    entry: bool,
) {
    let mut output = Vec::new();
    for mut statement in std::mem::take(statements) {
        if let Statement::If {
            condition,
            then_body,
            else_body,
        } = &mut statement
        {
            let candidate = match &*condition {
                Expression::Binary {
                    operator,
                    left,
                    right,
                } if is_comparison(*operator) => [
                    (left.as_ref(), right.as_ref()),
                    (right.as_ref(), left.as_ref()),
                ]
                .into_iter()
                .find_map(|(value, other)| {
                    leaf(value)
                        .filter(|n| {
                            globals.get(*n).copied().is_some_and(word) && !volatile.contains(*n)
                        })
                        .filter(|_| {
                            constant_value(other).is_some() || call_type(other, returns).is_some()
                        })
                        .map(|n| (n.to_owned(), Some(other.clone())))
                }),
                value => leaf(value)
                    .filter(|n| {
                        globals.get(*n).copied().is_some_and(word) && !volatile.contains(*n)
                    })
                    .map(|n| (n.to_owned(), None)),
            };
            if let Some((global, other)) = candidate {
                let chain = behavior.retain_guarded_globals_across_calls
                    && then_body
                        .first()
                        .is_some_and(|first| chain_uses_global(first, &global));
                if chain {
                    let value = capture_condition(
                        condition,
                        &global,
                        other.as_ref(),
                        globals,
                        returns,
                        names,
                        locals,
                        &mut output,
                        behavior.optimization >= mwcc_versions::Optimization::O4,
                    );
                    then_body[0] = super::callee_saved::rewrite_structured_statement(
                        &then_body[0],
                        &mut |e| {
                            matches!(e, Expression::Variable(n) if n == &global)
                                .then(|| value.clone())
                        },
                    );
                } else if let Some(Statement::Expression(Expression::Call { arguments, .. })) =
                    then_body.first_mut().filter(|statement| matches!(statement, Statement::Expression(expression) if capture_expression(expression)))
                {
                    let last_call = arguments.iter().rposition(expression_has_call);
                    let shared = crate::expressions::nested_word_arguments::shared_computed_global(
                        arguments, &global,
                    );
                    let selected: Vec<_> = arguments
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            expression_reads_name(arg, &global)
                                && (shared || last_call.is_none_or(|last| index > last))
                        })
                        .collect();
                    if selected.iter().any(|selected| *selected) {
                        let hoist = behavior.retain_guarded_globals_across_calls
                            && (behavior.optimization >= mwcc_versions::Optimization::O4
                                || (entry && output.is_empty()));
                        let value = capture_condition(
                            condition,
                            &global,
                            other.as_ref(),
                            globals,
                            returns,
                            names,
                            locals,
                            &mut output,
                            hoist,
                        );
                        for (arg, selected) in arguments.iter_mut().zip(selected) {
                            if selected {
                                *arg = replace(arg, &global, &value);
                            }
                        }
                    }
                }
            }
            rewrite_sequence(
                then_body, globals, volatile, returns, names, locals, behavior, false,
            );
            rewrite_sequence(
                else_body, globals, volatile, returns, names, locals, behavior, false,
            );
        }
        output.push(statement);
    }
    *statements = output;
}

fn chain_uses_global(statement: &Statement, global: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    let Some(Statement::Expression(Expression::Call { arguments, .. })) = then_body.first() else {
        return false;
    };
    else_body.is_empty()
        && capture_expression(condition)
        && arguments.iter().all(capture_expression)
        && expression_reads_name(condition, global)
        && arguments.iter().any(|a| expression_reads_name(a, global))
        && !arguments.iter().any(expression_has_call)
        && then_body
            .iter()
            .skip(1)
            .all(|s| matches!(s, Statement::Return(None)))
}

#[allow(clippy::too_many_arguments)]
fn capture_condition(
    condition: &mut Expression,
    global: &str,
    other: Option<&Expression>,
    globals: &HashMap<String, Type>,
    returns: &HashMap<String, Type>,
    names: &mut HashSet<String>,
    locals: &mut Vec<LocalDeclaration>,
    output: &mut Vec<Statement>,
    hoist: bool,
) -> Expression {
    let mut prefix = Vec::new();
    if let Some(other) = other.filter(|e| expression_has_call(e)) {
        let name = local(
            call_type(other, returns).expect("known word call"),
            names,
            locals,
        );
        prefix.push(Statement::Assign {
            name: name.clone(),
            value: other.clone(),
        });
        *condition = super::callee_saved::rewrite_structured_expression(condition, &mut |e| {
            structurally_equal(e, other).then(|| Expression::Variable(name.clone()))
        });
    }
    let name = local(globals[global], names, locals);
    let assign = Statement::Assign {
        name: name.clone(),
        value: Expression::Variable(global.to_owned()),
    };
    if hoist {
        prefix.insert(0, assign);
    } else {
        prefix.push(assign);
    }
    output.extend(prefix);
    let value = Expression::Variable(name);
    *condition = replace(condition, global, &value);
    value
}

// An embedded update must keep its source target and cannot become a read of
// the capture. Unknown expression forms therefore leave this rewrite untouched.
fn capture_expression(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(_) | Expression::IntegerLiteral(_) => true,
        Expression::Cast { operand, .. } | Expression::Unary { operand, .. } => {
            capture_expression(operand)
        }
        Expression::Binary { left, right, .. } => {
            capture_expression(left) && capture_expression(right)
        }
        Expression::Call { arguments, .. } => arguments.iter().all(capture_expression),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn updates_cannot_be_rewritten_as_guard_value_reads() {
        let expression = Expression::PostStep {
            target: Box::new(Expression::Variable("global".into())),
            operator: BinaryOperator::Add,
            pointer_link: None,
        };
        assert!(!capture_expression(&expression));
        assert!(!capture_expression(&Expression::Call {
            name: "consume".into(),
            arguments: vec![expression]
        }));
    }
}
