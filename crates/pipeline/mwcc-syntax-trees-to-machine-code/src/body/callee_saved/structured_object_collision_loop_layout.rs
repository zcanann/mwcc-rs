//! Saved-GPR roles for pairwise object collision loops.
//!
//! This source shape keeps two incoming objects, an owner/member anchor, a
//! list cursor, a sticky branch flag, a member-derived receiver, a peer object,
//! and two scalar member indices live across calls. Legacy MWCC assigns those
//! semantic roles stable homes rather than coloring them in definition order.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) struct StructuredObjectCollisionLoopLayout {
    preference_by_home: [u8; 9],
}

impl StructuredObjectCollisionLoopLayout {
    pub(super) fn plan(
        function: &Function,
        eager_locals: &[&LocalDeclaration],
        saved_parameters: &[&Parameter],
        deferred_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
        parameter_reuse: &StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        let [_, _] = saved_parameters else {
            return None;
        };
        if !eager_locals.is_empty()
            || deferred_locals.len() != 7
            || deferred.group_count != 7
            || parameter_reuse.fresh_group_count != 7
            || home_count != 9
        {
            return None;
        }
        let loop_statements: Vec<_> = function
            .statements
            .iter()
            .filter(|statement| matches!(statement, Statement::Loop { .. }))
            .collect();
        let [loop_statement] = loop_statements.as_slice() else {
            return None;
        };
        let Statement::Loop {
            initializer:
                Some(Expression::Assign {
                    target: initializer_target,
                    ..
                }),
            condition: Some(condition),
            step:
                Some(Expression::Assign {
                    target: step_target,
                    value: step_value,
                }),
            body,
            ..
        } = loop_statement
        else {
            return None;
        };
        let owners: Vec<_> = function
            .statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assign {
                    name,
                    value: Expression::Member { base, .. },
                } => {
                    let Expression::Variable(candidate) = base.as_ref() else {
                        return None;
                    };
                    saved_parameters
                        .iter()
                        .position(|parameter| parameter.name == *candidate)
                        .map(|parameter| (name, parameter))
                }
                _ => None,
            })
            .collect();
        let [(owner, owner_parameter)] = owners.as_slice() else {
            return None;
        };
        let flags: Vec<_> = function
            .statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assign {
                    name,
                    value: Expression::IntegerLiteral(0),
                } if statements_assign_integer(body, name, 1) => Some(name),
                _ => None,
            })
            .collect();
        let [flag] = flags.as_slice() else {
            return None;
        };
        let Expression::Variable(cursor) = initializer_target.as_ref() else {
            return None;
        };
        if !matches!(
            (step_target.as_ref(), step_value.as_ref()),
            (
                Expression::Variable(target),
                Expression::Member { base, .. },
            ) if target == cursor
                && matches!(base.as_ref(), Expression::Variable(name) if name == cursor)
        ) || !expression_reads_name(condition, cursor)
            || !statements_assign_integer(body, flag, 1)
        {
            return None;
        }

        let receiver = unique_derived_local(body, owner, DerivedValue::AddressedMember)?;
        let peer = unique_derived_local(body, cursor, DerivedValue::Member)?;
        let owner_index = unique_derived_local(body, owner, DerivedValue::Member)?;
        let peer_index = unique_derived_local(body, &peer, DerivedValue::Member)?;
        let role_names = [
            owner.as_str(),
            flag.as_str(),
            cursor.as_str(),
            receiver.as_str(),
            peer.as_str(),
            owner_index.as_str(),
            peer_index.as_str(),
        ];
        if role_names
            .iter()
            .any(|name| !deferred_locals.iter().any(|local| local.name == *name))
        {
            return None;
        }
        let mut unique_roles = role_names.to_vec();
        unique_roles.sort_unstable();
        unique_roles.dedup();
        if unique_roles.len() != role_names.len() {
            return None;
        }

        let mut preference_by_home = [0; 9];
        let mut occupied = [false; 9];
        let mut set = |home: usize, preference: u8| {
            if home >= preference_by_home.len() || occupied[home] {
                return false;
            }
            occupied[home] = true;
            preference_by_home[home] = preference;
            true
        };
        let home = |name: &str| {
            deferred
                .group_if_present(name)
                .map(|group| parameter_reuse.home_index(group))
        };
        if !set(*owner_parameter, 26)
            || !set(1 - *owner_parameter, 27)
            || !set(home(owner)?, 30)
            || !set(home(flag)?, 28)
            || !set(home(cursor)?, 29)
            || !set(home(&receiver)?, 31)
            || !set(home(&peer)?, 24)
            || !set(home(&owner_index)?, 23)
            || !set(home(&peer_index)?, 25)
            || occupied.iter().any(|occupied| !occupied)
        {
            return None;
        }
        Some(Self { preference_by_home })
    }

    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        self.preference_by_home.get(home_index).copied()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DerivedValue {
    Member,
    AddressedMember,
}

