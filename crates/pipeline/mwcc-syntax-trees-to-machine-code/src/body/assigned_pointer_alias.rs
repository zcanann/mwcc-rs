//! Copy propagation for a pure pointer address assigned inside one control-flow arm.
//!
//! Value-returning inline accessors can turn `alias = accessor(object)` into a
//! single `alias = &object.member` assignment.  The declaration-time alias pass
//! cannot see that shape because the local has no initializer.  This owner
//! removes the assignment only when it dominates every read in its lexical
//! block and the address base is immutable for the complete function.

use std::collections::HashMap;

use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Statement, Type};

pub(super) fn fold_single_assignment_derived_pointer_alias(
    function: &Function,
) -> Option<Function> {
    let address_taken = crate::frame::collect_address_taken(function);
    for local in &function.locals {
        if local.initializer.is_some()
            || !matches!(
                local.declared_type,
                Type::Pointer(_) | Type::StructPointer { .. }
            )
            || address_taken.contains(local.name.as_str())
        {
            continue;
        }
        let Some((replacement, base_name)) =
            unique_derived_address_assignment(&function.statements, &local.name)
        else {
            continue;
        };
        if super::callee_saved::read_after_possible_call_in_return(
            &function.statements,
            function.return_expression.as_ref(),
            &local.name,
        ) {
            // A derived subobject address that crosses a call owns a real
            // callee-saved live range. Folding it into its later member reads
            // would recompute the address after the call and hide that range
            // from structured home planning.
            continue;
        }
        if function_assigns_name(function, base_name) {
            continue;
        }
        let mut replacements = HashMap::new();
        replacements.insert(local.name.clone(), replacement);
        let Some(statements) =
            rewrite_dominated_block(&function.statements, &local.name, &replacements)
        else {
            continue;
        };
        return Some(Function {
            locals: function
                .locals
                .iter()
                .filter(|candidate| candidate.name != local.name)
                .cloned()
                .collect(),
            statements,
            ..function.clone()
        });
    }
    None
}

fn unique_derived_address_assignment<'a>(
    statements: &'a [Statement],
    name: &str,
) -> Option<(Expression, &'a str)> {
    fn visit<'a>(
        statements: &'a [Statement],
        name: &str,
        found: &mut Option<&'a Expression>,
    ) -> bool {
        for statement in statements {
            match statement {
                Statement::Assign {
                    name: assigned,
                    value,
                } if assigned == name => {
                    if found.replace(value).is_some() {
                        return false;
                    }
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    if crate::analysis::expression_assigns_name(condition, name)
                        || !visit(then_body, name, found)
                        || !visit(else_body, name, found)
                    {
                        return false;
                    }
                }
                Statement::Store { target, value } => {
                    if crate::analysis::expression_assigns_name(target, name)
                        || crate::analysis::expression_assigns_name(value, name)
                    {
                        return false;
                    }
                }
                Statement::Assign { value, .. } => {
                    if crate::analysis::expression_assigns_name(value, name) {
                        return false;
                    }
                }
                Statement::Expression(expression) | Statement::Return(Some(expression)) => {
                    if crate::analysis::expression_assigns_name(expression, name) {
                        return false;
                    }
                }
                // A loop or switch assignment needs CFG dominance rather than
                // lexical dominance. Leave those shapes to their owners.
                Statement::Loop { .. } | Statement::Switch { .. } => {
                    if statement_reads_name(statement, name) {
                        return false;
                    }
                }
                Statement::InlineAsm(_)
                | Statement::Return(None)
                | Statement::Break
                | Statement::Continue
                | Statement::Goto(_)
                | Statement::Label(_) => {}
            }
        }
        true
    }

    let mut found = None;
    if !visit(statements, name, &mut found) {
        return None;
    }
    let value = found?;
    let base_name = derived_address_base(value)?;
    Some((value.clone(), base_name))
}

fn derived_address_base(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::MemberAddress {
            base,
            index_stride: None,
            ..
        } => variable_through_cast(base),
        Expression::AddressOf { operand } => match operand.as_ref() {
            Expression::Member {
                base,
                member_type: Type::Struct { .. },
                index_stride: None,
                ..
            } => variable_through_cast(base),
            _ => None,
        },
        _ => None,
    }
}

