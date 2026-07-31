//! Whole-function frame planning for dereferenced by-value aggregate arguments.
//!
//! The EABI passes these values through caller-owned copies. MWCC keeps every
//! compiler-created object distinct even when later inlining removes the calls,
//! and allocates the objects in reverse call and argument order.

#[allow(unused_imports)]
use super::*;

pub(super) fn plan_structured_by_value_aggregate_arguments(
    function: &Function,
    call_parameter_types: &std::collections::HashMap<String, Vec<Type>>,
) -> Option<StructuredByValueAggregatePlan> {
    let source_types: std::collections::HashMap<_, _> = function
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
    let mut calls = Vec::new();
    for statement in &function.statements {
        super::structured_expression_visit::visit_statement(statement, &mut |expression| {
            if let Expression::Call { name, arguments } = expression {
                calls.push((name.clone(), arguments.clone()));
            }
        });
    }
    for guard in &function.guards {
        for expression in [&guard.condition, &guard.value] {
            super::structured_expression_visit::visit_expression(expression, &mut |expression| {
                if let Expression::Call { name, arguments } = expression {
                    calls.push((name.clone(), arguments.clone()));
                }
            });
        }
    }
    if let Some(expression) = &function.return_expression {
        super::structured_expression_visit::visit_expression(expression, &mut |expression| {
            if let Expression::Call { name, arguments } = expression {
                calls.push((name.clone(), arguments.clone()));
            }
        });
    }

    plan_calls(calls, call_parameter_types, &source_types)
}

fn plan_calls(
    calls: Vec<(String, Vec<Expression>)>,
    call_parameter_types: &std::collections::HashMap<String, Vec<Type>>,
    source_types: &std::collections::HashMap<&str, Type>,
) -> Option<StructuredByValueAggregatePlan> {
    let mut planned_calls = Vec::new();
    for (callee, arguments) in calls {
        let Some(parameter_types) = call_parameter_types.get(&callee) else {
            continue;
        };
        if parameter_types.len() != arguments.len() {
            return None;
        }
        let mut copies = Vec::new();
        for (argument_index, (argument, parameter_type)) in
            arguments.iter().zip(parameter_types).enumerate()
        {
            let Type::Struct { size, align } = parameter_type else {
                continue;
            };
            if *size == 0 || *size % 4 != 0 || *align > 4 {
                return None;
            }
            let Expression::Dereference { pointer } = argument else {
                return None;
            };
            let Expression::Variable(source_pointer) = pointer.as_ref() else {
                return None;
            };
            let Some(Type::StructPointer { element_size }) =
                source_types.get(source_pointer.as_str())
            else {
                return None;
            };
            if *element_size != 0 && *element_size != *size {
                return None;
            }
            copies.push(StructuredByValueAggregateArgumentCopy {
                argument_index,
                source_pointer: source_pointer.clone(),
                copy_offset: 0,
                size: *size,
            });
        }
        if !copies.is_empty() {
            planned_calls.push(StructuredByValueAggregateCall { callee, copies });
        }
    }
    if planned_calls.is_empty() {
        return None;
    }

    let mut cursor = 8i16;
    for call in planned_calls.iter_mut().rev() {
        for copy in call.copies.iter_mut().rev() {
            copy.copy_offset = cursor;
            cursor = cursor.checked_add(i16::try_from(copy.size).ok()?)?;
        }
    }
    Some(StructuredByValueAggregatePlan {
        calls: planned_calls,
        next_call: 0,
        total_bytes: cursor.checked_sub(8)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserves_distinct_objects_in_reverse_call_and_argument_order() {
        let dereference = |name: &str| Expression::Dereference {
            pointer: Box::new(Expression::Variable(name.into())),
        };
        let first = vec![dereference("a"), dereference("b"), dereference("c")];
        let second = vec![dereference("b"), dereference("c")];
        let parameters = std::collections::HashMap::from([
            ("first".into(), vec![Type::Struct { size: 12, align: 4 }; 3]),
            (
                "second".into(),
                vec![Type::Struct { size: 12, align: 4 }; 2],
            ),
        ]);
        let source_types = std::collections::HashMap::from([
            ("a", Type::StructPointer { element_size: 12 }),
            ("b", Type::StructPointer { element_size: 12 }),
            ("c", Type::StructPointer { element_size: 12 }),
        ]);

        let plan = plan_calls(
            vec![("first".into(), first), ("second".into(), second)],
            &parameters,
            &source_types,
        )
        .unwrap();

        assert_eq!(plan.total_bytes, 60);
        assert_eq!(
            plan.calls[0]
                .copies
                .iter()
                .map(|copy| copy.copy_offset)
                .collect::<Vec<_>>(),
            vec![56, 44, 32],
        );
        assert_eq!(
            plan.calls[1]
                .copies
                .iter()
                .map(|copy| copy.copy_offset)
                .collect::<Vec<_>>(),
            vec![20, 8],
        );
    }
}
