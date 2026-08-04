//! Short-range scalar common subexpressions inside structured loops.
//!
//! Optimized MWCC keeps cheap scaled values live when two later scalar
//! assignments consume the same computation without changing its source. The
//! semantic tree otherwise makes the ordinary expression emitter rebuild each
//! scale independently. Materializing a generated local exposes the shared
//! value to the existing liveness and register allocator without teaching
//! instruction selection about statement order.

#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::ArmBody;

const PREFIX: &str = "__mwcc_loop_scalar_cse_";
const REPLACEMENT_PROBE: &str = "__mwcc_loop_scalar_cse_replacement";

/// These generated values are renamed continuations of a source scalar, not
/// simultaneously live source-level roles. The planner proves that the source
/// has no observation between the generated definition and its final use, so
/// frame-pressure accounting must not count both names.
pub(super) fn is_materialized_scalar_continuation(name: &str) -> bool {
    name.starts_with(PREFIX)
}

pub(super) fn materialize_repeated_loop_scalars(function: &Function) -> Option<Function> {
    let address_taken = crate::frame::collect_address_taken(function);
    let volatile: std::collections::HashSet<&str> = function
        .locals
        .iter()
        .filter(|local| local.is_volatile)
        .map(|local| local.name.as_str())
        .collect();
    let types: std::collections::HashMap<&str, Type> = function
        .parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter.parameter_type))
        .chain(
            function
                .locals
                .iter()
                .map(|local| (local.name.as_str(), local.declared_type)),
        )
        .collect();
    let mut used: std::collections::HashSet<String> = types
        .keys()
        .map(|name| (*name).to_owned())
        .collect();
    let mut declarations = Vec::new();
    let mut next_name = 0usize;
    let (statements, changed) = rewrite_sequence(
        &function.statements,
        &types,
        &address_taken,
        &volatile,
        &mut used,
        &mut declarations,
        &mut next_name,
    );
    changed.then(|| {
        let mut rewritten = function.clone();
        rewritten.locals.extend(declarations);
        rewritten.statements = statements;
        rewritten
    })
}

fn rewrite_sequence(
    statements: &[Statement],
    types: &std::collections::HashMap<&str, Type>,
    address_taken: &std::collections::HashSet<String>,
    volatile: &std::collections::HashSet<&str>,
    used: &mut std::collections::HashSet<String>,
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
                let (body, nested_changed) = rewrite_sequence(
                    body,
                    types,
                    address_taken,
                    volatile,
                    used,
                    declarations,
                    next_name,
                );
                let (body, body_changed) = materialize_in_body(
                    &body,
                    types,
                    address_taken,
                    volatile,
                    used,
                    declarations,
                    next_name,
                );
                output.push(Statement::Loop {
                    kind: *kind,
                    initializer: initializer.clone(),
                    condition: condition.clone(),
                    step: step.clone(),
                    body,
                });
                changed |= nested_changed || body_changed;
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => {
                let (then_body, then_changed) = rewrite_sequence(
                    then_body,
                    types,
                    address_taken,
                    volatile,
                    used,
                    declarations,
                    next_name,
                );
                let (else_body, else_changed) = rewrite_sequence(
                    else_body,
                    types,
                    address_taken,
                    volatile,
                    used,
                    declarations,
                    next_name,
                );
                output.push(Statement::If {
                    condition: condition.clone(),
                    then_body,
                    else_body,
                });
                changed |= then_changed || else_changed;
            }
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => {
                let mut switch_changed = false;
                let arms = arms
                    .iter()
                    .map(|arm| mwcc_syntax_trees::SwitchArm {
                        value: arm.value,
                        body: rewrite_arm(
                            &arm.body,
                            types,
                            address_taken,
                            volatile,
                            used,
                            declarations,
                            next_name,
                            &mut switch_changed,
                        ),
                        falls_through: arm.falls_through,
                    })
                    .collect();
                let default = default.as_ref().map(|body| {
                    rewrite_arm(
                        body,
                        types,
                        address_taken,
                        volatile,
                        used,
                        declarations,
                        next_name,
                        &mut switch_changed,
                    )
                });
                output.push(Statement::Switch {
                    scrutinee: scrutinee.clone(),
                    arms,
                    default,
                });
                changed |= switch_changed;
            }
            _ => output.push(statement.clone()),
        }
    }
    (output, changed)
}

fn rewrite_arm(
    body: &ArmBody,
    types: &std::collections::HashMap<&str, Type>,
    address_taken: &std::collections::HashSet<String>,
    volatile: &std::collections::HashSet<&str>,
    used: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
    changed: &mut bool,
) -> ArmBody {
    match body {
        ArmBody::Return(value) => ArmBody::Return(value.clone()),
        ArmBody::Statements(statements) => {
            let (statements, arm_changed) = rewrite_sequence(
                statements,
                types,
                address_taken,
                volatile,
                used,
                declarations,
                next_name,
            );
            *changed |= arm_changed;
            ArmBody::Statements(statements)
        }
    }
}

