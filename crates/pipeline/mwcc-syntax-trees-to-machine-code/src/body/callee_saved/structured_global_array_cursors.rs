//! Carry leading global-array addresses through a counted loop.
//!
//! The original pointer locals become induction values. They must be private,
//! unmodified by the body, and dead after the loop: the final increment changes
//! their exit value from the last element to one past it. Address construction
//! moves into the for initializer; normal pointer arithmetic owns the stride.

use super::structured_expression_visit::{visit_expression, visit_statement};
use super::*;

/// Source identities retained when leading address definitions become loop
/// induction values. Allocation consumes these roles without reverse-matching
/// the rewritten comma expressions.
pub(super) struct CursorGroup {
    pub(super) index: String,
    pub(super) cursors: Vec<String>,
    pub(super) arrays: Vec<String>,
}

pub(super) struct Reduction {
    pub(super) function: Function,
    pub(super) groups: Vec<CursorGroup>,
}

pub(super) fn reduce(
    function: &Function,
    arrays: &std::collections::HashSet<String>,
    globals: &std::collections::HashMap<String, Type>,
) -> Option<Reduction> {
    let mut result = function.clone();
    let mut groups = Vec::new();
    for (position, statement) in function.statements.iter().enumerate() {
        let Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(initializer),
            condition: Some(condition),
            step: Some(step),
            body,
        } = statement
        else {
            continue;
        };
        let Expression::Assign { target, value } = initializer else {
            continue;
        };
        let Expression::Variable(index) = target.as_ref() else {
            continue;
        };
        if constant_value(value) != Some(0)
            || !private_local(function, index)
            || !function.locals.iter().any(|local| {
                local.name == *index && matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            })
        {
            continue;
        }
        if !matches!(condition, Expression::Binary { operator: BinaryOperator::Less, left, right }
            if matches!(left.as_ref(), Expression::Variable(name) if name == index)
                && constant_value(right).is_some_and(|n| n > 0 && n <= i64::from(i16::MAX)))
        {
            continue;
        }
        if !matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if name == index)
                && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                    if matches!(left.as_ref(), Expression::Variable(name) if name == index) && constant_value(right) == Some(1)))
        {
            continue;
        }
        // No alternate entries, opaque code, or nested control flow that could
        // observe a cursor between its tail update and the next leading bind.
        if !function.inline_asm_blocks.is_empty()
            || function.statements.iter().any(has_unstructured_control)
            || body.iter().any(|s| !simple_body(s) || rebinds(s, index))
        {
            continue;
        }
        let mut cursors = Vec::new();
        let mut cursor_arrays = Vec::new();
        for statement in body {
            let Statement::Assign {
                name,
                value: Expression::AddressOf { operand },
            } = statement
            else {
                break;
            };
            let Expression::Index { base, index: used } = operand.as_ref() else {
                break;
            };
            let Expression::Variable(global) = base.as_ref() else {
                break;
            };
            if !arrays.contains(global)
                || function.locals.iter().any(|local| local.name == *global)
                || function
                    .parameters
                    .iter()
                    .any(|parameter| parameter.name == *global)
                || !matches!(used.as_ref(), Expression::Variable(name) if name == index)
                || !private_local(function, name)
                || !function.locals.iter().any(|local| {
                    let stride = match local.declared_type {
                        Type::Pointer(element) => u32::from(element.size()),
                        Type::StructPointer { element_size } => element_size,
                        _ => return false,
                    };
                    let global_stride = globals.get(global).map(|ty| match ty {
                        Type::Struct { size, .. } => *size as u32,
                        other => u32::from(other.width()) / 8,
                    });
                    local.name == *name && stride > 0 && global_stride == Some(stride)
                })
                || cursors.iter().any(|(prior, _)| prior == name)
            {
                break;
            }
            cursors.push((
                name.clone(),
                Expression::AddressOf {
                    operand: Box::new(Expression::Index {
                        base: base.clone(),
                        index: Box::new(Expression::IntegerLiteral(0)),
                    }),
                },
            ));
            cursor_arrays.push(global.clone());
        }
        if cursors.is_empty() {
            continue;
        }
        let tail = &body[cursors.len()..];
        if cursors.iter().any(|(name, _)| {
            tail.iter().any(|s| rebinds(s, name))
                || function.statements[position + 1..]
                    .iter()
                    .any(|s| reads_name(s, name))
                || function
                    .return_expression
                    .as_ref()
                    .is_some_and(|e| expression_reads(e, name))
        }) {
            continue;
        }
        let mut init = initializer.clone();
        let mut next = step.clone();
        for (name, base) in &cursors {
            init = comma(init, assign(name, base.clone()));
            next = comma(
                next,
                assign(
                    name,
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(Expression::Variable(name.clone())),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    },
                ),
            );
        }
        result.statements[position] = Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(init),
            condition: Some(condition.clone()),
            step: Some(next),
            body: tail.to_vec(),
        };
        groups.push(CursorGroup {
            index: index.clone(),
            cursors: cursors.into_iter().map(|(name, _)| name).collect(),
            arrays: cursor_arrays,
        });
    }
    (!groups.is_empty()).then_some(Reduction { function: result, groups })
}

