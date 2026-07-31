//! Condition values consumed by the first call in either `if` arm.
//!
//! A pointer and one of its members can dominate both mutually exclusive arm
//! entries even though either call clobbers their physical argument registers.
//! The cache lifetime therefore ends at each arm's first statement.

use super::structured_entry_alias::EntryParameterAlias;
#[allow(unused_imports)]
use super::*;
use crate::condition_global_cache::ConditionGlobalValue;
use crate::condition_member_cache::ConditionMemberCache;
use std::collections::HashMap;

pub(super) struct BranchEntryCachePlan {
    pub(super) global: Option<String>,
    pub(super) member: Expression,
}

pub(super) fn plan(
    condition: &Expression,
    then_body: &[Statement],
    else_body: &[Statement],
    globals: &std::collections::HashMap<String, Type>,
    volatile_globals: &std::collections::HashSet<String>,
) -> Option<BranchEntryCachePlan> {
    let member = compared_member(condition)?;
    let Expression::Member { base, .. } = member else {
        unreachable!("compared_member returns a member");
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    if volatile_globals.contains(global)
        || !matches!(globals.get(global), Some(Type::StructPointer { .. }))
    {
        return None;
    }
    let then_arguments = direct_call_arguments(then_body.first()?)?;
    let else_arguments = direct_call_arguments(else_body.first()?)?;
    let global = if matches!(
        then_arguments.first(),
        Some(Expression::Variable(name)) if name == global
    ) && matches!(
        else_arguments,
        [Expression::Variable(name), value]
            if name == global && structurally_equal(value, member)
    ) {
        Some(global.clone())
    } else if matches!(
        then_arguments,
        [first, _] if constant_value(first) == Some(0)
    ) && matches!(
        else_arguments,
        [first, value]
            if constant_value(first) == Some(0) && structurally_equal(value, member)
    ) {
        None
    } else {
        return None;
    };
    Some(BranchEntryCachePlan {
        global,
        member: member.clone(),
    })
}

fn compared_member(expression: &Expression) -> Option<&Expression> {
    match expression {
        Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left,
            right,
        } => compared_member(left).or_else(|| compared_member(right)),
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } if constant_value(right) == Some(0)
            && matches!(left.as_ref(), Expression::Member { .. }) =>
        {
            Some(left)
        }
        Expression::Binary {
            operator: BinaryOperator::Equal | BinaryOperator::NotEqual,
            left,
            right,
        } if constant_value(left) == Some(0)
            && matches!(right.as_ref(), Expression::Member { .. }) =>
        {
            Some(right)
        }
        _ => None,
    }
}

fn direct_call_arguments(statement: &Statement) -> Option<&[Expression]> {
    let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
        return None;
    };
    Some(arguments)
}

impl Generator {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_branch_entry_cached_arm(
        &mut self,
        statements: &[Statement],
        global_cache: Option<&HashMap<String, ConditionGlobalValue>>,
        member_cache: Option<&ConditionMemberCache>,
        function: &Function,
        ephemeral_locals: &[&LocalDeclaration],
        return_branches: &mut Vec<usize>,
        label_positions: &mut HashMap<String, usize>,
        pending_gotos: &mut Vec<(usize, String)>,
        entry_alias: &mut Option<EntryParameterAlias>,
    ) -> Compilation<()> {
        let (Some(member_cache), Some((first, remainder))) =
            (member_cache, statements.split_first())
        else {
            return self.emit_structured_arm_with_global_pointer_cache(
                statements,
                function,
                ephemeral_locals,
                return_branches,
                label_positions,
                pending_gotos,
                entry_alias,
            );
        };
        let previous_globals = global_cache
            .map(|cache| std::mem::replace(&mut self.condition_global_values, cache.clone()));
        let edge_member_cache = self.condition_member_cache_rebased(member_cache);
        let previous_members =
            std::mem::replace(&mut self.condition_member_cache, edge_member_cache);
        let prefix_result = self.emit_structured_arm_with_global_pointer_cache(
            std::slice::from_ref(first),
            function,
            ephemeral_locals,
            return_branches,
            label_positions,
            pending_gotos,
            entry_alias,
        );
        if let Some(previous) = previous_globals {
            self.restore_condition_global_cache(previous);
        }
        self.restore_condition_member_cache(previous_members);
        prefix_result?;
        self.emit_structured_arm_with_global_pointer_cache(
            remainder,
            function,
            ephemeral_locals,
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

    fn member() -> Expression {
        Expression::Member {
            base: Box::new(Expression::Variable("current".into())),
            offset: 56,
            member_type: Type::StructPointer { element_size: 0 },
            index_stride: None,
        }
    }

    fn call(arguments: Vec<Expression>) -> Statement {
        Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments,
        })
    }

    #[test]
    fn recognizes_a_condition_member_consumed_by_both_arm_entries() {
        let condition = Expression::Binary {
            operator: BinaryOperator::LogicalAnd,
            left: Box::new(Expression::Binary {
                operator: BinaryOperator::Equal,
                left: Box::new(member()),
                right: Box::new(Expression::IntegerLiteral(0)),
            }),
            right: Box::new(Expression::Variable("ready".into())),
        };
        let then_body = [call(vec![
            Expression::Variable("current".into()),
            Expression::Variable("prior".into()),
        ])];
        let else_body = [call(vec![Expression::Variable("current".into()), member()])];
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        let plan = plan(
            &condition,
            &then_body,
            &else_body,
            &globals,
            &std::collections::HashSet::new(),
        )
        .expect("condition values should feed both arm entries");
        assert_eq!(plan.global.as_deref(), Some("current"));
    }

    #[test]
    fn recognizes_a_condition_member_as_the_null_first_call_second_argument() {
        let condition = Expression::Binary {
            operator: BinaryOperator::NotEqual,
            left: Box::new(member()),
            right: Box::new(Expression::IntegerLiteral(0)),
        };
        let then_body = [call(vec![
            Expression::IntegerLiteral(0),
            Expression::Variable("prior".into()),
        ])];
        let else_body = [call(vec![Expression::IntegerLiteral(0), member()])];
        let globals = std::collections::HashMap::from([(
            "current".into(),
            Type::StructPointer { element_size: 64 },
        )]);

        let plan = plan(
            &condition,
            &then_body,
            &else_body,
            &globals,
            &std::collections::HashSet::new(),
        )
        .expect("condition member should feed the else call");
        assert_eq!(plan.global, None);
    }
}
