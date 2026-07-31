//! Make shared global-array store bases explicit in structured ASTs.
//!
//! Consecutive stores into one global array share an address materialization in
//! MWCC. A hygienic pointer local exposes that live range to the ordinary
//! structured allocator without coupling array recognition to instruction
//! selection.

use mwcc_syntax_trees::{
    ArmBody, Expression, Function, LocalDeclaration, Pointee, Statement, Type,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn materialize_consecutive_global_array_store_base(
    function: &Function,
    globals: &HashMap<String, Type>,
    global_arrays: &HashSet<String>,
    volatile_globals: &HashSet<String>,
) -> Option<Function> {
    let mut rewritten = function.clone();
    let mut changed = false;
    loop {
        let occupied: HashSet<_> = rewritten
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .chain(rewritten.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let local_name = unique_base_name(&occupied);
        let Some((global, element)) = materialize_first_cluster(
            &mut rewritten.statements,
            &local_name,
            globals,
            global_arrays,
            volatile_globals,
        ) else {
            break;
        };
        rewritten.locals.push(LocalDeclaration {
            declared_type: Type::Pointer(element),
            name: local_name,
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        });
        debug_assert!(globals.contains_key(&global));
        changed = true;
    }
    changed.then_some(rewritten)
}

fn materialize_first_cluster(
    statements: &mut Vec<Statement>,
    local_name: &str,
    globals: &HashMap<String, Type>,
    global_arrays: &HashSet<String>,
    volatile_globals: &HashSet<String>,
) -> Option<(String, Pointee)> {
    let mut start = 0;
    while start < statements.len() {
        if let Some((global, element)) =
            direct_global_array_store(&statements[start], globals, global_arrays, volatile_globals)
        {
            let end = statements[start..]
                .iter()
                .take_while(|statement| {
                    direct_global_array_store(statement, globals, global_arrays, volatile_globals)
                        .is_some_and(|(candidate, _)| candidate == global)
                })
                .count()
                + start;
            if end - start >= 2 {
                statements.insert(
                    start,
                    Statement::Assign {
                        name: local_name.to_owned(),
                        value: Expression::AddressOf {
                            operand: Box::new(Expression::Variable(global.clone())),
                        },
                    },
                );
                for statement in &mut statements[start + 1..=end] {
                    let Statement::Store {
                        target: Expression::Index { base, .. },
                        ..
                    } = statement
                    else {
                        unreachable!("the selected cluster contains only array stores");
                    };
                    *base = Box::new(Expression::Variable(local_name.to_owned()));
                }
                return Some((global, element));
            }
        }

        let nested = match &mut statements[start] {
            Statement::If {
                then_body,
                else_body,
                ..
            } => materialize_first_cluster(
                then_body,
                local_name,
                globals,
                global_arrays,
                volatile_globals,
            )
            .or_else(|| {
                materialize_first_cluster(
                    else_body,
                    local_name,
                    globals,
                    global_arrays,
                    volatile_globals,
                )
            }),
            Statement::Loop { body, .. } => materialize_first_cluster(
                body,
                local_name,
                globals,
                global_arrays,
                volatile_globals,
            ),
            Statement::Switch { arms, default, .. } => {
                let in_arm = arms.iter_mut().find_map(|arm| match &mut arm.body {
                    ArmBody::Statements(body) => materialize_first_cluster(
                        body,
                        local_name,
                        globals,
                        global_arrays,
                        volatile_globals,
                    ),
                    ArmBody::Return(_) => None,
                });
                in_arm.or_else(|| match default {
                    Some(ArmBody::Statements(body)) => materialize_first_cluster(
                        body,
                        local_name,
                        globals,
                        global_arrays,
                        volatile_globals,
                    ),
                    Some(ArmBody::Return(_)) | None => None,
                })
            }
            _ => None,
        };
        if nested.is_some() {
            return nested;
        }
        start += 1;
    }
    None
}

fn direct_global_array_store(
    statement: &Statement,
    globals: &HashMap<String, Type>,
    global_arrays: &HashSet<String>,
    volatile_globals: &HashSet<String>,
) -> Option<(String, Pointee)> {
    let Statement::Store {
        target: Expression::Index { base, index },
        ..
    } = statement
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    if !matches!(index.as_ref(), Expression::IntegerLiteral(_))
        || !global_arrays.contains(global)
        || volatile_globals.contains(global)
    {
        return None;
    }
    let element = scalar_pointee(*globals.get(global)?)?;
    Some((global.clone(), element))
}

fn scalar_pointee(value_type: Type) -> Option<Pointee> {
    Some(match value_type {
        Type::Int => Pointee::Int,
        Type::UnsignedInt => Pointee::UnsignedInt,
        Type::Char => Pointee::Char,
        Type::UnsignedChar => Pointee::UnsignedChar,
        Type::Short => Pointee::Short,
        Type::UnsignedShort => Pointee::UnsignedShort,
        Type::Float => Pointee::Float,
        Type::Double => Pointee::Double,
        Type::Pointer(_) | Type::StructPointer { .. } => Pointee::UnsignedInt,
        Type::Void | Type::Struct { .. } | Type::LongLong | Type::UnsignedLongLong => return None,
    })
}

fn unique_base_name(occupied: &HashSet<&str>) -> String {
    for ordinal in 0usize.. {
        let candidate = format!("__mwcc_global_array_store_base_{ordinal}");
        if !occupied.contains(candidate.as_str()) {
            return candidate;
        }
    }
    unreachable!("the finite function cannot occupy every generated name")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(index: i64, value: &str) -> Statement {
        Statement::Store {
            target: Expression::Index {
                base: Box::new(Expression::Variable("values".into())),
                index: Box::new(Expression::IntegerLiteral(index)),
            },
            value: Expression::Variable(value.into()),
        }
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "publish".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements,
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
    fn materializes_one_pointer_for_consecutive_global_array_stores() {
        let function = function(vec![store(0, "a"), store(1, "b"), store(2, "c")]);
        let globals = HashMap::from([("values".into(), Type::UnsignedInt)]);
        let arrays = HashSet::from(["values".into()]);

        let rewritten = materialize_consecutive_global_array_store_base(
            &function,
            &globals,
            &arrays,
            &HashSet::new(),
        )
        .expect("the store cluster should share one array base");

        assert_eq!(
            rewritten.locals[0].declared_type,
            Type::Pointer(Pointee::UnsignedInt)
        );
        assert!(matches!(
            rewritten.statements.as_slice(),
            [
                Statement::Assign { name, .. },
                Statement::Store {
                    target: Expression::Index { base: first, .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Index { base: second, .. },
                    ..
                },
                Statement::Store {
                    target: Expression::Index { base: third, .. },
                    ..
                },
            ] if name == "__mwcc_global_array_store_base_0"
                && [first, second, third].iter().all(|base| matches!(
                    base.as_ref(),
                    Expression::Variable(name)
                        if name == "__mwcc_global_array_store_base_0"
                ))
        ));
    }

    #[test]
    fn leaves_a_single_global_array_store_unmaterialized() {
        let function = function(vec![store(0, "a")]);
        let globals = HashMap::from([("values".into(), Type::UnsignedInt)]);
        let arrays = HashSet::from(["values".into()]);

        assert!(materialize_consecutive_global_array_store_base(
            &function,
            &globals,
            &arrays,
            &HashSet::new(),
        )
        .is_none());
    }
}
