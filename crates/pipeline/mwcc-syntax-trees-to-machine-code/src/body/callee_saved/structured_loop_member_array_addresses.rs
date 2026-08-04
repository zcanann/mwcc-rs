//! Loop-scoped addresses for repeated embedded member-array elements.
//!
//! A member-array element used both before and after a loop-local call has an
//! lvalue lifetime distinct from the owner and from the loaded element value.
//! Optimized MWCC materializes that address once at the narrowest dominating
//! statement list. Exposing the address as a generated local lets ordinary
//! liveness allocate the saved home and lets each later load observe mutations
//! made through a call.

use super::*;

const ADDRESS_PREFIX: &str = "__mwcc_loop_member_address_";

#[derive(Clone)]
struct Candidate {
    element: Expression,
    owner: String,
    declared_type: Type,
    loaded_through_address: bool,
}

pub(super) fn materialize_loop_member_array_addresses(function: &Function) -> Option<Function> {
    let mut used: std::collections::HashSet<String> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(function.locals.iter().map(|local| local.name.clone()))
        .collect();
    let mut next_name = 0usize;
    let mut declarations = Vec::new();
    let mut changed = false;
    let statements = function
        .statements
        .iter()
        .map(|statement| {
            reduce_statement(
                statement,
                &mut used,
                &mut next_name,
                &mut declarations,
                &mut changed,
            )
        })
        .collect();

    changed.then(|| {
        let mut rewritten = function.clone();
        rewritten.locals.extend(declarations);
        rewritten.statements = statements;
        rewritten
    })
}

fn reduce_statement(
    statement: &Statement,
    used: &mut std::collections::HashSet<String>,
    next_name: &mut usize,
    declarations: &mut Vec<LocalDeclaration>,
    changed: &mut bool,
) -> Statement {
    if let Some(mut candidates) = recognize_loop(statement) {
        let Statement::Loop {
            kind,
            initializer,
            condition,
            step,
            body,
        } = statement
        else {
            unreachable!("member-array addresses are recognized only from loops")
        };
        let mut body = body.clone();
        for candidate in candidates.drain(..) {
            let name = fresh_name(used, next_name);
            let Some(rewritten) = materialize_in_list(&body, &candidate, &name) else {
                continue;
            };
            declarations.push(LocalDeclaration {
                declared_type: candidate.declared_type,
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
            });
            body = rewritten;
            *changed = true;
        }
        return Statement::Loop {
            kind: *kind,
            initializer: initializer.clone(),
            condition: condition.clone(),
            step: step.clone(),
            body,
        };
    }
    match statement {
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: condition.clone(),
            then_body: then_body
                .iter()
                .map(|statement| {
                    reduce_statement(statement, used, next_name, declarations, changed)
                })
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| {
                    reduce_statement(statement, used, next_name, declarations, changed)
                })
                .collect(),
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
            body: body
                .iter()
                .map(|statement| {
                    reduce_statement(statement, used, next_name, declarations, changed)
                })
                .collect(),
        },
        _ => statement.clone(),
    }
}

fn recognize_loop(statement: &Statement) -> Option<Vec<Candidate>> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    let index = zero_initializer(initializer)?;
    if counted_step(step)? != index || !crate::analysis::expression_reads_name(condition, index) {
        return None;
    }

    let mut candidates = Vec::<Candidate>::new();
    for statement in body {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            let Some(candidate) = candidate(expression, index) else {
                return;
            };
            if !candidates.iter().any(|existing| {
                crate::analysis::structurally_equal(&existing.element, &candidate.element)
            }) {
                candidates.push(candidate);
            }
        });
    }
    candidates.retain(|candidate| {
        let uses = body
            .iter()
            .map(|statement| statement_occurrences(statement, candidate))
            .sum::<usize>();
        uses >= 2
            && !super::structured_expression_visit::statements_assign_name(body, &candidate.owner)
            && body
                .iter()
                .any(|statement| candidate_is_used_by_call(statement, candidate))
    });
    (!candidates.is_empty()).then_some(candidates)
}

fn candidate(expression: &Expression, index: &str) -> Option<Candidate> {
    let Expression::Index { base, index: used } = expression else {
        return None;
    };
    if !matches!(used.as_ref(), Expression::Variable(name) if name == index) {
        return None;
    }
    match base.as_ref() {
        Expression::MemberAddress {
            base: owner,
            element,
            index_stride: None,
            ..
        } if matches!(owner.as_ref(), Expression::Variable(_))
            && matches!(element, Pointee::Pointer | Pointee::WordPointer) =>
        {
            let Expression::Variable(owner) = owner.as_ref() else {
                unreachable!("member-array owner was gated as a variable")
            };
            Some(Candidate {
                element: expression.clone(),
                owner: owner.clone(),
                declared_type: Type::Pointer(*element),
                loaded_through_address: true,
            })
        }
        Expression::Member {
            base: owner,
            member_type: Type::Struct { size, .. },
            index_stride: None,
            ..
        } if *size != 0 && matches!(owner.as_ref(), Expression::Variable(_)) => {
            let Expression::Variable(owner) = owner.as_ref() else {
                unreachable!("member-array owner was gated as a variable")
            };
            Some(Candidate {
                element: expression.clone(),
                owner: owner.clone(),
                declared_type: Type::StructPointer {
                    element_size: *size,
                },
                loaded_through_address: false,
            })
        }
        _ => None,
    }
}