fn private_local(function: &Function, name: &str) -> bool {
    if !function.locals.iter().any(|local| {
        local.name == name && !local.is_volatile && !local.is_static && local.array_length.is_none()
    }) {
        return false;
    }
    let mut escaped = false;
    let mut inspect = |expression: &Expression| {
        if matches!(expression, Expression::AddressOf { operand } if matches!(operand.as_ref(), Expression::Variable(n) if n == name))
        {
            escaped = true;
        }
    };
    for statement in &function.statements {
        visit_statement(statement, &mut inspect);
    }
    for local in &function.locals {
        if let Some(value) = &local.initializer {
            visit_expression(value, &mut inspect);
        }
    }
    if let Some(value) = &function.return_expression {
        visit_expression(value, &mut inspect);
    }
    for guard in &function.guards {
        visit_expression(&guard.condition, &mut inspect);
        visit_expression(&guard.value, &mut inspect);
    }
    !escaped
}
fn expression_reads(expression: &Expression, name: &str) -> bool {
    let mut found = false;
    visit_expression(expression, &mut |e| {
        found |= matches!(e, Expression::Variable(n) if n == name);
    });
    found
}
fn reads_name(statement: &Statement, name: &str) -> bool {
    let mut found = false;
    visit_statement(statement, &mut |e| {
        found |= matches!(e, Expression::Variable(n) if n == name);
    });
    found
}
fn has_unstructured_control(statement: &Statement) -> bool {
    match statement {
        Statement::Goto(_) | Statement::Label(_) | Statement::InlineAsm(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body)
            .any(has_unstructured_control),
        Statement::Loop { body, .. } => body.iter().any(has_unstructured_control),
        Statement::Switch { .. } => true,
        _ => false,
    }
}
fn simple_body(statement: &Statement) -> bool {
    match statement {
        Statement::Assign { .. } | Statement::Store { .. } | Statement::Expression(_) => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body.iter().chain(else_body).all(simple_body),
        _ => false,
    }
}
fn assign(name: &str, value: Expression) -> Expression {
    Expression::Assign {
        target: Box::new(Expression::Variable(name.to_owned())),
        value: Box::new(value),
    }
}
fn comma(left: Expression, right: Expression) -> Expression {
    Expression::Comma {
        left: Box::new(left),
        right: Box::new(right),
    }
}

