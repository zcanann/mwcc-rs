//! Saved-GPR roles for a guarded call-result publication loop.
//!
//! The loop acquires an object through one call, locks it, and conditionally
//! publishes both the object and loop index through two incoming pointers.
//! MWCC assigns registers by those roles rather than declaration class, keeping
//! the five values in one `r27..r31` save range.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::{LocalDeclaration, Parameter};

pub(super) struct StructuredGuardedCallPublicationLayout {
    preference_by_home: [u8; 5],
    save_order: [usize; 5],
}

impl StructuredGuardedCallPublicationLayout {
    pub(super) fn plan(
        function: &Function,
        eager_locals: &[&LocalDeclaration],
        saved_parameters: &[&Parameter],
        deferred_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
        parameter_reuse: &StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        let [result] = eager_locals else {
            return None;
        };
        let [first_output, second_output] = saved_parameters else {
            return None;
        };
        if deferred_locals.len() != 2
            || deferred.group_count != 2
            || parameter_reuse.fresh_group_count != 2
            || home_count != 5
            || !matches!(result.initializer, Some(Expression::IntegerLiteral(_)))
        {
            return None;
        }
        let [
            leading_store,
            Statement::Loop {
                initializer: Some(initializer),
                condition: Some(condition),
                step: Some(step),
                body,
                ..
            },
        ] = function.statements.as_slice()
        else {
            return None;
        };
        let object_output_name = zero_store_pointer(leading_store)?;
        let (index_output, object_output, index_output_home, object_output_home) =
            if object_output_name == first_output.name {
                (second_output, first_output, 2, 1)
            } else if object_output_name == second_output.name {
                (first_output, second_output, 1, 2)
            } else {
                return None;
            };
        let counter = initialized_counter(initializer)?;
        if !loop_condition_uses(condition, counter) || !loop_step_increments(step, counter) {
            return None;
        }
        let [
            Statement::Assign {
                name: object,
                value: Expression::Call { arguments, .. },
            },
            acquire,
            Statement::If {
                then_body,
                else_body,
                ..
            },
            release,
        ] = body.as_slice()
        else {
            return None;
        };
        if !else_body.is_empty()
            || arguments.len() != 1
            || !is_variable(&arguments[0], counter)
            || !call_uses(acquire, object)
            || !call_uses(release, object)
            || !then_body
                .iter()
                .any(|statement| stores_variable_through(statement, &object_output.name, object))
            || !then_body.iter().any(|statement| {
                stores_variable_through(statement, &index_output.name, counter)
            })
            || !then_body.iter().any(|statement| {
                matches!(statement, Statement::Assign { name, value: Expression::IntegerLiteral(_) }
                    if name == &result.name)
            })
        {
            return None;
        }
        if !deferred_locals
            .iter()
            .all(|local| local.name == counter || local.name == *object)
        {
            return None;
        }

        let counter_home = parameter_reuse.home_index(deferred.group(counter));
        let object_home = parameter_reuse.home_index(deferred.group(object));
        let mut preference_by_home = [0; 5];
        let assignments = [
            (0, 31),
            (index_output_home, 27),
            (object_output_home, 28),
            (counter_home, 30),
            (object_home, 29),
        ];
        let mut occupied = [false; 5];
        for (home, register) in assignments {
            if home >= occupied.len() || occupied[home] {
                return None;
            }
            occupied[home] = true;
            preference_by_home[home] = register;
        }
        if occupied.iter().any(|occupied| !occupied) {
            return None;
        }
        Some(Self {
            preference_by_home,
            save_order: [
                index_output_home,
                object_output_home,
                object_home,
                counter_home,
                0,
            ],
        })
    }

    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        self.preference_by_home.get(home_index).copied()
    }

    pub(super) fn save_order(&self) -> [usize; 5] {
        self.save_order
    }

    pub(super) fn frame_slot(&self, home_index: usize) -> Option<usize> {
        self.save_order
            .iter()
            .position(|candidate| *candidate == home_index)
    }
}

fn initialized_counter(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Assign { target, value }
            if matches!(value.as_ref(), Expression::IntegerLiteral(0)) =>
        {
            variable(target)
        }
        _ => None,
    }
}

fn loop_condition_uses(expression: &Expression, counter: &str) -> bool {
    matches!(expression, Expression::Binary { left, right, .. }
        if is_variable(left, counter) && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
}

fn loop_step_increments(expression: &Expression, counter: &str) -> bool {
    matches!(expression, Expression::Assign { target, value }
        if is_variable(target, counter)
            && matches!(value.as_ref(), Expression::Binary { left, right, .. }
                if is_variable(left, counter)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1))))
}

fn zero_store_pointer(statement: &Statement) -> Option<&str> {
    match statement {
        Statement::Store {
            target: Expression::Dereference { pointer: target },
            value,
        } if expression_is_zero(value) => variable(target),
        _ => None,
    }
}