fn materialize_in_list(
    statements: &[Statement],
    candidate: &Candidate,
    name: &str,
) -> Option<Vec<Statement>> {
    let containing: Vec<_> = statements
        .iter()
        .enumerate()
        .filter_map(|(index, statement)| {
            (statement_occurrences(statement, candidate) != 0).then_some(index)
        })
        .collect();
    let first = *containing.first()?;
    if containing.len() == 1 {
        if let Some(delegated) = materialize_inside_statement(&statements[first], candidate, name) {
            let mut rewritten = statements.to_vec();
            rewritten[first] = delegated;
            return Some(rewritten);
        }
    }

    let mut rewritten = Vec::with_capacity(statements.len() + 1);
    for (index, statement) in statements.iter().enumerate() {
        if index == first {
            rewritten.push(initializer(candidate, name));
        }
        rewritten.push(rewrite_candidate(statement, candidate, name));
    }
    Some(rewritten)
}

fn materialize_inside_statement(
    statement: &Statement,
    candidate: &Candidate,
    name: &str,
) -> Option<Statement> {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return None;
    };
    if expression_occurrences(condition, candidate) != 0 {
        return None;
    }
    let then_uses = then_body
        .iter()
        .map(|statement| statement_occurrences(statement, candidate))
        .sum::<usize>();
    let else_uses = else_body
        .iter()
        .map(|statement| statement_occurrences(statement, candidate))
        .sum::<usize>();
    if then_uses != 0 && else_uses == 0 {
        return Some(Statement::If {
            condition: condition.clone(),
            then_body: materialize_in_list(then_body, candidate, name)?,
            else_body: else_body.clone(),
        });
    }
    if else_uses != 0 && then_uses == 0 {
        return Some(Statement::If {
            condition: condition.clone(),
            then_body: then_body.clone(),
            else_body: materialize_in_list(else_body, candidate, name)?,
        });
    }
    None
}

fn initializer(candidate: &Candidate, name: &str) -> Statement {
    Statement::Assign {
        name: name.into(),
        value: Expression::AddressOf {
            operand: Box::new(candidate.element.clone()),
        },
    }
}

fn rewrite_candidate(statement: &Statement, candidate: &Candidate, name: &str) -> Statement {
    super::structured_expression_visit::rewrite_statement(statement, &mut |expression| {
        if matches!(expression, Expression::AddressOf { operand }
            if crate::analysis::structurally_equal(operand, &candidate.element))
        {
            return Some(Expression::Variable(name.into()));
        }
        if !crate::analysis::structurally_equal(expression, &candidate.element) {
            return None;
        }
        Some(if candidate.loaded_through_address {
            Expression::Dereference {
                pointer: Box::new(Expression::Variable(name.into())),
            }
        } else {
            Expression::Variable(name.into())
        })
    })
}

fn statement_occurrences(statement: &Statement, candidate: &Candidate) -> usize {
    let mut count = 0usize;
    super::structured_expression_visit::visit_statement(statement, &mut |expression| {
        count += usize::from(crate::analysis::structurally_equal(
            expression,
            &candidate.element,
        ));
    });
    count
}

fn expression_occurrences(expression: &Expression, candidate: &Candidate) -> usize {
    let mut count = 0usize;
    super::structured_expression_visit::visit_expression(expression, &mut |expression| {
        count += usize::from(crate::analysis::structurally_equal(
            expression,
            &candidate.element,
        ));
    });
    count
}

fn candidate_is_used_by_call(statement: &Statement, candidate: &Candidate) -> bool {
    let mut used = false;
    super::structured_expression_visit::visit_statement(statement, &mut |expression| {
        let (Expression::Call { arguments, .. }
        | Expression::CallThrough { arguments, .. }
        | Expression::VirtualCall { arguments, .. }) = expression
        else {
            return;
        };
        used |= arguments
            .iter()
            .any(|argument| expression_occurrences(argument, candidate) != 0);
    });
    used
}

fn zero_initializer(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    (crate::analysis::constant_value(value) == Some(0)).then_some(index)
}

