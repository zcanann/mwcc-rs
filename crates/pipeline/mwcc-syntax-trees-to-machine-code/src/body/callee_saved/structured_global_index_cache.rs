//! Call-surviving scaled indices for repeated global struct-array accesses.
//!
//! This is a source-level value plan, not an instruction peephole. A stable
//! parameter subscript used repeatedly across calls owns one scaled-index live
//! range; expression lowering still owns each global base and displacement.

use mwcc_syntax_trees::{ArmBody, Expression, Function, Statement, Type};

use super::structured_expression_visit::visit_statement;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredGlobalIndexPlan {
    pub(super) global: String,
    pub(super) index: String,
    pub(super) stride: u32,
    pub(super) retain_element: bool,
}

pub(super) fn plan(
    function: &Function,
    globals: &std::collections::HashMap<String, Type>,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<StructuredGlobalIndexPlan> {
    let parameters: std::collections::HashSet<&str> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect();
    let mut occurrences =
        std::collections::HashMap::<(String, String, u32), Vec<u32>>::new();
    for statement in &function.statements {
        visit_statement(statement, &mut |expression| {
            let Expression::MemberAddress { base, offset, .. } = expression else {
                return;
            };
            let Expression::Index { base, index } = base.as_ref() else {
                return;
            };
            let (Expression::Variable(global), Expression::Variable(index)) =
                (base.as_ref(), index.as_ref())
            else {
                return;
            };
            if !parameters.contains(index.as_str())
                || !global_array_sizes.contains_key(global.as_str())
            {
                return;
            }
            let Some(Type::Struct { size, .. }) = globals.get(global).copied() else {
                return;
            };
            if size != 0 {
                occurrences
                    .entry((global.to_owned(), index.to_owned(), u32::from(size)))
                    .or_default()
                    .push(*offset);
            }
        });
    }
    let ((global, index, stride), offsets) =
        occurrences.into_iter().max_by_key(|(_, offsets)| offsets.len())?;
    if offsets.len() < 3 || statements_assign_name(&function.statements, &index) {
        return None;
    }
    let retain_element =
        offsets.first().is_some_and(|offset| *offset != 0)
            && offsets.last() == Some(&0);
    Some(StructuredGlobalIndexPlan {
        global,
        index,
        stride,
        retain_element,
    })
}

fn statements_assign_name(statements: &[Statement], name: &str) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign { name: target, .. } => target == name,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statements_assign_name(then_body, name)
                || statements_assign_name(else_body, name)
        }
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| arm_body_assigns_name(&arm.body, name))
                || default
                    .as_ref()
                    .is_some_and(|body| arm_body_assigns_name(body, name))
        }
        Statement::Loop { body, .. } => statements_assign_name(body, name),
        _ => false,
    })
}

fn arm_body_assigns_name(body: &ArmBody, name: &str) -> bool {
    match body {
        ArmBody::Return(_) => false,
        ArmBody::Statements(statements) => statements_assign_name(statements, name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{Function, Parameter};

    fn member(offset: u32) -> Expression {
        Expression::MemberAddress {
            base: Box::new(Expression::Index {
                base: Box::new(Expression::Variable("records".into())),
                index: Box::new(Expression::Variable("i".into())),
            }),
            offset,
            element: mwcc_syntax_trees::Pointee::UnsignedChar,
            index_stride: None,
        }
    }

    fn function(statements: Vec<Statement>) -> Function {
        Function {
            return_type: Type::Void,
            name: "f".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![Parameter {
                parameter_type: Type::Int,
                name: "i".into(),
            }],
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

    fn maps() -> (
        std::collections::HashMap<String, Type>,
        std::collections::HashMap<String, u32>,
    ) {
        (
            std::collections::HashMap::from([(
                "records".into(),
                Type::Struct {
                    size: 108,
                    align: 4,
                },
            )]),
            std::collections::HashMap::from([("records".into(), 864)]),
        )
    }

    #[test]
    fn plans_a_repeated_stable_parameter_subscript() {
        let function = function(vec![Statement::Expression(Expression::Call {
            name: "sink".into(),
            arguments: vec![member(0), member(4), member(8)],
        })]);
        let (globals, sizes) = maps();
        assert_eq!(
            plan(&function, &globals, &sizes),
            Some(StructuredGlobalIndexPlan {
                global: "records".into(),
                index: "i".into(),
                stride: 108,
                retain_element: false,
            })
        );
    }

    #[test]
    fn rejects_an_index_reassigned_in_the_body() {
        let function = function(vec![
            Statement::Expression(Expression::Call {
                name: "sink".into(),
                arguments: vec![member(0), member(4), member(8)],
            }),
            Statement::Assign {
                name: "i".into(),
                value: Expression::IntegerLiteral(0),
            },
        ]);
        let (globals, sizes) = maps();
        assert_eq!(plan(&function, &globals, &sizes), None);
    }

    #[test]
    fn retains_an_element_used_first_by_member_and_last_by_base() {
        let function = function(vec![Statement::Expression(Expression::Call {
            name: "sink".into(),
            arguments: vec![member(64), member(64), member(0)],
        })]);
        let (globals, sizes) = maps();
        assert!(
            plan(&function, &globals, &sizes)
                .is_some_and(|plan| plan.retain_element)
        );
    }
}
