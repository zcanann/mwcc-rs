//! Make shared global-aggregate store bases explicit in structured ASTs.
//!
//! MWCC materializes one address for a consecutive cluster of stores into the
//! same global struct. A hygienic pointer local exposes that live range to the
//! ordinary structured allocator and fixes its activation point after any
//! preceding call.

use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};
use std::collections::{HashMap, HashSet};

pub(crate) fn materialize_consecutive_global_struct_store_base(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<Function> {
    for start in 0..function.statements.len() {
        let Some(global) = direct_global_struct_store(
            function.statements.get(start)?,
            globals,
            volatile_globals,
        ) else {
            continue;
        };
        let end = function.statements[start..]
            .iter()
            .take_while(|statement| {
                direct_global_struct_store(statement, globals, volatile_globals)
                    .as_deref()
                    == Some(global.as_str())
            })
            .count()
            + start;
        if end - start < 2 {
            continue;
        }
        let Type::Struct { size, .. } = globals[&global] else {
            unreachable!("the store recognizer selected a struct global");
        };
        let occupied: HashSet<_> = function
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .chain(function.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let local_name = unique_base_name(&occupied);

        let mut rewritten = function.clone();
        rewritten.locals.push(LocalDeclaration {
            declared_type: Type::StructPointer { element_size: size },
            name: local_name.clone(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        rewritten.statements.insert(
            start,
            Statement::Assign {
                name: local_name.clone(),
                value: Expression::AddressOf {
                    operand: Box::new(Expression::Variable(global)),
                },
            },
        );
        for statement in &mut rewritten.statements[start + 1..=end] {
            let Statement::Store {
                target: Expression::Member { base, .. },
                ..
            } = statement
            else {
                unreachable!("the selected cluster contains only member stores");
            };
            *base = Box::new(Expression::Variable(local_name.clone()));
        }
        return Some(rewritten);
    }
    None
}

fn direct_global_struct_store(
    statement: &Statement,
    globals: &HashMap<String, Type>,
    volatile_globals: &HashSet<String>,
) -> Option<String> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                index_stride: None,
                ..
            },
        ..
    } = statement
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    (matches!(globals.get(global), Some(Type::Struct { .. }))
        && !volatile_globals.contains(global))
    .then(|| global.clone())
}

fn unique_base_name(occupied: &HashSet<&str>) -> String {
    for ordinal in 0usize.. {
        let candidate = format!("__mwcc_global_store_base_{ordinal}");
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the finite function cannot occupy every generated name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_one_base_at_the_consecutive_store_cluster() {
        let store = |offset| Statement::Store {
            target: Expression::Member {
                base: Box::new(Expression::Variable("state".into())),
                offset,
                member_type: Type::UnsignedInt,
                index_stride: None,
            },
            value: Expression::IntegerLiteral(0),
        };
        let function = Function {
            return_type: Type::Void,
            name: "publish".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![
                Statement::Expression(Expression::Call {
                    name: "read".into(),
                    arguments: Vec::new(),
                }),
                store(12),
                store(8),
            ],
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
        let globals =
            HashMap::from([("state".into(), Type::Struct { size: 16, align: 4 })]);

        let rewritten = materialize_consecutive_global_struct_store_base(
            &function,
            &globals,
            &HashSet::new(),
        )
        .expect("the two stores should share one base");
        assert_eq!(rewritten.locals.len(), 1);
        assert_eq!(rewritten.locals[0].name, "__mwcc_global_store_base_0");
        assert!(matches!(
            rewritten.statements.as_slice(),
            [
                Statement::Expression(Expression::Call { .. }),
                Statement::Assign {
                    value: Expression::AddressOf { .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Member { base: first, .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Member { base: second, .. },
                    ..
                },
            ] if matches!(
                first.as_ref(),
                Expression::Variable(name) if name == "__mwcc_global_store_base_0"
            ) && matches!(
                second.as_ref(),
                Expression::Variable(name) if name == "__mwcc_global_store_base_0"
            )
        ));
    }
}