fn unique_derived_local(
    statements: &[Statement],
    base_name: &str,
    kind: DerivedValue,
) -> Option<String> {
    let mut names = Vec::new();
    collect_derived_locals(statements, base_name, kind, &mut names);
    names.sort_unstable();
    names.dedup();
    let [name] = names.as_slice() else {
        return None;
    };
    Some(name.clone())
}

fn collect_derived_locals(
    statements: &[Statement],
    base_name: &str,
    kind: DerivedValue,
    names: &mut Vec<String>,
) {
    for statement in statements {
        match statement {
            Statement::Assign { name, value } if derived_from_member(value, base_name, kind) => {
                names.push(name.clone());
            }
            Statement::If {
                then_body,
                else_body,
                ..
            } => {
                collect_derived_locals(then_body, base_name, kind, names);
                collect_derived_locals(else_body, base_name, kind, names);
            }
            Statement::Loop { body, .. } => {
                collect_derived_locals(body, base_name, kind, names);
            }
            Statement::Switch { arms, default, .. } => {
                for body in arms.iter().map(|arm| &arm.body).chain(default.iter()) {
                    if let mwcc_syntax_trees::ArmBody::Statements(statements) = body {
                        collect_derived_locals(statements, base_name, kind, names);
                    }
                }
            }
            _ => {}
        }
    }
}

fn derived_from_member(expression: &Expression, base_name: &str, kind: DerivedValue) -> bool {
    let member = match (kind, expression) {
        (DerivedValue::Member, Expression::Member { base, .. }) => base,
        (DerivedValue::AddressedMember, Expression::AddressOf { operand }) => {
            let Expression::Member { base, .. } = operand.as_ref() else {
                return false;
            };
            base
        }
        _ => return false,
    };
    matches!(member.as_ref(), Expression::Variable(name) if name == base_name)
}

fn statements_assign_integer(statements: &[Statement], target: &str, expected: i64) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Assign {
            name,
            value: Expression::IntegerLiteral(value),
        } => name == target && *value == expected,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            statements_assign_integer(then_body, target, expected)
                || statements_assign_integer(else_body, target, expected)
        }
        Statement::Loop { body, .. } => statements_assign_integer(body, target, expected),
        Statement::Switch { arms, default, .. } => {
            arms.iter().any(|arm| {
                matches!(
                    &arm.body,
                    mwcc_syntax_trees::ArmBody::Statements(statements)
                        if statements_assign_integer(statements, target, expected)
                )
            }) || default.as_ref().is_some_and(|body| {
                matches!(
                    body,
                    mwcc_syntax_trees::ArmBody::Statements(statements)
                        if statements_assign_integer(statements, target, expected)
                )
            })
        }
        _ => false,
    })
}

fn expression_reads_name(expression: &Expression, expected: &str) -> bool {
    let mut reads = false;
    super::structured_expression_visit::visit_expression(expression, &mut |expression| {
        reads |= matches!(expression, Expression::Variable(name) if name == expected);
    });
    reads
}

#[cfg(test)]
mod tests {
    use super::super::structured_eager_home_reuse::StructuredEagerHomeReuse;
    use super::super::structured_locals::plan_deferred_saved_homes;
    use super::*;

    fn local(name: &str, declared_type: Type) -> LocalDeclaration {
        LocalDeclaration {
            declared_type,
            name: name.into(),
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            row_bytes: None,
        }
    }

