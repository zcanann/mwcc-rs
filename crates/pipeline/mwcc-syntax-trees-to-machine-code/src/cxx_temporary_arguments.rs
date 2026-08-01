//! Materialization of functional C++ temporaries used as call arguments.
//!
//! The frontend represents `Class(args)` with the resolved constructor symbol
//! but without its implicit placement address.  Before frame planning, turn
//! each such argument into explicit object storage and a pointer-valued
//! constructor result.  Keeping this as a syntax-tree normalization lets the
//! ordinary liveness, saved-register, and call-argument schedulers reproduce
//! MWCC's right-to-left argument evaluation without a constructor-specific
//! instruction emitter.

use mwcc_syntax_trees::{
    ArmBody, Expression, Function, LocalDeclaration, Statement, Type,
};
use std::collections::{HashMap, HashSet};

pub(crate) fn materialize(
    function: &Function,
    call_return_types: &HashMap<String, Type>,
    call_parameter_types: &HashMap<String, Vec<Type>>,
) -> Option<Function> {
    let mut output = function.clone();
    let mut occupied = output
        .parameters
        .iter()
        .map(|parameter| parameter.name.clone())
        .chain(output.locals.iter().map(|local| local.name.clone()))
        .collect::<HashSet<_>>();
    let mut next_id = 0usize;
    let mut changed = false;
    output.statements = materialize_statements(
        &output.statements,
        &mut output.locals,
        &mut occupied,
        &mut next_id,
        &mut changed,
        call_return_types,
        call_parameter_types,
    );
    changed.then_some(output)
}

