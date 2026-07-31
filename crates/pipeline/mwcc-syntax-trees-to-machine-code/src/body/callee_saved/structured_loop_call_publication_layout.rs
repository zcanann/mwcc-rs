//! Saved-GPR roles for a call-result publication loop followed by a call loop.
//!
//! This O0 source shape keeps an incoming owner, an array index, the published
//! call result, and a later call-loop bound in four distinct saved homes. The
//! roles, rather than declaration order, determine MWCC's register order.

use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
#[allow(unused_imports)]
use super::*;
use mwcc_syntax_trees::Parameter;

pub(super) struct StructuredLoopCallPublicationLayout {
    preference_by_home: [u8; 4],
    save_order: [usize; 4],
}

impl StructuredLoopCallPublicationLayout {
    pub(super) fn plan(
        function: &Function,
        eager_locals: &[&LocalDeclaration],
        saved_parameters: &[&Parameter],
        deferred_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
        parameter_reuse: &StructuredParameterHomeReuse,
        home_count: usize,
    ) -> Option<Self> {
        let [_owner] = saved_parameters else {
            return None;
        };
        if !eager_locals.is_empty()
            || deferred_locals.len() != 3
            || deferred.group_count != 3
            || parameter_reuse.fresh_group_count != 3
            || home_count != 4
        {
            return None;
        }

        let mut publication_roles = None;
        for statement in &function.statements {
            let Statement::Loop { body, .. } = statement else {
                continue;
            };
            let [publication, use_result, ..] = body.as_slice() else {
                continue;
            };
            let Statement::Store {
                target: Expression::Index { base: array, index },
                value:
                    Expression::Assign {
                        target: assigned,
                        value: produced,
                    },
            } = publication
            else {
                continue;
            };
            let (
                Expression::Variable(_array),
                Expression::Variable(counter),
                Expression::Variable(result),
                Expression::Call { .. },
            ) = (
                array.as_ref(),
                index.as_ref(),
                assigned.as_ref(),
                produced.as_ref(),
            )
            else {
                continue;
            };
            if !matches!(
                use_result,
                Statement::Store {
                    target: Expression::Index { base, .. },
                    value: Expression::Variable(value),
                } if value == counter
                    && matches!(base.as_ref(), Expression::MemberAddress { base, .. }
                        if matches!(base.as_ref(), Expression::Variable(name) if name == result))
            ) {
                continue;
            }
            if publication_roles.replace((counter, result)).is_some() {
                return None;
            }
        }
        let (counter, result) = publication_roles?;
        let tail = deferred_locals
            .iter()
            .find(|local| local.name != *counter && local.name != *result)?;
        if !deferred_locals.iter().any(|local| local.name == *counter)
            || !deferred_locals.iter().any(|local| local.name == *result)
        {
            return None;
        }

        let home = |name: &str| {
            deferred
                .group_if_present(name)
                .map(|group| parameter_reuse.home_index(group))
        };
        let counter_home = home(counter)?;
        let result_home = home(result)?;
        let tail_home = home(&tail.name)?;
        let mut preference_by_home = [0; 4];
        let mut occupied = [false; 4];
        let mut set = |home: usize, preference: u8| {
            if home >= preference_by_home.len() || occupied[home] {
                return false;
            }
            occupied[home] = true;
            preference_by_home[home] = preference;
            true
        };
        if !set(0, 29)
            || !set(counter_home, 31)
            || !set(result_home, 30)
            || !set(tail_home, 28)
            || occupied.iter().any(|occupied| !occupied)
        {
            return None;
        }
        Some(Self {
            preference_by_home,
            save_order: [counter_home, result_home, 0, tail_home],
        })
    }

    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        self.preference_by_home.get(home_index).copied()
    }

    pub(super) fn save_order(&self) -> [usize; 4] {
        self.save_order
    }

    pub(super) fn frame_slot(&self, home_index: usize) -> Option<usize> {
        self.save_order
            .iter()
            .position(|candidate| *candidate == home_index)
    }
}