fn counted_step(expression: &Expression) -> Option<&str> {
    let Expression::Assign { target, value } = expression else {
        return None;
    };
    let Expression::Variable(index) = target.as_ref() else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = value.as_ref()
    else {
        return None;
    };
    (matches!(left.as_ref(), Expression::Variable(name) if name == index)
        && crate::analysis::constant_value(right).is_some_and(|step| step > 0))
    .then_some(index)
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next: &mut usize) -> String {
    loop {
        let name = format!("{ADDRESS_PREFIX}{}", *next);
        *next += 1;
        if used.insert(name.clone()) {
            return name;
        }
    }
}

pub(super) struct HomeLayout {
    preferences: std::collections::HashMap<usize, u8>,
}

impl HomeLayout {
    pub(super) fn plan(
        eager_local_count: usize,
        saved_parameter_count: usize,
        deferred_locals: &[&LocalDeclaration],
        deferred_homes: &super::structured_locals::DeferredSavedHomePlan,
        parameter_reuse: &super::structured_parameter_home_reuse::StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        if eager_local_count != 2
            || saved_parameter_count != 1
            || deferred_locals.len() != 2
            || deferred_homes.group_count != 2
            || home_count != 5
            || !deferred_locals
                .iter()
                .all(|local| local.name.starts_with(ADDRESS_PREFIX))
        {
            return None;
        }
        let first = parameter_reuse.home_index(deferred_homes.group(&deferred_locals[0].name));
        let second = parameter_reuse.home_index(deferred_homes.group(&deferred_locals[1].name));
        Some(Self {
            preferences: std::collections::HashMap::from([
                (0, 29),
                (1, 30),
                (2, 31),
                (first, 28),
                (second, 27),
            ]),
        })
    }

    pub(super) fn preference(&self, home: usize) -> Option<u8> {
        self.preferences.get(&home).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(array_offset: u32, element: Pointee) -> Expression {
        Expression::Index {
            base: Box::new(Expression::MemberAddress {
                base: Box::new(Expression::Variable("object".into())),
                offset: array_offset,
                element,
                index_stride: None,
            }),
            index: Box::new(Expression::Variable("i".into())),
        }
    }

    fn counted_loop(body: Vec<Statement>) -> Statement {
        Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            condition: Some(Expression::Binary {
                operator: BinaryOperator::Less,
                left: Box::new(Expression::Variable("i".into())),
                right: Box::new(Expression::IntegerLiteral(4)),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable("i".into())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable("i".into())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body,
        }
    }

    #[test]
    fn places_a_repeated_call_element_address_before_its_guard() {
        let element = index(56, Pointee::Pointer);
        let loop_statement = counted_loop(vec![Statement::If {
            condition: element.clone(),
            then_body: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![element],
            })],
            else_body: Vec::new(),
        }]);
        let candidates = recognize_loop(&loop_statement).expect("address should be retained");
        let Statement::Loop { body, .. } = &loop_statement else {
            unreachable!()
        };
        let rewritten = materialize_in_list(body, &candidates[0], "@address")
            .expect("address should be materialized");

        assert!(matches!(
            rewritten.as_slice(),
            [Statement::Assign { name, .. }, Statement::If { .. }] if name == "@address"
        ));
    }

    #[test]
    fn places_a_struct_element_address_inside_its_taken_arm() {
        let element = Expression::Index {
            base: Box::new(Expression::Member {
                base: Box::new(Expression::Variable("object".into())),
                offset: 72,
                member_type: Type::Struct {
                    size: 24,
                    align: 4,
                },
                index_stride: None,
            }),
            index: Box::new(Expression::Variable("i".into())),
        };
        let address = || Expression::AddressOf {
            operand: Box::new(element.clone()),
        };
        let loop_statement = counted_loop(vec![Statement::If {
            condition: Expression::IntegerLiteral(1),
            then_body: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![address(), address()],
            })],
            else_body: Vec::new(),
        }]);

        let candidates = recognize_loop(&loop_statement).expect("address should be retained");
        let Statement::Loop { body, .. } = &loop_statement else {
            unreachable!()
        };
        let rewritten = materialize_in_list(body, &candidates[0], "@address")
            .expect("address should be materialized");

        assert!(matches!(
            rewritten.as_slice(),
            [Statement::If { then_body, .. }]
                if matches!(then_body.as_slice(),
                    [Statement::Assign { name, .. }, Statement::Expression(Expression::Call { arguments, .. })]
                        if name == "@address"
                            && arguments.iter().all(|argument|
                                matches!(argument, Expression::Variable(name) if name == "@address")))
        ));
    }

    #[test]
    fn rejects_an_address_whose_owner_is_reassigned_in_the_loop() {
        let element = index(56, Pointee::Pointer);
        let loop_statement = counted_loop(vec![
            Statement::Assign {
                name: "object".into(),
                value: Expression::Variable("next".into()),
            },
            Statement::If {
                condition: element.clone(),
                then_body: vec![Statement::Expression(Expression::Call {
                    name: "consume".into(),
                    arguments: vec![element],
                })],
                else_body: Vec::new(),
            },
        ]);

        assert!(recognize_loop(&loop_statement).is_none());
    }
}
