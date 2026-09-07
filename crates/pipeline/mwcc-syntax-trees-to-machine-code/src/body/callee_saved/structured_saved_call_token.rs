//! Saved roles for an entry call result retained until a terminal release call.
use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
use super::*;
use mwcc_syntax_trees::{LocalDeclaration, Parameter};

mod schedule;

pub(super) struct SavedCallTokenLayout;

impl SavedCallTokenLayout {
    pub(super) fn plan(
        function: &Function,
        eager: &[&LocalDeclaration],
        parameters: &[&Parameter],
        deferred: &[&LocalDeclaration],
        homes: &DeferredSavedHomePlan,
        reuse: &StructuredParameterHomeReuse,
        count: usize,
    ) -> Option<Self> {
        let [token] = deferred else { return None };
        if !eager.is_empty()
            || parameters.len() != 2
            || function.parameters.len() != 2
            || count != 3
            || homes.group_count != 1
            || reuse.fresh_group_count != 1
            || reuse.home_index(homes.group(&token.name)) != 2
            || retained_token(function)? != token.name
        {
            return None;
        }
        Some(Self)
    }

    pub(super) fn preference(&self, home: usize) -> Option<u8> {
        // Parameters arrive in reverse source order, followed by the deferred token.
        [30, 29, 31].get(home).copied()
    }

    pub(super) fn save_order(&self) -> [usize; 3] {
        [2, 0, 1]
    }

    pub(super) fn frame_slot(&self, home: usize) -> usize {
        [1, 2, 0][home]
    }
}

pub(super) fn retained_token(function: &Function) -> Option<&str> {
    let [Statement::Assign {
        name: token,
        value: Expression::Call { arguments, .. },
    }, middle @ .., Statement::Expression(Expression::Call {
        arguments: release, ..
    })] = function.statements.as_slice()
    else {
        return None;
    };
    if !arguments.is_empty()
        || middle.is_empty()
        || !matches!(release.as_slice(), [Expression::Variable(value)] if value == token)
        || !matches!(
            function.return_expression,
            None | Some(Expression::IntegerLiteral(_))
        )
        || !function.guards.is_empty()
    {
        return None;
    }
    let local = function.locals.iter().find(|local| local.name == *token)?;
    if local.initializer.is_some()
        || local.is_volatile
        || local.is_static
        || local.array_length.is_some()
        || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
        || middle
            .iter()
            .any(|statement| mentions_token_or_exits(statement, token))
    {
        return None;
    }
    Some(token)
}

fn mentions_token_or_exits(statement: &Statement, token: &str) -> bool {
    if super::structured_liveness::statement_reads_name(statement, token) {
        return true;
    }
    match statement {
        Statement::Assign { name, .. } => name == token,
        Statement::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body)
            .any(|statement| mentions_token_or_exits(statement, token)),
        Statement::Expression(_) | Statement::Store { .. } => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: name.into(),
            arguments,
        })
    }
    fn var(name: &str) -> Expression {
        Expression::Variable(name.into())
    }
    fn fixture() -> Function {
        Function {
            return_type: Type::Void,
            name: "transaction".into(),
            is_static: false,
            is_weak: false,
            parameters: ["output", "value"]
                .into_iter()
                .map(|name| Parameter {
                    name: name.into(),
                    parameter_type: Type::Int,
                })
                .collect(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Int,
                name: "cookie".into(),
                initializer: None,
                is_volatile: false,
                is_static: false,
                is_const: false,
                array_length: None,
                data_bytes: None,
                data_relocations: vec![],
                attribute_alignment: None,
                row_bytes: None,
            }],
            statements: vec![
                Statement::Assign {
                    name: "cookie".into(),
                    value: Expression::Call {
                        name: "acquire".into(),
                        arguments: vec![],
                    },
                },
                call("work", vec![var("output"), var("value")]),
                call("release", vec![var("cookie")]),
            ],
            guards: vec![],
            return_expression: None,
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: vec![],
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        }
    }

    #[test]
    fn token_roles_own_one_consistent_home_and_frame_order() {
        let f = fixture();
        let deferred = [&f.locals[0]];
        let homes =
            super::super::structured_locals::plan_deferred_saved_homes(&f, &deferred).unwrap();
        let reuse = StructuredParameterHomeReuse::retain_distinct(0, 2, 1);
        let plan = SavedCallTokenLayout::plan(
            &f,
            &[],
            &[&f.parameters[1], &f.parameters[0]],
            &deferred,
            &homes,
            &reuse,
            3,
        )
        .unwrap();
        assert_eq!(retained_token(&f), Some("cookie"));
        assert_eq!(
            plan.save_order().map(|home| plan.preference(home).unwrap()),
            [31, 30, 29]
        );
        for (slot, home) in plan.save_order().into_iter().enumerate() {
            assert_eq!(plan.frame_slot(home), slot);
        }
    }

    #[test]
    fn token_must_remain_immutable_and_unobserved_until_release() {
        for replacement in [
            call("observe", vec![var("cookie")]),
            call(
                "escape",
                vec![Expression::AddressOf {
                    operand: Box::new(var("cookie")),
                }],
            ),
            Statement::Assign {
                name: "cookie".into(),
                value: Expression::IntegerLiteral(0),
            },
            Statement::Expression(Expression::Assign {
                target: Box::new(var("cookie")),
                value: Box::new(Expression::IntegerLiteral(0)),
            }),
            Statement::If {
                condition: var("value"),
                then_body: vec![Statement::Assign {
                    name: "cookie".into(),
                    value: Expression::IntegerLiteral(0),
                }],
                else_body: vec![],
            },
            Statement::Return(None),
            Statement::Goto("exit".into()),
        ] {
            let mut f = fixture();
            f.statements[1] = replacement;
            assert!(retained_token(&f).is_none());
        }
    }

    #[test]
    fn token_requires_an_entry_call_and_a_single_terminal_argument() {
        for change in 0..5 {
            let mut f = fixture();
            match change {
                0 => f.locals[0].is_volatile = true,
                1 => f.locals[0].declared_type = Type::Double,
                2 => f.return_expression = Some(var("cookie")),
                3 => f.statements[2] = call("release", vec![var("cookie"), var("value")]),
                _ => {
                    f.statements[0] = Statement::Assign {
                        name: "cookie".into(),
                        value: Expression::Call {
                            name: "acquire".into(),
                            arguments: vec![var("value")],
                        },
                    }
                }
            }
            assert!(retained_token(&f).is_none());
        }
    }
}
