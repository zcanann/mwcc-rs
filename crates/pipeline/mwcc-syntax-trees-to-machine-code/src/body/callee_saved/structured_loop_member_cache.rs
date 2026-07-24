//! Reuse of stable member loads across a loop-local clamp branch.
//!
//! MWCC retains a member loaded by a comparison when the taken arm only assigns
//! locals and reads that same member again. The proof here is deliberately
//! narrow: an empty else arm, no calls or stores, and no reassignment of the
//! member base.

#[allow(unused_imports)]
use super::*;

pub(super) fn cache_repeated_loop_members(function: &Function) -> Option<Function> {
    let mut used_names: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let (statements, changed) = rewrite_sequence(
        &function.statements,
        false,
        &mut used_names,
        &mut declarations,
        &mut next_name,
    );
    changed.then(|| {
        let mut cached = function.clone();
        cached.locals.extend(declarations);
        cached.statements = statements;
        cached
    })
}

fn rewrite_sequence(
    statements: &[Statement],
    in_loop: bool,
    used_names: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, bool) {
    let mut output = Vec::with_capacity(statements.len());
    let mut changed = false;
    for statement in statements {
        match statement {
            Statement::Loop {
                kind,
                initializer,
                condition,
                step,
                body,
            } => {
                let (body, body_changed) =
                    rewrite_sequence(body, true, used_names, declarations, next_name);
                output.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body,
                });
                changed |= body_changed;
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } if in_loop => {
                if let Some((member, member_type)) =
                    cacheable_member(condition, then_body, else_body)
                {
                    let name = fresh_name(used_names, next_name);
                    declarations.push(local(&name, member_type));
                    output.push(Statement::Assign {
                        name: name.clone(),
                        value: member.clone(),
                    });
                    output.push(Statement::If {
                        condition: super::structured_loop_packet_invariant_rewrite::replace(
                            condition,
                            &[(member, name.clone())],
                        ),
                        then_body: then_body
                            .iter()
                            .map(|statement| replace_assignment(statement, member, &name))
                            .collect(),
                        else_body: Vec::new(),
                    });
                    changed = true;
                } else {
                    let (then_body, then_changed) =
                        rewrite_sequence(then_body, true, used_names, declarations, next_name);
                    let (else_body, else_changed) =
                        rewrite_sequence(else_body, true, used_names, declarations, next_name);
                    output.push(Statement::If {
                        condition: condition.clone(),
                        then_body,
                        else_body,
                    });
                    changed |= then_changed || else_changed;
                }
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let (then_body, then_changed) =
                    rewrite_sequence(then_body, false, used_names, declarations, next_name);
                let (else_body, else_changed) =
                    rewrite_sequence(else_body, false, used_names, declarations, next_name);
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

fn cacheable_member<'a>(
    condition: &'a Expression,
    then_body: &[Statement],
    else_body: &[Statement],
) -> Option<(&'a Expression, Type)> {
    if !else_body.is_empty() || then_body.is_empty() {
        return None;
    }
    let member = direct_member(condition)?;
    let Expression::Member {
        base,
        member_type,
        index_stride: None,
        ..
    } = member
    else {
        return None;
    };
    let Expression::Variable(base_name) = base.as_ref() else {
        return None;
    };
    if !matches!(
        member_type,
        Type::UnsignedChar | Type::UnsignedShort | Type::UnsignedInt
    ) {
        return None;
    }
    if then_body.iter().any(|statement| {
        !matches!(statement, Statement::Assign { name, value }
            if name != base_name && safe_value(value))
    }) {
        return None;
    }
    let uses = count_expression(condition, member)
        + then_body
            .iter()
            .map(|statement| match statement {
                Statement::Assign { value, .. } => count_expression(value, member),
                _ => 0,
            })
            .sum::<usize>();
    (uses >= 3).then_some((member, Type::UnsignedInt))
}

fn direct_member(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Member { .. } => Some(expression),
        Expression::Binary { left, right, .. } => {
            direct_member(left).or_else(|| direct_member(right))
        }
        Expression::Cast { operand, .. } | Expression::Unary { operand, .. } => {
            direct_member(operand)
        }
        _ => None,
    }
}