fn variable_through_cast(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        Expression::Cast { operand, .. } => variable_through_cast(operand),
        _ => None,
    }
}

fn rewrite_dominated_block(
    statements: &[Statement],
    name: &str,
    replacements: &HashMap<String, Expression>,
) -> Option<Vec<Statement>> {
    if let Some(assignment_index) = statements.iter().position(
        |statement| matches!(statement, Statement::Assign { name: assigned, .. } if assigned == name),
    ) {
        if statements[..assignment_index]
            .iter()
            .any(|statement| statement_reads_name(statement, name))
            || !statements[assignment_index + 1..]
                .iter()
                .any(|statement| statement_reads_name(statement, name))
        {
            return None;
        }
        let mut rewritten = statements[..assignment_index].to_vec();
        rewritten.extend(
            statements[assignment_index + 1..]
                .iter()
                .map(|statement| substitute_statement(statement, replacements)),
        );
        return Some(rewritten);
    }

    for (index, statement) in statements.iter().enumerate() {
        let Statement::If {
            condition,
            then_body,
            else_body,
        } = statement
        else {
            continue;
        };
        if crate::analysis::expression_reads_name(condition, name) {
            return None;
        }
        let rewritten_then = rewrite_dominated_block(then_body, name, replacements);
        let rewritten_else = rewrite_dominated_block(else_body, name, replacements);
        let (then_body, else_body) = match (rewritten_then, rewritten_else) {
            (Some(then_body), None)
                if !else_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name)) =>
            {
                (then_body, else_body.clone())
            }
            (None, Some(else_body))
                if !then_body
                    .iter()
                    .any(|statement| statement_reads_name(statement, name)) =>
            {
                (then_body.clone(), else_body)
            }
            _ => continue,
        };
        if statements[..index]
            .iter()
            .chain(&statements[index + 1..])
            .any(|statement| statement_reads_name(statement, name))
        {
            return None;
        }
        let mut rewritten = statements.to_vec();
        rewritten[index] = Statement::If {
            condition: condition.clone(),
            then_body,
            else_body,
        };
        return Some(rewritten);
    }
    None
}

fn substitute_statement(
    statement: &Statement,
    replacements: &HashMap<String, Expression>,
) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: crate::value_tracking::substitute(target, replacements),
            value: crate::value_tracking::substitute(value, replacements),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: crate::value_tracking::substitute(value, replacements),
        },
        Statement::Expression(expression) => {
            Statement::Expression(crate::value_tracking::substitute(expression, replacements))
        }
        Statement::If {
            condition,
            then_body,
            else_body,
        } => Statement::If {
            condition: crate::value_tracking::substitute(condition, replacements),
            then_body: then_body
                .iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
            else_body: else_body
                .iter()
                .map(|statement| substitute_statement(statement, replacements))
                .collect(),
        },
        Statement::Return(value) => Statement::Return(
            value
                .as_ref()
                .map(|value| crate::value_tracking::substitute(value, replacements)),
        ),
        // The recognizer rejects aliases whose dominated suffix contains these
        // control-flow forms, so cloning is sufficient here.
        other => other.clone(),
    }
}

fn function_assigns_name(function: &Function, name: &str) -> bool {
    function
        .statements
        .iter()
        .any(|statement| statement_assigns_name(statement, name))
        || function.guards.iter().any(|guard| {
            crate::analysis::expression_assigns_name(&guard.condition, name)
                || crate::analysis::expression_assigns_name(&guard.value, name)
        })
        || function
            .return_expression
            .as_ref()
            .is_some_and(|value| crate::analysis::expression_assigns_name(value, name))
}

