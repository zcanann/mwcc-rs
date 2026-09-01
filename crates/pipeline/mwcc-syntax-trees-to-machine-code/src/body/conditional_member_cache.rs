//! Materialize repeated read-only member images inside one conditional tree.

use mwcc_syntax_trees::{ArmBody, Expression, Function, LocalDeclaration, Statement, Type};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct PointerMember {
    root: String,
    offset: u32,
    element_size: u32,
}

fn pointer_member(expression: &Expression, parameters: &HashSet<&str>) -> Option<PointerMember> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::StructPointer { element_size },
        index_stride: None,
    } = expression
    else {
        return None;
    };
    let Expression::Variable(root) = base.as_ref() else {
        return None;
    };
    parameters.contains(root.as_str()).then(|| PointerMember {
        root: root.clone(),
        offset: *offset,
        element_size: *element_size,
    })
}

fn matches_pointer_member(expression: &Expression, key: &PointerMember) -> bool {
    matches!(
        expression,
        Expression::Member {
            base,
            offset,
            member_type: Type::StructPointer { element_size },
            index_stride: None,
        } if *offset == key.offset
            && *element_size == key.element_size
            && matches!(base.as_ref(), Expression::Variable(root) if root == &key.root)
    )
}

fn count_pointer_members(
    statement: &Statement,
    parameters: &HashSet<&str>,
) -> HashMap<PointerMember, usize> {
    let mut counts = HashMap::new();
    super::callee_saved::visit_structured_statement(statement, &mut |expression| {
        if let Some(key) = pointer_member(expression, parameters) {
            *counts.entry(key).or_default() += 1;
        }
    });
    counts
}

fn expression_contains_pointer_member(expression: &Expression, key: &PointerMember) -> bool {
    let mut found = false;
    super::callee_saved::visit_structured_expression(expression, &mut |expression| {
        found |= matches_pointer_member(expression, key);
    });
    found
}

fn statement_has_call_or_store(statement: &Statement) -> bool {
    let mut call = false;
    super::callee_saved::visit_structured_statement(statement, &mut |expression| {
        call |= matches!(
            expression,
            Expression::Call { .. }
                | Expression::CallThrough { .. }
                | Expression::VirtualCall { .. }
                | Expression::ConstructedNew { .. }
        );
    });
    call || match statement {
        Statement::Store { .. } => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body)
            .any(statement_has_call_or_store),
        Statement::Switch { arms, default, .. } => arms
            .iter()
            .map(|arm| &arm.body)
            .chain(default)
            .any(|body| match body {
                ArmBody::Return(_) => false,
                ArmBody::Statements(statements) => {
                    statements.iter().any(statement_has_call_or_store)
                }
            }),
        Statement::Loop { body, .. } => body.iter().any(statement_has_call_or_store),
        _ => false,
    }
}

fn fresh_name(occupied: &mut HashSet<String>, next: &mut usize, suffix: &str) -> String {
    loop {
        let candidate = format!("__mwcc_condition_member_{}_{}", *next, suffix);
        *next += 1;
        if occupied.insert(candidate.clone()) {
            return candidate;
        }
    }
}

