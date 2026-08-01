//! Statement linearization for automatically inlined global scalar helpers.
//!
//! Value composition represents an effectful helper as a comma expression.
//! When that value is nested in otherwise-pure arithmetic, its proven global
//! updates can be restored to ordinary statements at the same source point.
//! This keeps stateful sequencing out of numeric conversion and lets statement
//! lowering schedule the stores exactly as it does in the original helper.

use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};
use std::collections::HashSet;

pub(super) fn linearize(
    function: &Function,
    globals: &HashSet<String>,
) -> (Function, Vec<String>) {
    let mut function = function.clone();
    let mut allocator = ResultHomeAllocator::new(&function);
    function.statements = linearize_statements(&function.statements, globals, &mut allocator);
    function.locals.extend(allocator.locals);
    (function, allocator.homes)
}

fn linearize_statements(
    statements: &[Statement],
    globals: &HashSet<String>,
    allocator: &mut ResultHomeAllocator,
) -> Vec<Statement> {
    let mut output = Vec::new();
    for statement in statements {
        match statement {
            Statement::Assign { name, value } => {
                if let Some((effects, value)) = extract(value, globals, allocator) {
                    output.extend(effects);
                    output.push(Statement::Assign {
                        name: name.clone(),
                        value,
                    });
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
                then_body: linearize_statements(then_body, globals, allocator),
                else_body: linearize_statements(else_body, globals, allocator),
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
                body: linearize_statements(body, globals, allocator),
            }),
            _ => output.push(statement.clone()),
        }
    }
    output
}

fn extract(
    expression: &Expression,
    globals: &HashSet<String>,
    allocator: &mut ResultHomeAllocator,
) -> Option<(Vec<Statement>, Expression)> {
    match expression {
        Expression::Comma { left, right } => {
            let mut effects = discarded_effects(left, globals)?;
            let value = if let Some((right_effects, value)) = extract(right, globals, allocator) {
                effects.extend(right_effects);
                value
            } else {
                let (homes, value) = allocator.materialize(right);
                effects.extend(homes);
                value
            };
            Some((effects, value))
        }
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            if !crate::analysis::expression_has_side_effect(right) {
                if let Some((effects, value)) = extract(left, globals, allocator) {
                    return Some((
                        effects,
                        Expression::Binary {
                            operator: *operator,
                            left: Box::new(value),
                            right: right.clone(),
                        },
                    ));
                }
            }
            if !crate::analysis::expression_has_side_effect(left) {
                if let Some((effects, value)) = extract(right, globals, allocator) {
                    return Some((
                        effects,
                        Expression::Binary {
                            operator: *operator,
                            left: left.clone(),
                            right: Box::new(value),
                        },
                    ));
                }
            }
            None
        }
        Expression::Cast {
            target_type,
            operand,
        } => extract(operand, globals, allocator).map(|(effects, value)| {
            (
                effects,
                Expression::Cast {
                    target_type: *target_type,
                    operand: Box::new(value),
                },
            )
        }),
        _ => None,
    }
}

struct ResultHomeAllocator {
    occupied: HashSet<String>,
    next_id: usize,
    locals: Vec<LocalDeclaration>,
    homes: Vec<String>,
}

impl ResultHomeAllocator {
    fn new(function: &Function) -> Self {
        Self {
            occupied: function
                .parameters
                .iter()
                .map(|parameter| parameter.name.clone())
                .chain(function.locals.iter().map(|local| local.name.clone()))
                .collect(),
            next_id: 0,
            locals: Vec::new(),
            homes: Vec::new(),
        }
    }

    fn materialize(&mut self, value: &Expression) -> (Vec<Statement>, Expression) {
        let value_type = match value {
            Expression::Cast { target_type, .. } => *target_type,
            _ => Type::Int,
        };
        let source = self.fresh_name("source");
        let consumer = self.fresh_name("consumer");
        for name in [&source, &consumer] {
            self.locals.push(LocalDeclaration {
                declared_type: value_type,
                name: name.clone(),
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
            self.homes.push(name.clone());
        }
        (
            vec![
                Statement::Assign {
                    name: source.clone(),
                    value: value.clone(),
                },
                Statement::Assign {
                    name: consumer.clone(),
                    value: Expression::Variable(source),
                },
            ],
            Expression::Variable(consumer),
        )
    }

    fn fresh_name(&mut self, role: &str) -> String {
        loop {
            let name = format!("__mwcc_global_result_{}_{}", self.next_id, role);
            self.next_id += 1;
            if self.occupied.insert(name.clone()) {
                return name;
            }
        }
    }
}

fn discarded_effects(expression: &Expression, globals: &HashSet<String>) -> Option<Vec<Statement>> {
    match expression {
        Expression::Comma { left, right } => {
            let mut effects = discarded_effects(left, globals)?;
            effects.extend(discarded_effects(right, globals)?);
            Some(effects)
        }
        Expression::Assign { target, value } => {
            let Expression::Variable(name) = target.as_ref() else {
                return None;
            };
            globals.contains(name).then(|| {
                vec![Statement::Store {
                    target: Expression::Variable(name.clone()),
                    value: value.as_ref().clone(),
                }]
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, Type};

    #[test]
    fn hoists_ordered_global_updates_from_pure_arithmetic() {
        let global = || Expression::Variable("state".into());
        let transaction = Expression::Comma {
            left: Box::new(Expression::Assign {
                target: Box::new(global()),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    left: Box::new(global()),
                    right: Box::new(Expression::IntegerLiteral(3)),
                }),
            }),
            right: Box::new(Expression::Comma {
                left: Box::new(Expression::Assign {
                    target: Box::new(global()),
                    value: Box::new(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left: Box::new(global()),
                        right: Box::new(Expression::IntegerLiteral(1)),
                    }),
                }),
                right: Box::new(global()),
            }),
        };
        let function = Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: vec![Statement::Assign {
                name: "result".into(),
                value: Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    left: Box::new(Expression::Variable("scale".into())),
                    right: Box::new(transaction),
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

        let (linearized, result_homes) =
            linearize(&function, &HashSet::from(["state".into()]));
        assert!(matches!(linearized.statements.as_slice(), [
            Statement::Store { .. },
            Statement::Store { .. },
            Statement::Assign { name: source, .. },
            Statement::Assign { name: consumer, value: Expression::Variable(forwarded) },
            Statement::Assign { value: Expression::Binary { right, .. }, .. },
        ] if source == &result_homes[0]
            && consumer == &result_homes[1]
            && forwarded == source
            && matches!(right.as_ref(), Expression::Variable(name) if name == consumer)));
        assert_eq!(result_homes.len(), 2);
    }
}