// Memory stores through a pointer do not replace the pointer itself. The
// broader structured write analysis intentionally treats these as dependent
// writes; induction needs the narrower local-binding fact.
fn rebinds(statement: &Statement, name: &str) -> bool {
    if matches!(statement, Statement::Assign { name: target, .. } if target == name) {
        return true;
    }
    if let Statement::If {
        then_body,
        else_body,
        ..
    } = statement
    {
        if then_body.iter().chain(else_body).any(|s| rebinds(s, name)) {
            return true;
        }
    }
    let mut assigned = false;
    visit_statement(statement, &mut |e| {
        if let Expression::Assign { target, .. } | Expression::PostStep { target, .. } = e {
            assigned |= matches!(target.as_ref(), Expression::Variable(n) if n == name);
        }
    });
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;
    fn variable(name: &str) -> Expression {
        Expression::Variable(name.into())
    }
    fn source() -> Function {
        let local = |name: &str, declared_type| LocalDeclaration {
            name: name.into(),
            declared_type,
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: vec![],
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        };
        Function {
            name: "walk".into(),
            return_type: Type::Void,
            is_static: false,
            is_weak: false,
            parameters: vec![],
            locals: vec![
                local("i", Type::UnsignedInt),
                local("p", Type::Pointer(Pointee::UnsignedInt)),
            ],
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: Some(assign("i", Expression::IntegerLiteral(0))),
                condition: Some(Expression::Binary {
                    operator: BinaryOperator::Less,
                    left: Box::new(variable("i")),
                    right: Box::new(Expression::IntegerLiteral(64)),
                }),
                step: Some(assign(
                    "i",
                    Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(variable("i")),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    },
                )),
                body: vec![
                    Statement::Assign {
                        name: "p".into(),
                        value: Expression::AddressOf {
                            operand: Box::new(Expression::Index {
                                base: Box::new(variable("words")),
                                index: Box::new(variable("i")),
                            }),
                        },
                    },
                    Statement::Expression(Expression::Assign {
                        target: Box::new(Expression::Dereference {
                            pointer: Box::new(variable("p")),
                        }),
                        value: Box::new(variable("i")),
                    }),
                ],
            }],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }
    fn reduced(function: &Function) -> Option<Function> {
        reduce(
            function,
            &std::collections::HashSet::from(["words".into()]),
            &std::collections::HashMap::from([("words".into(), Type::UnsignedInt)]),
        ).map(|reduction| reduction.function)
    }
    fn body(function: &mut Function) -> &mut Vec<Statement> {
        let Statement::Loop { body, .. } = &mut function.statements[0] else {
            panic!()
        };
        body
    }
    #[test]
    fn carries_the_original_pointer_and_keeps_indirect_assignments() {
        let original = source();
        let rewritten = reduced(&original).expect("affine pointer");
        assert_eq!(rewritten.locals.len(), original.locals.len());
        let Statement::Loop {
            initializer: Some(Expression::Comma { .. }),
            step: Some(Expression::Comma { .. }),
            body,
            ..
        } = &rewritten.statements[0]
        else {
            panic!("cursor init and step")
        };
        assert_eq!(body.len(), 1);
        assert!(!rebinds(&body[0], "p"));
    }
    #[test]
    fn rejects_observed_mutated_and_escaped_bindings() {
        let mut returned = source();
        returned.return_expression = Some(variable("p"));
        assert!(reduced(&returned).is_none());
        let mut observed = source();
        observed
            .statements
            .push(Statement::Expression(variable("p")));
        assert!(reduced(&observed).is_none());
        for name in ["i", "p"] {
            let mut changed = source();
            body(&mut changed).push(Statement::Expression(assign(
                name,
                Expression::IntegerLiteral(7),
            )));
            assert!(reduced(&changed).is_none());
            let mut escaped = source();
            body(&mut escaped).push(Statement::Expression(Expression::AddressOf {
                operand: Box::new(variable(name)),
            }));
            assert!(reduced(&escaped).is_none());
            let mut volatile = source();
            volatile
                .locals
                .iter_mut()
                .find(|l| l.name == name)
                .unwrap()
                .is_volatile = true;
            assert!(reduced(&volatile).is_none());
        }
    }
    #[test]
    fn rejects_stride_mismatches_shadowing_and_alternate_control() {
        let mut mismatch = source();
        mismatch.locals[1].declared_type = Type::Pointer(Pointee::UnsignedChar);
        assert!(reduced(&mismatch).is_none());
        let mut shadow = source();
        shadow.parameters.push(mwcc_syntax_trees::Parameter {
            name: "words".into(),
            parameter_type: Type::Pointer(Pointee::UnsignedInt),
        });
        assert!(reduced(&shadow).is_none());
        for statement in [
            Statement::Continue,
            Statement::Break,
            Statement::Return(None),
            Statement::Label("again".into()),
        ] {
            let mut alternate = source();
            body(&mut alternate).push(statement);
            assert!(reduced(&alternate).is_none());
        }
        let mut zero = source();
        if let Statement::Loop {
            condition: Some(Expression::Binary { right, .. }),
            ..
        } = &mut zero.statements[0]
        {
            *right = Box::new(Expression::IntegerLiteral(0));
        }
        assert!(reduced(&zero).is_none());
    }
}
