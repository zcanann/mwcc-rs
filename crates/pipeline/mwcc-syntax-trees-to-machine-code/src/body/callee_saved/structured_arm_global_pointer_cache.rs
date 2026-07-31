//! Shared global-pointer bases within a call-free structured arm prefix.
//!
//! When two member loads read the same nonvolatile global pointer before the
//! arm's first call, MWCC loads the pointer once and keeps that base live
//! through both displacements. The first call ends the cache lifetime.

#[allow(unused_imports)]
use super::*;
use crate::condition_global_cache::ConditionGlobalValue;
use super::structured_condition_join_cache::followup_after_call_free_join;
use super::structured_entry_alias::EntryParameterAlias;
use super::structured_expression_visit::visit_statement;

struct ArmGlobalPointerCachePlan {
    global: String,
    prefix_len: usize,
}

fn repeated_leading_guard_store_constant(statements: &[Statement]) -> Option<i32> {
    let [
        Statement::Store {
            target:
                Expression::Member {
                    index_stride: None,
                    ..
                },
            value: leading_value,
        },
        Statement::If {
            condition,
            then_body,
            else_body,
        },
        ..,
    ] = statements
    else {
        return None;
    };
    let Some(Statement::Store {
        value: guarded_value,
        ..
    }) = then_body.first()
    else {
        return None;
    };
    let leading = i32::try_from(constant_value(leading_value)?).ok()?;
    let guarded = i32::try_from(constant_value(guarded_value)?).ok()?;
    (leading == guarded
        && else_body.is_empty()
        && !crate::analysis::expression_has_side_effect(condition))
    .then_some(leading)
}

fn repeated_constant_scope_len(statements: &[Statement]) -> usize {
    let [
        Statement::If { then_body, .. },
        following,
        ..,
    ] = statements
    else {
        return 1;
    };
    if followup_after_call_free_join(then_body, Some(following)).is_some() {
        2
    } else {
        1
    }
}