struct Reuse {
    expression: Expression,
    source: String,
    value_type: Type,
    first: usize,
    second: usize,
    name: String,
}

fn materialize_in_body(
    body: &[Statement],
    types: &std::collections::HashMap<&str, Type>,
    address_taken: &std::collections::HashSet<String>,
    volatile: &std::collections::HashSet<&str>,
    used: &mut std::collections::HashSet<String>,
    declarations: &mut Vec<LocalDeclaration>,
    next_name: &mut usize,
) -> (Vec<Statement>, bool) {
    let mut reuses: Vec<Reuse> = Vec::new();
    for first in 0..body.len() {
        let Statement::Assign { value, .. } = &body[first] else {
            continue;
        };
        for candidate in crate::analysis::computed_subexpressions(value) {
            let Some((source, value_type)) = scaled_scalar(candidate, types) else {
                continue;
            };
            if volatile.contains(source)
                || address_taken.contains(source)
                || statement_assigns_name(&body[first], source)
                || reuses.iter().any(|reuse| {
                    reuse.source == source
                        || crate::analysis::structurally_equal(&reuse.expression, candidate)
                })
            {
                continue;
            }
            let second = (first + 1..body.len()).find(|second| {
                let Statement::Assign { value, .. } = &body[*second] else {
                    return false;
                };
                crate::analysis::computed_subexpressions(value)
                    .into_iter()
                    .any(|other| crate::analysis::structurally_equal(candidate, other))
            });
            let Some(second) = second else {
                continue;
            };
            let Statement::Assign {
                value: second_value,
                ..
            } = &body[second]
            else {
                continue;
            };
            let replacement = [(candidate, REPLACEMENT_PROBE.to_owned())];
            let first_without_reuse =
                super::structured_loop_packet_invariant_rewrite::replace(value, &replacement);
            let second_without_reuse = super::structured_loop_packet_invariant_rewrite::replace(
                second_value,
                &replacement,
            );
            if source == REPLACEMENT_PROBE
                || crate::analysis::expression_reads_name(&first_without_reuse, source)
                || crate::analysis::expression_reads_name(&second_without_reuse, source)
                || statement_assigns_name(&body[second], source)
                || body[first + 1..second].iter().any(|statement| {
                    super::structured_liveness::statement_reads_name(statement, source)
                        || statement_assigns_name(statement, source)
                })
            {
                continue;
            }
            reuses.push(Reuse {
                expression: candidate.clone(),
                source: source.to_owned(),
                value_type,
                first,
                second,
                name: fresh_name(used, next_name),
            });
        }
    }
    if reuses.is_empty() {
        return (body.to_vec(), false);
    }

    for reuse in &reuses {
        declarations.push(local(&reuse.name, reuse.value_type));
    }
    let mut output = Vec::with_capacity(body.len() + reuses.len());
    for (index, statement) in body.iter().enumerate() {
        for reuse in reuses.iter().filter(|reuse| reuse.first == index) {
            output.push(Statement::Assign {
                name: reuse.name.clone(),
                value: reuse.expression.clone(),
            });
        }
        let Statement::Assign { name, value } = statement else {
            output.push(statement.clone());
            continue;
        };
        let replacements: Vec<_> = reuses
            .iter()
            .filter(|reuse| reuse.first == index || reuse.second == index)
            .map(|reuse| (&reuse.expression, reuse.name.clone()))
            .collect();
        if replacements.is_empty() {
            output.push(statement.clone());
        } else {
            output.push(Statement::Assign {
                name: name.clone(),
                value: super::structured_loop_packet_invariant_rewrite::replace(
                    value,
                    &replacements,
                ),
            });
        }
    }
    (output, true)
}

fn scaled_scalar<'a>(
    expression: &'a Expression,
    types: &std::collections::HashMap<&str, Type>,
) -> Option<(&'a str, Type)> {
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = expression
    else {
        return None;
    };
    let (Expression::Variable(source), Expression::IntegerLiteral(scale)) =
        (left.as_ref(), right.as_ref())
    else {
        return None;
    };
    let scale = u32::try_from(*scale).ok()?;
    if scale < 2 || !scale.is_power_of_two() {
        return None;
    }
    let value_type = match types.get(source.as_str())? {
        Type::UnsignedInt => Type::UnsignedInt,
        Type::Char
        | Type::UnsignedChar
        | Type::Short
        | Type::UnsignedShort
        | Type::Int => Type::Int,
        _ => return None,
    };
    Some((source, value_type))
}

