//! Semantic frame temporaries for non-mutating three-float products.
//!
//! An expression such as `destination = source * scale` enters the executable
//! IR as one aggregate store. MWCC's inlined `Vector3f::operator*` retains an
//! eight-byte scratch object and a distinct twelve-byte return object, then
//! word-copies that return object into the destination. Make those objects
//! explicit before frame planning so ordinary scalar stores, liveness, and
//! aggregate-copy lowering own their respective jobs.

use mwcc_syntax_trees::{
    BinaryOperator, Expression, Function, LocalDeclaration, Statement, Type,
};
use std::collections::HashSet;

pub(crate) fn materialize(function: &Function) -> Option<Function> {
    let mut output = function.clone();
    let mut occupied = output
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(output.locals.iter().map(|local| local.name.clone()))
        .collect::<HashSet<_>>();
    let mut next_id = 0usize;
    let mut changed = false;
    output.statements = rewrite_statements(
        &output.statements,
        &mut output.locals,
        &mut occupied,
        &mut next_id,
        &mut changed,
    );
    changed.then_some(output)
}

fn rewrite_statements(
    statements: &[Statement],
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next_id: &mut usize,
    changed: &mut bool,
) -> Vec<Statement> {
    let mut output = Vec::new();
    for statement in statements {
        match statement {
            Statement::Store { target, value } => {
                if let Some(replacement) = materialize_store(
                    target,
                    value,
                    locals,
                    occupied,
                    next_id,
                ) {
                    output.extend(replacement);
                    *changed = true;
                } else {
                    output.push(statement.clone());
                }
            }
            Statement::If {
                condition,
                then_body,
                else_body,
            } => output.push(Statement::If {
                condition: condition.clone(),
                then_body: rewrite_statements(
                    then_body,
                    locals,
                    occupied,
                    next_id,
                    changed,
                ),
                else_body: rewrite_statements(
                    else_body,
                    locals,
                    occupied,
                    next_id,
                    changed,
                ),
            }),
            Statement::Loop {
                kind,
                initializer,
                condition,
                step,
                body,
            } => output.push(Statement::Loop {
                kind: *kind,
                initializer: initializer.clone(),
                condition: condition.clone(),
                step: step.clone(),
                body: rewrite_statements(
                    body,
                    locals,
                    occupied,
                    next_id,
                    changed,
                ),
            }),
            _ => output.push(statement.clone()),
        }
    }
    output
}

fn materialize_store(
    target: &Expression,
    value: &Expression,
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next_id: &mut usize,
) -> Option<Vec<Statement>> {
    if !matches!(
        target,
        Expression::Member {
            member_type: Type::Struct { size: 12, .. },
            index_stride: None,
            ..
        }
    ) {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = value
    else {
        return None;
    };
    let (source, scalar) = if matches!(
        left.as_ref(),
        Expression::Member {
            member_type: Type::Struct { size: 12, .. },
            index_stride: None,
            ..
        }
    ) {
        (left.as_ref(), right.as_ref())
    } else if matches!(
        right.as_ref(),
        Expression::Member {
            member_type: Type::Struct { size: 12, .. },
            index_stride: None,
            ..
        }
    ) {
        (right.as_ref(), left.as_ref())
    } else {
        return None;
    };
    if crate::analysis::structurally_equal(source, target) {
        return None;
    }
    let Expression::Member {
        base: source_base,
        offset: source_offset,
        ..
    } = source
    else {
        return None;
    };
    source_offset.checked_add(8)?;
    let scale = unique_name("__mwcc_vec3_scale", occupied, next_id);
    let result = unique_name("__mwcc_vec3_result", occupied, next_id);
    let scratch = unique_name("__mwcc_vec3_scratch", occupied, next_id);
    // Reverse-declaration frame layout places the eight-byte scratch below the
    // result object: scratch@24 and result@32 in the measured mainline frame.
    locals.push(local(result.clone(), Type::Struct { size: 12, align: 4 }));
    locals.push(local(scratch.clone(), Type::Struct { size: 8, align: 4 }));
    locals.push(local(scale.clone(), Type::Float));

    let source_lane = |lane: u32| Expression::Member {
        base: source_base.clone(),
        offset: source_offset + lane,
        member_type: Type::Float,
        index_stride: None,
    };
    let temporary_lane = |name: &str, lane: u32| Expression::Member {
        base: Box::new(Expression::Variable(name.to_owned())),
        offset: lane,
        member_type: Type::Float,
        index_stride: None,
    };
    let product = |lane| Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: Box::new(source_lane(lane)),
        right: Box::new(Expression::Variable(scale.clone())),
    };
    Some(vec![
        Statement::Assign {
            name: scale.clone(),
            value: scalar.clone(),
        },
        Statement::Store {
            target: temporary_lane(&scratch, 0),
            value: product(0),
        },
        Statement::Store {
            target: temporary_lane(&result, 0),
            value: temporary_lane(&scratch, 0),
        },
        Statement::Store {
            target: temporary_lane(&result, 4),
            value: product(4),
        },
        Statement::Store {
            target: temporary_lane(&result, 8),
            value: product(8),
        },
        Statement::Store {
            target: target.clone(),
            value: Expression::Variable(result),
        },
    ])
}

fn unique_name(prefix: &str, occupied: &mut HashSet<String>, next_id: &mut usize) -> String {
    loop {
        let name = format!("{prefix}_{}", *next_id);
        *next_id += 1;
        if occupied.insert(name.clone()) {
            return name;
        }
    }
}

fn local(name: String, declared_type: Type) -> LocalDeclaration {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn member(base: &str, offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset,
            member_type: Type::Struct { size: 12, align: 4 },
            index_stride: None,
        }
    }

    #[test]
    fn materializes_distinct_vec3_product_storage() {
        let mut function = Function {
            return_type: Type::Void,
            name: "scale".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Store {
                target: member("destination", 112),
                value: Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    left: Box::new(member("source", 24)),
                    right: Box::new(Expression::FloatLiteral(150.0)),
                },
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
        let materialized = materialize(&function).unwrap();
        function = materialized;
        assert_eq!(function.locals.len(), 3);
        assert_eq!(function.statements.len(), 6);
        assert!(matches!(
            function.statements.last(),
            Some(Statement::Store {
                value: Expression::Variable(name),
                ..
            }) if name.starts_with("__mwcc_vec3_result")
        ));
    }
}