fn scalar_local(name: String, declared_type: Type) -> LocalDeclaration {
    LocalDeclaration {
        declared_type,
        name,
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

fn cache_statement(
    statement: &Statement,
    parameters: &HashSet<&str>,
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next: &mut usize,
) -> Option<Vec<Statement>> {
    let Statement::If { condition, .. } = statement else {
        return None;
    };
    if statement_has_call_or_store(statement) {
        return None;
    }
    let mut candidates = count_pointer_members(statement, parameters)
        .into_iter()
        .filter(|(key, count)| {
            *count >= 4
                && expression_contains_pointer_member(condition, key)
                && !super::callee_saved::structured_statements_assign_name(
                    std::slice::from_ref(statement),
                    &key.root,
                )
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.1.cmp(&left.1));
    let (key, _) = candidates.first()?;

    let base_name = fresh_name(occupied, next, "base");
    locals.push(scalar_local(
        base_name.clone(),
        Type::StructPointer {
            element_size: key.element_size,
        },
    ));
    let base_value = Expression::Member {
        base: Box::new(Expression::Variable(key.root.clone())),
        offset: key.offset,
        member_type: Type::StructPointer {
            element_size: key.element_size,
        },
        index_stride: None,
    };
    let mut rewritten =
        super::callee_saved::rewrite_structured_statement(statement, &mut |expression| {
            matches_pointer_member(expression, key).then(|| Expression::Variable(base_name.clone()))
        });
    let mut prefix = vec![Statement::Assign {
        name: base_name.clone(),
        value: base_value,
    }];

    let mut status_offsets = HashMap::<u32, usize>::new();
    super::callee_saved::visit_structured_statement(&rewritten, &mut |expression| {
        if let Expression::Member {
            base,
            offset,
            member_type: Type::UnsignedInt,
            index_stride: None,
        } = expression
        {
            if matches!(base.as_ref(), Expression::Variable(name) if name == &base_name) {
                *status_offsets.entry(*offset).or_default() += 1;
            }
        }
    });
    if let Some((status_offset, _)) = status_offsets
        .into_iter()
        .filter(|(_, count)| *count >= 3)
        .max_by_key(|(_, count)| *count)
    {
        let status_name = fresh_name(occupied, next, "value");
        locals.push(scalar_local(status_name.clone(), Type::UnsignedInt));
        prefix.push(Statement::Assign {
            name: status_name.clone(),
            value: Expression::Member {
                base: Box::new(Expression::Variable(base_name.clone())),
                offset: status_offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
        });
        rewritten =
            super::callee_saved::rewrite_structured_statement(&rewritten, &mut |expression| {
                matches!(
                    expression,
                    Expression::Member {
                        base,
                        offset,
                        member_type: Type::UnsignedInt,
                        index_stride: None,
                    } if *offset == status_offset
                        && matches!(base.as_ref(), Expression::Variable(name) if name == &base_name)
                )
                .then(|| Expression::Variable(status_name.clone()))
            });
    }
    prefix.push(rewritten);
    Some(prefix)
}

fn rewrite_blocks(
    statements: &[Statement],
    parameters: &HashSet<&str>,
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next: &mut usize,
) -> Vec<Statement> {
    let mut output = Vec::new();
    for statement in statements {
        if let Some(cached) = cache_statement(statement, parameters, locals, occupied, next) {
            output.extend(cached);
            continue;
        }
        let rewritten = match statement {
            Statement::If {
                condition,
                then_body,
                else_body,
            } => Statement::If {
                condition: condition.clone(),
                then_body: rewrite_blocks(then_body, parameters, locals, occupied, next),
                else_body: rewrite_blocks(else_body, parameters, locals, occupied, next),
            },
            other => other.clone(),
        };
        output.push(rewritten);
    }
    output
}

/// Cache a repeated pointer-valued member and its repeated scalar status image
/// at the first condition that already evaluates both. The statement must be
/// call- and store-free, so the cached values cannot be invalidated inside the
/// conditional tree.
pub(super) fn materialize(function: &Function) -> Option<Function> {
    let parameters = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<HashSet<_>>();
    let mut locals = function.locals.clone();
    let original_local_count = locals.len();
    let mut occupied = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(locals.iter().map(|local| local.name.clone()))
        .collect::<HashSet<_>>();
    let mut next = 0;
    let statements = rewrite_blocks(
        &function.statements,
        &parameters,
        &mut locals,
        &mut occupied,
        &mut next,
    );
    (locals.len() != original_local_count).then(|| Function {
        locals,
        statements,
        ..function.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, Parameter};

    fn status() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("block".into())),
                offset: 0,
                member_type: Type::StructPointer { element_size: 68 },
                index_stride: None,
            }),
            offset: 8,
            member_type: Type::UnsignedInt,
            index_stride: None,
        }
    }

    fn or(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalOr,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    #[test]
    fn caches_a_pointer_member_and_status_image_once_per_conditional_tree() {
        let function = Function {
            return_type: Type::Float,
            name: "fade".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::StructPointer { element_size: 44 },
                name: "block".into(),
            }],
            locals: vec![scalar_local("result".into(), Type::Float)],
            statements: vec![Statement::If {
                condition: or(or(status(), status()), status()),
                then_body: vec![Statement::Assign {
                    name: "result".into(),
                    value: Expression::Member {
                        base: Box::new(Expression::Member {
                            base: Box::new(Expression::Variable("block".into())),
                            offset: 0,
                            member_type: Type::StructPointer { element_size: 68 },
                            index_stride: None,
                        }),
                        offset: 56,
                        member_type: Type::Float,
                        index_stride: None,
                    },
                }],
                else_body: Vec::new(),
            }],
            guards: Vec::new(),
            return_expression: Some(Expression::Variable("result".into())),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let cached = materialize(&function).expect("the repeated conditional members should cache");
        assert_eq!(cached.locals.len(), 3);
        assert!(matches!(
            cached.statements.as_slice(),
            [
                Statement::Assign { .. },
                Statement::Assign { .. },
                Statement::If { .. }
            ]
        ));
        let mut nested_pointer_reads = 0;
        super::super::callee_saved::visit_structured_statement(
            &cached.statements[2],
            &mut |expression| {
                nested_pointer_reads +=
                    usize::from(pointer_member(expression, &HashSet::from(["block"])).is_some());
            },
        );
        assert_eq!(nested_pointer_reads, 0);
    }
}