fn statement_assigns_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || crate::analysis::expression_assigns_name(value, name),
        Statement::Store { target, value } => {
            crate::analysis::expression_assigns_name(target, name)
                || crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Expression(value) | Statement::Return(Some(value)) => {
            crate::analysis::expression_assigns_name(value, name)
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            crate::analysis::expression_assigns_name(condition, name)
                || then_body
                    .iter()
                    .chain(else_body)
                    .any(|statement| statement_assigns_name(statement, name))
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            crate::analysis::expression_assigns_name(scrutinee, name)
                || arms.iter().any(|arm| arm_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_assigns_name(body, name))
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
                .any(|value| crate::analysis::expression_assigns_name(value, name))
                || body
                    .iter()
                    .any(|statement| statement_assigns_name(statement, name))
        }
        Statement::InlineAsm(_) => true,
        Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_)
        | Statement::Return(None) => false,
    }
}

fn arm_assigns_name(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(value) => crate::analysis::expression_assigns_name(value, name),
        ArmBody::Statements(statements) => statements
            .iter()
            .any(|statement| statement_assigns_name(statement, name)),
    }
}

fn fresh_name(used: &mut std::collections::HashSet<String>, next_name: &mut usize) -> String {
    loop {
        let name = format!("{PREFIX}{}", *next_name);
        *next_name += 1;
        if used.insert(name.clone()) {
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
        attribute_alignment: None,
        row_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scale(name: &str, amount: i64) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left: Box::new(Expression::Variable(name.into())),
            right: Box::new(Expression::IntegerLiteral(amount)),
        }
    }

    fn sum(left: Expression, right: Expression) -> Expression {
        Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(left),
            right: Box::new(right),
        }
    }

    fn assign(name: &str, value: Expression) -> Statement {
        Statement::Assign {
            name: name.into(),
            value,
        }
    }

    fn function(body: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "scales".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: ["a", "b", "first", "second"]
                .into_iter()
                .map(|name| local(name, Type::Int))
                .collect(),
            statements: vec![Statement::Loop {
                kind: LoopKind::For,
                initializer: None,
                condition: Some(Expression::Variable("a".into())),
                step: None,
                body,
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
    fn materializes_independent_scales_across_scalar_assignments() {
        let expression = sum(scale("a", 4), scale("b", 2));
        let source = function(vec![
            assign("first", expression.clone()),
            Statement::Expression(Expression::Variable("first".into())),
            assign("second", expression),
        ]);
        let rewritten = materialize_repeated_loop_scalars(&source)
            .expect("the repeated scales should be materialized");
        assert_eq!(rewritten.locals.len(), source.locals.len() + 2);
        assert_eq!(
            rewritten.locals[source.locals.len()..]
                .iter()
                .map(|local| local.name.as_str())
                .collect::<Vec<_>>(),
            vec!["__mwcc_loop_scalar_cse_0", "__mwcc_loop_scalar_cse_1"]
        );
        let Statement::Loop { body, .. } = &rewritten.statements[0] else {
            panic!("expected the source loop")
        };
        assert_eq!(body.len(), 5);
        assert!(matches!(&body[0], Statement::Assign { name, .. }
            if name == "__mwcc_loop_scalar_cse_0"));
        assert!(matches!(&body[1], Statement::Assign { name, .. }
            if name == "__mwcc_loop_scalar_cse_1"));
        for index in [2usize, 4] {
            let Statement::Assign { value, .. } = &body[index] else {
                panic!("expected a rewritten scalar assignment")
            };
            assert!(crate::analysis::expression_reads_name(
                value,
                "__mwcc_loop_scalar_cse_0"
            ));
            assert!(crate::analysis::expression_reads_name(
                value,
                "__mwcc_loop_scalar_cse_1"
            ));
        }
    }

    #[test]
    fn leaves_a_scale_alone_when_its_source_changes_between_uses() {
        let repeated = scale("a", 4);
        let source = function(vec![
            assign("first", repeated.clone()),
            assign("a", Expression::IntegerLiteral(3)),
            assign("second", repeated),
        ]);
        assert!(materialize_repeated_loop_scalars(&source).is_none());
    }

    #[test]
    fn leaves_a_scale_alone_when_the_first_assignment_changes_its_source() {
        let repeated = scale("a", 4);
        let source = function(vec![
            assign("a", repeated.clone()),
            assign("second", repeated),
        ]);
        assert!(materialize_repeated_loop_scalars(&source).is_none());
    }

    #[test]
    fn leaves_a_scale_alone_when_the_source_remains_live_between_uses() {
        let repeated = scale("a", 4);
        let source = function(vec![
            assign("first", repeated.clone()),
            Statement::Expression(Expression::Variable("a".into())),
            assign("second", repeated),
        ]);
        assert!(materialize_repeated_loop_scalars(&source).is_none());
    }

    #[test]
    fn leaves_a_scale_alone_when_the_second_expression_also_needs_the_source() {
        let repeated = scale("a", 4);
        let source = function(vec![
            assign("first", repeated.clone()),
            assign(
                "second",
                sum(repeated, Expression::Variable("a".into())),
            ),
        ]);
        assert!(materialize_repeated_loop_scalars(&source).is_none());
    }
}