#[allow(clippy::too_many_arguments)]
fn materialize_statements(
    statements: &[Statement],
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next_id: &mut usize,
    changed: &mut bool,
    call_return_types: &HashMap<String, Type>,
    call_parameter_types: &HashMap<String, Vec<Type>>,
) -> Vec<Statement> {
    let mut output = Vec::new();
    for statement in statements {
        let mut prefix = Vec::new();
        let statement = match statement {
            Statement::Expression(expression) => Statement::Expression(materialize_expression(
                expression,
                &mut prefix,
                locals,
                occupied,
                next_id,
                changed,
                call_return_types,
                call_parameter_types,
            )),
            Statement::Assign { name, value } => Statement::Assign {
                name: name.clone(),
                value: materialize_expression(
                    value,
                    &mut prefix,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
            },
            Statement::Store { target, value } => Statement::Store {
                target: materialize_expression(
                    target,
                    &mut prefix,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
                value: materialize_expression(
                    value,
                    &mut prefix,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
            },
            Statement::If {
                condition,
                then_body,
                else_body,
            } => Statement::If {
                condition: materialize_expression(
                    condition,
                    &mut prefix,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
                then_body: materialize_statements(
                    then_body,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
                else_body: materialize_statements(
                    else_body,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
            },
            Statement::Return(value) => Statement::Return(value.as_ref().map(|value| {
                materialize_expression(
                    value,
                    &mut prefix,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                )
            })),
            Statement::Switch {
                scrutinee,
                arms,
                default,
            } => Statement::Switch {
                scrutinee: materialize_expression(
                    scrutinee,
                    &mut prefix,
                    locals,
                    occupied,
                    next_id,
                    changed,
                    call_return_types,
                    call_parameter_types,
                ),
                arms: arms
                    .iter()
                    .map(|arm| {
                        let mut arm = arm.clone();
                        arm.body = materialize_arm_body(
                            &arm.body,
                            locals,
                            occupied,
                            next_id,
                            changed,
                            call_return_types,
                            call_parameter_types,
                        );
                        arm
                    })
                    .collect(),
                default: default.as_ref().map(|body| {
                    materialize_arm_body(
                        body,
                        locals,
                        occupied,
                        next_id,
                        changed,
                        call_return_types,
                        call_parameter_types,
                    )
                }),
            },
            Statement::Loop { body, .. } => {
                let mut statement = statement.clone();
                if let Statement::Loop { body: output_body, .. } = &mut statement {
                    *output_body = materialize_statements(
                        body,
                        locals,
                        occupied,
                        next_id,
                        changed,
                        call_return_types,
                        call_parameter_types,
                    );
                }
                statement
            }
            Statement::InlineAsm(_)
            | Statement::Break
            | Statement::Continue
            | Statement::Goto(_)
            | Statement::Label(_) => statement.clone(),
        };
        output.extend(prefix);
        output.push(statement);
    }
    output
}

#[allow(clippy::too_many_arguments)]
fn materialize_arm_body(
    body: &ArmBody,
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next_id: &mut usize,
    changed: &mut bool,
    call_return_types: &HashMap<String, Type>,
    call_parameter_types: &HashMap<String, Vec<Type>>,
) -> ArmBody {
    match body {
        ArmBody::Statements(statements) => ArmBody::Statements(materialize_statements(
            statements,
            locals,
            occupied,
            next_id,
            changed,
            call_return_types,
            call_parameter_types,
        )),
        // A temporary-bearing return arm needs a statement body so its
        // constructor calls remain inside the selected switch edge.
        ArmBody::Return(value) => {
            let mut prefix = Vec::new();
            let value = materialize_expression(
                value,
                &mut prefix,
                locals,
                occupied,
                next_id,
                changed,
                call_return_types,
                call_parameter_types,
            );
            if prefix.is_empty() {
                ArmBody::Return(value)
            } else {
                prefix.push(Statement::Return(Some(value)));
                ArmBody::Statements(prefix)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn materialize_expression(
    expression: &Expression,
    prefix: &mut Vec<Statement>,
    locals: &mut Vec<LocalDeclaration>,
    occupied: &mut HashSet<String>,
    next_id: &mut usize,
    changed: &mut bool,
    call_return_types: &HashMap<String, Type>,
    call_parameter_types: &HashMap<String, Vec<Type>>,
) -> Expression {
    let Expression::Call { name, arguments } = expression else {
        return expression.clone();
    };
    let mut arguments = arguments.clone();
    for index in (0..arguments.len()).rev() {
        let Some((constructor, constructor_arguments, element_size)) =
            functional_constructor(
                &arguments[index],
                call_return_types,
                call_parameter_types,
            )
        else {
            continue;
        };
        let storage = unique_name("__mwcc_temporary_storage", occupied, next_id);
        let result = unique_name("__mwcc_temporary_result", occupied, next_id);
        locals.push(local(
            storage.clone(),
            Type::Struct {
                size: element_size,
                align: 4,
            },
        ));
        locals.push(local(
            result.clone(),
            Type::StructPointer { element_size },
        ));
        let mut call_arguments = Vec::with_capacity(constructor_arguments.len() + 1);
        call_arguments.push(Expression::AddressOf {
            operand: Box::new(Expression::Variable(storage)),
        });
        call_arguments.extend(constructor_arguments.iter().cloned());
        prefix.push(Statement::Assign {
            name: result.clone(),
            value: Expression::Call {
                name: constructor.to_owned(),
                arguments: call_arguments,
            },
        });
        arguments[index] = Expression::Variable(result);
        *changed = true;
    }
    Expression::Call {
        name: name.clone(),
        arguments,
    }
}

fn functional_constructor<'a>(
    expression: &'a Expression,
    call_return_types: &HashMap<String, Type>,
    call_parameter_types: &HashMap<String, Vec<Type>>,
) -> Option<(&'a str, &'a [Expression], u32)> {
    let Expression::Call { name, arguments } = expression else {
        return None;
    };
    let Type::StructPointer { element_size } = call_return_types.get(name)? else {
        return None;
    };
    let parameters = call_parameter_types.get(name)?;
    (name.starts_with("__ct__")
        && parameters.len() == arguments.len() + 1
        && matches!(
            parameters.first(),
            Some(Type::StructPointer {
                element_size: parameter_size
            }) if parameter_size == element_size
        ))
    .then_some((name.as_str(), arguments.as_slice(), *element_size))
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
    use mwcc_syntax_trees::Parameter;

    #[test]
    fn materializes_constructor_arguments_right_to_left() {
        let function = Function {
            return_type: Type::Void,
            name: "run".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Pointer(mwcc_syntax_trees::Pointee::UnsignedChar),
                name: "object".into(),
            }],
            locals: Vec::new(),
            statements: vec![Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![
                    Expression::Variable("object".into()),
                    Expression::Call {
                        name: "__ct__4InfoFi".into(),
                        arguments: vec![Expression::IntegerLiteral(1)],
                    },
                    Expression::Call {
                        name: "__ct__4InfoFi".into(),
                        arguments: vec![Expression::IntegerLiteral(2)],
                    },
                ],
            })],
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
        let return_types = HashMap::from([(
            "__ct__4InfoFi".into(),
            Type::StructPointer { element_size: 8 },
        )]);
        let parameter_types = HashMap::from([(
            "__ct__4InfoFi".into(),
            vec![Type::StructPointer { element_size: 8 }, Type::Int],
        )]);

        let materialized = materialize(&function, &return_types, &parameter_types).unwrap();
        assert_eq!(materialized.locals.len(), 4);
        assert!(matches!(
            materialized.statements.as_slice(),
            [
                Statement::Assign {
                    value: Expression::Call { arguments: first, .. },
                    ..
                },
                Statement::Assign {
                    value: Expression::Call { arguments: second, .. },
                    ..
                },
                Statement::Expression(Expression::Call { arguments, .. })
            ] if matches!(first.as_slice(), [Expression::AddressOf { .. }, Expression::IntegerLiteral(2)])
                && matches!(second.as_slice(), [Expression::AddressOf { .. }, Expression::IntegerLiteral(1)])
                && matches!(arguments.as_slice(), [Expression::Variable(object), Expression::Variable(first), Expression::Variable(second)]
                    if object == "object" && first.ends_with("_3") && second.ends_with("_1"))
        ));
    }
}
