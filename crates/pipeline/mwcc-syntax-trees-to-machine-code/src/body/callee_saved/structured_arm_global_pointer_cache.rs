//! Shared global-pointer bases within a call-free structured arm prefix.
//!
//! When two member loads read the same nonvolatile global pointer before the
//! arm's first call, MWCC loads the pointer once and keeps that base live
//! through both displacements. The first call ends the cache lifetime.

#[allow(unused_imports)]
use super::*;
use crate::condition_global_cache::ConditionGlobalValue;
use super::structured_entry_alias::EntryParameterAlias;
use super::structured_expression_visit::visit_statement;

struct ArmGlobalPointerCachePlan {
    global: String,
    prefix_len: usize,
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
        let Some(plan) = plan(statements, &self.globals, &self.volatile_globals) else {
            return self.emit_structured_statements(
                statements,
                function,
                ephemeral_locals,
                false,
                return_branches,
                label_positions,
                pending_gotos,
                entry_alias,
            );
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
}