fn safe_value(expression: &Expression) -> bool {
    match expression {
        Expression::IntegerLiteral(_) | Expression::Variable(_) => true,
        Expression::Binary { left, right, .. } => safe_value(left) && safe_value(right),
        Expression::Unary { operand, .. } | Expression::Cast { operand, .. } => safe_value(operand),
        Expression::Member { base, .. } => safe_value(base),
        _ => false,
    }
}

fn count_expression(expression: &Expression, candidate: &Expression) -> usize {
    if crate::analysis::structurally_equal(expression, candidate) {
        return 1;
    }
    match expression {
        Expression::Binary { left, right, .. } | Expression::Comma { left, right } => {
            count_expression(left, candidate) + count_expression(right, candidate)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::IndexedUpdateValue { value: operand } => count_expression(operand, candidate),
        Expression::Member { base, .. } => count_expression(base, candidate),
        _ => 0,
    }
}

fn replace_assignment(statement: &Statement, member: &Expression, name: &str) -> Statement {
    match statement {
        Statement::Assign {
            name: target,
            value,
        } => Statement::Assign {
            name: target.clone(),
            value: super::structured_loop_packet_invariant_rewrite::replace(
                value,
                &[(member, name.to_owned())],
            ),
        },
        _ => statement.clone(),
    }
}

fn fresh_name(used_names: &mut std::collections::HashSet<String>, next_name: &mut usize) -> String {
    loop {
        let name = format!("__mwcc_loop_member_{}", *next_name);
        *next_name += 1;
        if used_names.insert(name.clone()) {
            return name;
        }
    }
}

fn local(name: &str, declared_type: Type) -> LocalDeclaration {
    LocalDeclaration {
        declared_type,
        name: name.to_owned(),
        initializer: None,
        is_volatile: false,
        array_length: None,
        is_static: false,
        data_bytes: None,
        data_relocations: Vec::new(),
        is_const: false,
        row_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset: 4,
            member_type: Type::UnsignedShort,
            index_stride: None,
        }
    }

    fn function(then_body: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "clamp".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![mwcc_syntax_trees::Parameter {
                parameter_type: Type::StructPointer { element_size: 8 },
                name: "state".into(),
            }],
            locals: vec![
                local("end", Type::UnsignedInt),
                local("start", Type::UnsignedInt),
                local("count", Type::UnsignedInt),
            ],
            statements: vec![Statement::Loop {
                kind: LoopKind::While,
                initializer: None,
                condition: Some(Expression::IntegerLiteral(1)),
                step: None,
                body: vec![Statement::If {
                    condition: Expression::Binary {
                        operator: BinaryOperator::Greater,
                        left: Box::new(Expression::Variable("end".into())),
                        right: Box::new(member()),
                    },
                    then_body,
                    else_body: Vec::new(),
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
    fn caches_a_member_shared_by_a_clamp_condition_and_arm() {
        let function = function(vec![
            Statement::Assign {
                name: "end".into(),
                value: member(),
            },
            Statement::Assign {
                name: "count".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Subtract,
                    left: Box::new(member()),
                    right: Box::new(Expression::Variable("start".into())),
                },
            },
        ]);
        let cached = cache_repeated_loop_members(&function).expect("cached loop member");
        let Statement::Loop { body, .. } = &cached.statements[0] else {
            panic!("expected loop")
        };

        assert!(matches!(
            &body[..],
            [
                Statement::Assign { name, value },
                Statement::If {
                    condition: Expression::Binary { right, .. },
                    then_body,
                    ..
                },
            ] if name == "__mwcc_loop_member_0"
                && crate::analysis::structurally_equal(value, &member())
                && matches!(right.as_ref(), Expression::Variable(cached) if cached == name)
                && matches!(&then_body[0], Statement::Assign {
                    value: Expression::Variable(cached),
                    ..
                } if cached == name)
        ));
    }

    #[test]
    fn rejects_a_taken_arm_with_a_store() {
        let function = function(vec![Statement::Store {
            target: Expression::Variable("count".into()),
            value: member(),
        }]);
        assert!(cache_repeated_loop_members(&function).is_none());
    }
}