fn plan(
    statements: &[Statement],
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<ArmGlobalPointerCachePlan> {
    let prefix_len = statements
        .iter()
        .take_while(|statement| !crate::analysis::statement_has_call(statement))
        .count();
    let mut counts = std::collections::HashMap::<String, usize>::new();
    for statement in &statements[..prefix_len] {
        visit_statement(statement, &mut |expression| {
            let Expression::Member { base, .. } = expression else {
                return;
            };
            let Expression::Variable(name) = base.as_ref() else {
                return;
            };
            if matches!(globals.get(name), Some(Type::StructPointer { .. }))
                && !volatile_globals.contains(name)
            {
                *counts.entry(name.clone()).or_default() += 1;
            }
        });
    }
    let (global, count) = counts
        .into_iter()
        .max_by(|(left_name, left_count), (right_name, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| right_name.cmp(left_name))
        })?;
    (count >= 2).then_some(ArmGlobalPointerCachePlan { global, prefix_len })
}

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_structured_arm_with_global_pointer_cache(
        &mut self,
        statements: &[Statement],
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut std::collections::HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        // A pointer carried across a switch edge has one leading transaction
        // of lifetime in each arm. It may feed a member store or a guarded
        // callback, but must not remain live through the rest of the arm (and
        // especially not across a call). Keeping that boundary explicit lets
        // the allocator use a caller-clobbered work register instead of
        // manufacturing a callee-saved lifetime for the whole switch.
        if self.structured_shared_switch_global_value.is_some()
            && !statements.is_empty()
        {
            let repeated_constant =
                repeated_leading_guard_store_constant(statements);
            let mut previous_constants = repeated_constant.map(|constant| {
                // The comparison-switch base in r4 is dead on entry to any
                // selected arm. The linear interval model cannot see that
                // path boundary and would reject r4 as overlapping later
                // dispatch comparisons, so this arm-local semantic proof owns
                // the fixed work register directly.
                let register = 4;
                self.load_integer_constant(register, i64::from(constant));
                std::mem::replace(
                    &mut self.prematerialized_constants,
                    vec![(constant, register)],
                )
            });
            let (leading, remainder) = statements.split_at(1);
            let leading_result = self.emit_structured_statements(
                leading,
                function,
                ephemeral_locals,
                false,
                return_branches,
                label_positions,
                pending_gotos,
                entry_alias,
            );
            if let Err(diagnostic) = leading_result {
                if let Some(previous) = previous_constants {
                    self.prematerialized_constants = previous;
                }
                return Err(diagnostic);
            }
            let previous_values =
                std::mem::take(&mut self.condition_global_values);
            let previous_shared =
                self.structured_shared_switch_global_value.take();
            let remainder_result = if previous_constants.is_some()
                && !remainder.is_empty()
            {
                let scope_len = repeated_constant_scope_len(remainder);
                let (constant_scope, after_scope) =
                    remainder.split_at(scope_len);
                let scope_result = self.emit_structured_statements(
                    constant_scope,
                    function,
                    ephemeral_locals,
                    false,
                    return_branches,
                    label_positions,
                    pending_gotos,
                    entry_alias,
                );
                self.prematerialized_constants = previous_constants
                    .take()
                    .expect("the repeated constant was materialized");
                scope_result.and_then(|()| {
                    self.emit_structured_statements(
                        after_scope,
                        function,
                        ephemeral_locals,
                        false,
                        return_branches,
                        label_positions,
                        pending_gotos,
                        entry_alias,
                    )
                })
            } else {
                self.emit_structured_statements(
                    remainder,
                    function,
                    ephemeral_locals,
                    false,
                    return_branches,
                    label_positions,
                    pending_gotos,
                    entry_alias,
                )
            };
            self.condition_global_values = previous_values;
            self.structured_shared_switch_global_value = previous_shared;
            if let Some(previous) = previous_constants {
                self.prematerialized_constants = previous;
            }
            return remainder_result;
        }
        let Some(plan) = plan(statements, &self.globals, &self.volatile_globals) else {
            let start = self.output.instructions.len();
            let result = self.emit_structured_statements(
                statements,
                function,
                ephemeral_locals,
                false,
                return_branches,
                label_positions,
                pending_gotos,
                entry_alias,
            );
            if result.is_ok() {
                self.schedule_shared_switch_member_callback_prefix(
                    statements, start,
                );
            }
            return result;
        };
        let (prefix, remainder) = statements.split_at(plan.prefix_len);
        let previous = std::mem::take(&mut self.condition_global_values);
        self.condition_global_values
            .insert(plan.global, ConditionGlobalValue::Pending);
        let prefix_result = self.emit_structured_statements(
            prefix,
            function,
            ephemeral_locals,
            false,
            return_branches,
            label_positions,
            pending_gotos,
            entry_alias,
        );
        self.restore_condition_global_cache(previous);
        prefix_result?;
        self.emit_structured_statements(
            remainder,
            function,
            ephemeral_locals,
            false,
            return_branches,
            label_positions,
            pending_gotos,
            entry_alias,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(offset: u32) -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("state".into())),
            offset,
            member_type: Type::Int,
            index_stride: None,
        }
    }

    #[test]
    fn plans_two_pointer_members_before_the_first_call() {
        let statements = vec![
            Statement::Assign {
                name: "left".into(),
                value: member(40),
            },
            Statement::Assign {
                name: "right".into(),
                value: member(44),
            },
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: Vec::new(),
            }),
        ];
        let globals = std::collections::HashMap::from([(
            "state".into(),
            Type::StructPointer { element_size: 48 },
        )]);
        let plan = plan(&statements, &globals, &std::collections::HashSet::new())
            .expect("the call-free pair should share its pointer base");
        assert_eq!(plan.global, "state");
        assert_eq!(plan.prefix_len, 2);
    }

    #[test]
    fn does_not_join_member_loads_across_a_call() {
        let statements = vec![
            Statement::Assign {
                name: "left".into(),
                value: member(40),
            },
            Statement::Expression(Expression::Call {
                name: "consume".into(),
                arguments: Vec::new(),
            }),
            Statement::Assign {
                name: "right".into(),
                value: member(44),
            },
        ];
        let globals = std::collections::HashMap::from([(
            "state".into(),
            Type::StructPointer { element_size: 48 },
        )]);
        assert!(plan(&statements, &globals, &std::collections::HashSet::new()).is_none());
    }

    #[test]
    fn retains_a_leading_member_constant_through_the_following_guard() {
        let mut statements = vec![
            Statement::Store {
                target: member(0),
                value: Expression::IntegerLiteral(1),
            },
            Statement::If {
                condition: Expression::Variable("same_task".into()),
                then_body: vec![Statement::Store {
                    target: Expression::Variable("yielded".into()),
                    value: Expression::IntegerLiteral(1),
                }],
                else_body: Vec::new(),
            },
        ];

        assert_eq!(
            repeated_leading_guard_store_constant(&statements),
            Some(1)
        );
        let following_guard = Statement::If {
            condition: member(40),
            then_body: Vec::new(),
            else_body: Vec::new(),
        };
        statements.push(following_guard);
        assert_eq!(repeated_constant_scope_len(&statements[1..]), 2);
    }

}
