//! Global pointer values shared by a preceding load and several switch arms.
//!
//! MWCC can keep a nonvolatile pointer global live across a call-free switch
//! guard when several mutually exclusive arms consume the pointer before their
//! first call. The lifetime is planned from syntax, then carried explicitly
//! through switch-edge cache resets by the structured emitter.

#[allow(unused_imports)]
use super::*;
use super::structured_expression_visit::visit_expression;
use mwcc_syntax_trees::ArmBody;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SharedSwitchGlobalValueHome {
    LazyPreferred(u8),
    EagerFixed(u8),
}

pub(super) struct SharedSwitchGlobalValuePlan {
    pub(super) activation_index: usize,
    pub(super) completion_index: usize,
    pub(super) global: String,
    pub(super) home: SharedSwitchGlobalValueHome,
}

pub(super) fn plan(
    statements: &[Statement],
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<SharedSwitchGlobalValuePlan> {
    preceding_member_load_plan(statements, globals, volatile_globals)
        .or_else(|| guarded_scrutinee_rewrite_plan(statements, globals, volatile_globals))
}

fn preceding_member_load_plan(
    statements: &[Statement],
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<SharedSwitchGlobalValuePlan> {
    statements
        .windows(2)
        .enumerate()
        .find_map(|(activation_index, pair)| {
            let [
                Statement::Store {
                    value:
                        Expression::Member {
                            base,
                            index_stride: None,
                            ..
                        },
                    ..
                },
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                },
            ] = pair
            else {
                return None;
            };
            let Expression::Variable(global) = base.as_ref() else {
                return None;
            };
            if !else_body.is_empty()
                || crate::analysis::expression_has_side_effect(condition)
                || volatile_globals.contains(global)
                || !matches!(globals.get(global), Some(Type::StructPointer { .. }))
            {
                return None;
            }
            let Some(Statement::Switch { arms, .. }) = then_body.first() else {
                return None;
            };
            let consuming_arms = arms
                .iter()
                .filter(|arm| arm_starts_with_member_store(&arm.body, global))
                .count();
            let completion_index = activation_index
                + 1
                + usize::from(
                    statements
                        .get(activation_index + 2)
                        .is_some_and(|statement| {
                            guarded_arm_starts_with_member_store(statement, global)
                        }),
                );
            (consuming_arms >= 3).then(|| SharedSwitchGlobalValuePlan {
                activation_index,
                completion_index,
                global: global.clone(),
                home: SharedSwitchGlobalValueHome::LazyPreferred(4),
            })
        })
}

/// Retain a pointer loaded by a call-result guard into the immediately
/// following switch. This is the source shape produced by dispatchers that
/// rewrite one exceptional command before switching on the call result:
///
/// `if (current->flags && command == exceptional) command = replacement;`
/// `switch (command) { ... }`
///
/// The guarded assignment may change only the switch scrutinee. Requiring at
/// least three arms to consume the pointer at their entry keeps this a
/// whole-switch allocation decision rather than ordinary local CSE.
fn guarded_scrutinee_rewrite_plan(
    statements: &[Statement],
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<SharedSwitchGlobalValuePlan> {
    statements
        .windows(2)
        .enumerate()
        .find_map(|(activation_index, pair)| {
            let [
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                },
                Statement::Switch {
                    scrutinee,
                    arms,
                    ..
                },
            ] = pair
            else {
                return None;
            };
            let Expression::Variable(scrutinee_name) = scrutinee else {
                return None;
            };
            if !else_body.is_empty()
                || crate::analysis::expression_has_side_effect(condition)
                || !matches!(
                    then_body.as_slice(),
                    [Statement::Assign { name, value }]
                        if name == scrutinee_name
                            && !crate::analysis::expression_has_side_effect(value)
                )
            {
                return None;
            }
            let global =
                unique_condition_struct_pointer(condition, globals, volatile_globals)?;
            let consuming_arms = arms
                .iter()
                .filter(|arm| arm_starts_with_global_member_use(&arm.body, &global))
                .count();
            (consuming_arms >= 3).then(|| SharedSwitchGlobalValuePlan {
                activation_index,
                completion_index: activation_index + 1,
                global,
                home: SharedSwitchGlobalValueHome::EagerFixed(5),
            })
        })
}

fn unique_condition_struct_pointer(
    condition: &Expression,
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<String> {
    let mut candidates = std::collections::BTreeSet::new();
    visit_expression(condition, &mut |expression| {
        let Expression::Member { base, .. } = expression else {
            return;
        };
        let Expression::Variable(name) = base.as_ref() else {
            return;
        };
        if matches!(globals.get(name), Some(Type::StructPointer { .. }))
            && !volatile_globals.contains(name)
        {
            candidates.insert(name.clone());
        }
    });
    let mut candidates = candidates.into_iter();
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

fn arm_starts_with_global_member_use(body: &ArmBody, global: &str) -> bool {
    let ArmBody::Statements(statements) = body else {
        return false;
    };
    let Some(first) = statements.first() else {
        return false;
    };
    let expression = match first {
        Statement::Store { target, .. } => target,
        Statement::Assign { value, .. } | Statement::Expression(value) => value,
        Statement::If { condition, .. } => condition,
        _ => return false,
    };
    if crate::analysis::expression_has_side_effect(expression) {
        return false;
    }
    let mut uses_global = false;
    visit_expression(expression, &mut |expression| {
        uses_global |= matches!(
            expression,
            Expression::Member { base, .. }
                if matches!(base.as_ref(), Expression::Variable(name) if name == global)
        );
    });
    uses_global
}

fn guarded_arm_starts_with_member_store(statement: &Statement, global: &str) -> bool {
    let Statement::If {
        condition,
        then_body,
        else_body,
    } = statement
    else {
        return false;
    };
    !crate::analysis::expression_has_side_effect(condition)
        && (statements_start_with_member_store(then_body, global)
            || statements_start_with_member_store(else_body, global))
}

fn statements_start_with_member_store(statements: &[Statement], global: &str) -> bool {
    matches!(
        statements.first(),
        Some(Statement::Store {
            target: Expression::Member {
                base,
                index_stride: None,
                ..
            },
            ..
        }) if matches!(base.as_ref(), Expression::Variable(name) if name == global)
    )
}

fn arm_starts_with_member_store(body: &ArmBody, global: &str) -> bool {
    let ArmBody::Statements(statements) = body else {
        return false;
    };
    statements_start_with_member_store(statements, global)
}

impl Generator {
    /// Fill the retained-base store's issue slot with the independent high
    /// half of the following callback address. This schedule is scoped to the
    /// switch lifetime above; ordinary member stores retain source order.
    pub(super) fn schedule_shared_switch_member_callback_prefix(
        &mut self,
        statements: &[Statement],
        start: usize,
    ) {
        let Some((global, retained)) = self.structured_shared_switch_global_value.as_ref() else {
            return;
        };
        let [
            Statement::Store {
                target:
                    Expression::Member {
                        base,
                        index_stride: None,
                        ..
                    },
                value: Expression::IntegerLiteral(_),
            },
            Statement::Expression(Expression::Call {
                name: callee,
                arguments,
            }),
        ] = statements
        else {
            return;
        };
        let Expression::Variable(base_name) = base.as_ref() else {
            return;
        };
        if base_name != global || callback_name(arguments).is_none() {
            return;
        }
        let Some(window) = self.output.instructions.get(start..start + 5) else {
            return;
        };
        if !matches!(
            window,
            [
                Instruction::AddImmediate {
                    d: stored,
                    a: 0,
                    ..
                },
                Instruction::StoreWord {
                    s: stored_again,
                    a: store_base,
                    ..
                },
                Instruction::AddImmediateShifted {
                    d: argument_high,
                    a: 0,
                    ..
                },
                Instruction::AddImmediate {
                    d: argument_low,
                    a: low_base,
                    ..
                },
                Instruction::BranchAndLink { target },
            ] if stored == stored_again
                && store_base == retained
                && argument_high == argument_low
                && argument_high == low_base
                && target == callee
        ) || !super::super::schedule_relocations::same_target_value(
            &self.output.relocations,
            &self.output.constants,
            start + 2,
            start + 3,
        ) {
            return;
        }
        super::structured_conversion_call_schedule::permute_region(
            &mut self.output,
            start,
            &[0, 2, 1, 3, 4],
        );
    }
}

fn callback_name(arguments: &[Expression]) -> Option<&str> {
    let [argument] = arguments else {
        return None;
    };
    match argument {
        Expression::Variable(name) => Some(name),
        Expression::AddressOf { operand } => {
            let Expression::Variable(name) = operand.as_ref() else {
                return None;
            };
            Some(name)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::SwitchArm;

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("executing".into())),
            offset,
            member_type: Type::Int,
            index_stride: None,
        }
    }

    fn arm(value: i64) -> SwitchArm {
        SwitchArm {
            value,
            body: ArmBody::Statements(vec![Statement::Store {
                target: member(12),
                value: Expression::IntegerLiteral(value),
            }]),
            falls_through: false,
        }
    }

    #[test]
    fn carries_a_preceding_member_base_into_three_switch_arms() {
        let statements = vec![
            Statement::Store {
                target: Expression::Variable("command".into()),
                value: member(8),
            },
            Statement::If {
                condition: Expression::Variable("resume".into()),
                then_body: vec![Statement::Switch {
                    scrutinee: Expression::Variable("resume".into()),
                    arms: vec![arm(1), arm(2), arm(3)],
                    default: None,
                }],
                else_body: Vec::new(),
            },
        ];
        let globals = std::collections::HashMap::from([(
            "executing".into(),
            Type::StructPointer { element_size: 48 },
        )]);

        let plan = plan(&statements, &globals, &std::collections::HashSet::new())
            .expect("the pointer should span the guarded switch");

        assert_eq!(plan.activation_index, 0);
        assert_eq!(plan.completion_index, 1);
        assert_eq!(plan.global, "executing");
        assert_eq!(
            plan.home,
            SharedSwitchGlobalValueHome::LazyPreferred(4),
        );
    }

    #[test]
    fn extends_the_false_edge_value_into_the_following_guard() {
        let mut statements = vec![
            Statement::Store {
                target: Expression::Variable("command".into()),
                value: member(8),
            },
            Statement::If {
                condition: Expression::Variable("resume".into()),
                then_body: vec![Statement::Switch {
                    scrutinee: Expression::Variable("resume".into()),
                    arms: vec![arm(1), arm(2), arm(3)],
                    default: None,
                }],
                else_body: Vec::new(),
            },
        ];
        statements.push(Statement::If {
            condition: Expression::Variable("motor".into()),
            then_body: vec![Statement::Store {
                target: member(12),
                value: Expression::IntegerLiteral(1),
            }],
            else_body: Vec::new(),
        });
        let globals = std::collections::HashMap::from([(
            "executing".into(),
            Type::StructPointer { element_size: 48 },
        )]);

        let plan = plan(&statements, &globals, &std::collections::HashSet::new())
            .expect("the false-edge pointer should reach the following guard");

        assert_eq!(plan.completion_index, 2);
    }

    #[test]
    fn carries_a_guard_pointer_into_a_following_command_switch() {
        let guard = Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                left: Box::new(member(8)),
                right: Box::new(Expression::Binary {
                    operator: BinaryOperator::Equal,
                    left: Box::new(Expression::Variable("command".into())),
                    right: Box::new(Expression::IntegerLiteral(2)),
                }),
            },
            then_body: vec![Statement::Assign {
                name: "command".into(),
                value: Expression::IntegerLiteral(3),
            }],
            else_body: Vec::new(),
        };
        let callback_guard = |offset| Statement::If {
            condition: member(offset),
            then_body: Vec::new(),
            else_body: Vec::new(),
        };
        let switch = Statement::Switch {
            scrutinee: Expression::Variable("command".into()),
            arms: vec![
                arm(1),
                arm(2),
                SwitchArm {
                    value: 3,
                    body: ArmBody::Statements(vec![callback_guard(48)]),
                    falls_through: false,
                },
            ],
            default: None,
        };
        let globals = std::collections::HashMap::from([(
            "executing".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        let plan = plan(
            &[guard, switch],
            &globals,
            &std::collections::HashSet::new(),
        )
        .expect("the guarded pointer should span the following switch");

        assert_eq!(plan.activation_index, 0);
        assert_eq!(plan.completion_index, 1);
        assert_eq!(plan.global, "executing");
        assert_eq!(plan.home, SharedSwitchGlobalValueHome::EagerFixed(5));
    }
}