fn expression_is_zero(expression: &Expression) -> bool {
    match expression {
        Expression::IntegerLiteral(0) => true,
        Expression::Cast { operand, .. } => expression_is_zero(operand),
        _ => false,
    }
}

fn stores_variable_through(statement: &Statement, pointer: &str, value: &str) -> bool {
    matches!(statement, Statement::Store {
        target: Expression::Dereference { pointer: target },
        value: stored,
    } if is_variable(target, pointer) && is_variable(stored, value))
}

fn call_uses(statement: &Statement, name: &str) -> bool {
    matches!(statement, Statement::Expression(Expression::Call { arguments, .. })
        if arguments.iter().any(|argument| is_variable(argument, name)))
}

fn is_variable(expression: &Expression, name: &str) -> bool {
    variable(expression) == Some(name)
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(name: &str, declared_type: Type, initializer: Option<Expression>) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer,
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

    fn call(name: &str, arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.into(),
            arguments,
        })
    }

    #[test]
    fn assigns_publication_roles_independently_of_saved_parameter_order() {
        let function = Function {
            return_type: Type::Int,
            name: "claim".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Int),
                    name: "index_output".into(),
                },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Pointer),
                    name: "object_output".into(),
                },
            ],
            locals: vec![
                local("result", Type::Int, Some(Expression::IntegerLiteral(768))),
                local("index", Type::Int, None),
                local("object", Type::StructPointer { element_size: 16 }, None),
            ],
            statements: vec![
                Statement::Store {
                    target: Expression::Dereference {
                        pointer: Box::new(Expression::Variable("object_output".into())),
                    },
                    value: Expression::Cast {
                        target_type: Type::Pointer(Pointee::Int),
                        operand: Box::new(Expression::IntegerLiteral(0)),
                    },
                },
                Statement::Loop {
                    kind: LoopKind::For,
                    initializer: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("index".into())),
                        value: Box::new(Expression::IntegerLiteral(0)),
                    }),
                    condition: Some(Expression::Binary {
                        operator: BinaryOperator::Less,
                        left: Box::new(Expression::Variable("index".into())),
                        right: Box::new(Expression::IntegerLiteral(3)),
                    }),
                    step: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("index".into())),
                        value: Box::new(Expression::Binary {
                            operator: BinaryOperator::Add,
                            left: Box::new(Expression::Variable("index".into())),
                            right: Box::new(Expression::IntegerLiteral(1)),
                        }),
                    }),
                    body: vec![
                        Statement::Assign {
                            name: "object".into(),
                            value: Expression::Call {
                                name: "produce".into(),
                                arguments: vec![Expression::Variable("index".into())],
                            },
                        },
                        call("acquire", vec![Expression::Variable("object".into())]),
                        Statement::If {
                            condition: Expression::Unary {
                                operator: UnaryOperator::LogicalNot,
                                operand: Box::new(Expression::Member {
                                    base: Box::new(Expression::Variable("object".into())),
                                    offset: 4,
                                    member_type: Type::Int,
                                    index_stride: None,
                                }),
                            },
                            then_body: vec![
                                Statement::Assign {
                                    name: "result".into(),
                                    value: Expression::IntegerLiteral(0),
                                },
                                Statement::Store {
                                    target: Expression::Dereference {
                                        pointer: Box::new(Expression::Variable(
                                            "object_output".into(),
                                        )),
                                    },
                                    value: Expression::Variable("object".into()),
                                },
                                Statement::Store {
                                    target: Expression::Dereference {
                                        pointer: Box::new(Expression::Variable(
                                            "index_output".into(),
                                        )),
                                    },
                                    value: Expression::Variable("index".into()),
                                },
                            ],
                            else_body: Vec::new(),
                        },
                        call("release", vec![Expression::Variable("object".into())]),
                    ],
                },
            ],
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
        let eager = vec![&function.locals[0]];
        let deferred_locals = vec![&function.locals[1], &function.locals[2]];
        let deferred = super::super::structured_locals::plan_deferred_saved_homes(
            &function,
            &deferred_locals,
        )
        .unwrap();
        let saved_parameters = vec![&function.parameters[1], &function.parameters[0]];
        let parameter_reuse = StructuredParameterHomeReuse::retain_distinct(1, 2, 2);

        let layout = StructuredGuardedCallPublicationLayout::plan(
            &function,
            &eager,
            &saved_parameters,
            &deferred_locals,
            &deferred,
            &parameter_reuse,
            5,
        )
        .unwrap();
        let counter_home = parameter_reuse.home_index(deferred.group("index"));
        let object_home = parameter_reuse.home_index(deferred.group("object"));

        assert_eq!(layout.preference(0), Some(31));
        assert_eq!(layout.preference(1), Some(28));
        assert_eq!(layout.preference(2), Some(27));
        assert_eq!(layout.preference(counter_home), Some(30));
        assert_eq!(layout.preference(object_home), Some(29));
    }
}