    fn member(base: &str, offset: u32, member_type: Type) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable(base.into())),
            offset,
            member_type,
            index_stride: None,
        }
    }

    #[test]
    fn assigns_collision_roles_independently_of_parameter_order() {
        let function = Function {
            return_type: Type::Void,
            name: "collide".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Int),
                    name: "owner_object".into(),
                },
                Parameter {
                    parameter_type: Type::Pointer(Pointee::Int),
                    name: "motion".into(),
                },
            ],
            locals: vec![
                local("owner", Type::Pointer(Pointee::Int)),
                local("flag", Type::UnsignedChar),
                local("cursor", Type::Pointer(Pointee::Int)),
                local("receiver", Type::Pointer(Pointee::Int)),
                local("peer", Type::Pointer(Pointee::Int)),
                local("owner_index", Type::Int),
                local("peer_index", Type::Int),
            ],
            statements: vec![
                Statement::Assign {
                    name: "owner".into(),
                    value: member("owner_object", 44, Type::Pointer(Pointee::Int)),
                },
                Statement::Assign {
                    name: "flag".into(),
                    value: Expression::IntegerLiteral(0),
                },
                Statement::Loop {
                    kind: LoopKind::For,
                    initializer: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("cursor".into())),
                        value: Box::new(member("objects", 32, Type::Pointer(Pointee::Int))),
                    }),
                    condition: Some(Expression::Binary {
                        operator: BinaryOperator::NotEqual,
                        left: Box::new(Expression::Variable("cursor".into())),
                        right: Box::new(Expression::IntegerLiteral(0)),
                    }),
                    step: Some(Expression::Assign {
                        target: Box::new(Expression::Variable("cursor".into())),
                        value: Box::new(member("cursor", 8, Type::Pointer(Pointee::Int))),
                    }),
                    body: vec![
                        Statement::Assign {
                            name: "receiver".into(),
                            value: Expression::AddressOf {
                                operand: Box::new(member("owner", 708, Type::Int)),
                            },
                        },
                        Statement::Assign {
                            name: "peer".into(),
                            value: member("cursor", 44, Type::Pointer(Pointee::Int)),
                        },
                        Statement::Assign {
                            name: "owner_index".into(),
                            value: member("owner", 2108, Type::Int),
                        },
                        Statement::Assign {
                            name: "peer_index".into(),
                            value: member("peer", 2108, Type::Int),
                        },
                        Statement::Expression(Expression::Call {
                            name: "consume".into(),
                            arguments: vec![
                                Expression::Variable("owner_object".into()),
                                Expression::Variable("motion".into()),
                                Expression::Variable("receiver".into()),
                                Expression::Variable("owner_index".into()),
                                Expression::Variable("peer_index".into()),
                            ],
                        }),
                        Statement::Assign {
                            name: "flag".into(),
                            value: Expression::IntegerLiteral(1),
                        },
                    ],
                },
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
        let deferred_locals: Vec<_> = function.locals.iter().collect();
        let deferred = plan_deferred_saved_homes(&function, &deferred_locals).unwrap();
        let saved_parameters = vec![&function.parameters[1], &function.parameters[0]];
        let eager = StructuredEagerHomeReuse::plan(&function, &[], &deferred);
        let parameter_reuse =
            StructuredParameterHomeReuse::plan(&function, 0, &saved_parameters, &deferred, &eager);

        let layout = StructuredObjectCollisionLoopLayout::plan(
            &function,
            &[],
            &saved_parameters,
            &deferred_locals,
            &deferred,
            &parameter_reuse,
            9,
        )
        .unwrap();
        let preference =
            |name: &str| layout.preference(parameter_reuse.home_index(deferred.group(name)));

        assert_eq!(layout.preference(0), Some(27));
        assert_eq!(layout.preference(1), Some(26));
        assert_eq!(preference("owner"), Some(30));
        assert_eq!(preference("flag"), Some(28));
        assert_eq!(preference("cursor"), Some(29));
        assert_eq!(preference("receiver"), Some(31));
        assert_eq!(preference("peer"), Some(24));
        assert_eq!(preference("owner_index"), Some(23));
        assert_eq!(preference("peer_index"), Some(25));
    }
}