fn statement_assigns_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            crate::analysis::expression_assigns_name(target, name)
                || crate::analysis::expression_assigns_name(value, name)
        }
        Statement::Assign {
            name: assigned,
            value,
        } => assigned == name || crate::analysis::expression_assigns_name(value, name),
        Statement::Expression(expression) | Statement::Return(Some(expression)) => {
            crate::analysis::expression_assigns_name(expression, name)
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
                .any(|expression| crate::analysis::expression_assigns_name(expression, name))
                || body
                    .iter()
                    .any(|statement| statement_assigns_name(statement, name))
        }
        Statement::Switch { .. } => statement_reads_name(statement, name),
        Statement::InlineAsm(_)
        | Statement::Return(None)
        | Statement::Break
        | Statement::Continue
        | Statement::Goto(_)
        | Statement::Label(_) => false,
    }
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    let function = Function {
        return_type: Type::Void,
        name: String::new(),
        is_static: true,
        is_weak: false,
        parameters: Vec::new(),
        locals: Vec::<LocalDeclaration>::new(),
        statements: vec![statement.clone()],
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
    crate::analysis::function_uses_name(&function, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{BinaryOperator, Parameter};

    #[test]
    fn folds_a_derived_address_assigned_inside_its_only_arm() {
        let pointer = Type::StructPointer { element_size: 64 };
        let function = Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: pointer,
                name: "object".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: pointer,
                name: "alias".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![Statement::If {
                condition: Expression::Binary {
                    operator: BinaryOperator::NotEqual,
                    left: Box::new(Expression::Variable("object".into())),
                    right: Box::new(Expression::IntegerLiteral(0)),
                },
                then_body: vec![
                    Statement::Assign {
                        name: "alias".into(),
                        value: Expression::AddressOf {
                            operand: Box::new(Expression::Member {
                                base: Box::new(Expression::Variable("object".into())),
                                offset: 16,
                                member_type: Type::Struct { size: 32, align: 4 },
                                index_stride: None,
                            }),
                        },
                    },
                    Statement::Expression(Expression::Call {
                        name: "consume".into(),
                        arguments: vec![Expression::Member {
                            base: Box::new(Expression::Variable("alias".into())),
                            offset: 8,
                            member_type: Type::Float,
                            index_stride: None,
                        }],
                    }),
                ],
                else_body: Vec::new(),
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

        let folded =
            fold_single_assignment_derived_pointer_alias(&function).expect("dominated alias");
        assert!(folded.locals.is_empty());
        let Statement::If { then_body, .. } = &folded.statements[0] else {
            panic!("if body");
        };
        assert!(matches!(
            then_body.as_slice(),
            [Statement::Expression(Expression::Call { arguments, .. })]
                if matches!(
                    arguments.as_slice(),
                    [Expression::Member { base, offset: 8, .. }]
                        if matches!(base.as_ref(), Expression::AddressOf { .. })
                )
        ));
    }

    #[test]
    fn preserves_an_alias_read_after_the_defining_arm() {
        let mut function = test_function_with_arm_alias();
        function
            .statements
            .push(Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("alias".into())],
            }));
        assert!(fold_single_assignment_derived_pointer_alias(&function).is_none());
    }

    #[test]
    fn preserves_a_derived_address_live_across_a_call() {
        let mut function = test_function_with_arm_alias();
        function.statements = vec![
            Statement::Assign {
                name: "alias".into(),
                value: Expression::MemberAddress {
                    base: Box::new(Expression::Variable("object".into())),
                    offset: 16,
                    element: mwcc_syntax_trees::Pointee::UnsignedInt,
                    index_stride: None,
                },
            },
            Statement::Expression(Expression::Call {
                name: "mutate".into(),
                arguments: Vec::new(),
            }),
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: vec![Expression::Variable("alias".into())],
            }),
        ];

        assert!(fold_single_assignment_derived_pointer_alias(&function).is_none());
    }

    fn test_function_with_arm_alias() -> Function {
        let pointer = Type::StructPointer { element_size: 64 };
        Function {
            return_type: Type::Void,
            name: "caller".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: pointer,
                name: "object".into(),
            }],
            locals: vec![LocalDeclaration {
                declared_type: pointer,
                name: "alias".into(),
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![Statement::If {
                condition: Expression::Variable("object".into()),
                then_body: vec![
                    Statement::Assign {
                        name: "alias".into(),
                        value: Expression::MemberAddress {
                            base: Box::new(Expression::Variable("object".into())),
                            offset: 16,
                            element: mwcc_syntax_trees::Pointee::UnsignedInt,
                            index_stride: None,
                        },
                    },
                    Statement::Expression(Expression::Call {
                        name: "consume".into(),
                        arguments: vec![Expression::Variable("alias".into())],
                    }),
                ],
                else_body: Vec::new(),
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
}
