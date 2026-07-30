//! Saved-home layout for an interrupt-token state switch with an indirect callback.
//!
//! This family has three overlapping values: the callback, the state receiver,
//! and the entry call's token. MWCC colors by lifetime role rather than source
//! or discovery order, placing the token highest and the receiver lowest.

use super::structured_expression_visit::visit_statement;
use super::structured_locals::DeferredSavedHomePlan;
use super::structured_parameter_home_reuse::StructuredParameterHomeReuse;
use mwcc_syntax_trees::{Expression, Function, LocalDeclaration, Parameter, Statement};

pub(super) struct StructuredAsyncCallbackSwitchLayout {
    callback_home: usize,
    receiver_home: usize,
    token_home: usize,
}

#[derive(Clone, Copy)]
pub(super) struct StructuredAsyncCallbackSwitchHomes {
    pub(super) callback: u8,
    pub(super) receiver: u8,
    pub(super) token: u8,
}

impl StructuredAsyncCallbackSwitchLayout {
    pub(super) fn plan(
        function: &Function,
        with_frame_array: bool,
        eager_count: usize,
        saved_parameters: &[&Parameter],
        deferred_saved_locals: &[&LocalDeclaration],
        deferred: &DeferredSavedHomePlan,
        reuse: &StructuredParameterHomeReuse,
        total_count: usize,
    ) -> Option<Self> {
        if with_frame_array
            || eager_count != 0
            || saved_parameters.len() != 2
            || deferred_saved_locals.len() != 1
            || deferred.group_count != 1
            || reuse.fresh_group_count != 1
            || total_count != 3
        {
            return None;
        }
        let Statement::Assign {
            name: token,
            value: Expression::Call { arguments, .. },
        } = function.statements.first()?
        else {
            return None;
        };
        if !arguments.is_empty() || token != &deferred_saved_locals[0].name {
            return None;
        }
        let switch = function
            .statements
            .iter()
            .find(|statement| matches!(statement, Statement::Switch { .. }))?;
        let Statement::Switch { scrutinee, .. } = switch else {
            unreachable!("the selected statement is a switch");
        };

        let callback_indexes: Vec<_> = saved_parameters
            .iter()
            .enumerate()
            .filter_map(|(index, parameter)| {
                statement_calls_name(switch, &parameter.name).then_some(index)
            })
            .collect();
        let [callback_index] = callback_indexes.as_slice() else {
            return None;
        };
        let receiver_index = 1usize.checked_sub(*callback_index)?;
        if crate::analysis::count_name_occurrences(
            scrutinee,
            &saved_parameters[receiver_index].name,
        ) == 0
        {
            return None;
        }
        let token_home = reuse.home_index(deferred.group(token));
        if token_home >= total_count {
            return None;
        }

        Some(Self {
            callback_home: *callback_index,
            receiver_home: receiver_index,
            token_home,
        })
    }

    pub(super) fn preference(&self, home_index: usize) -> Option<u8> {
        if home_index == self.callback_home {
            Some(30)
        } else if home_index == self.receiver_home {
            Some(29)
        } else if home_index == self.token_home {
            Some(31)
        } else {
            None
        }
    }

    pub(super) fn homes(&self, homes: &[u8]) -> StructuredAsyncCallbackSwitchHomes {
        StructuredAsyncCallbackSwitchHomes {
            callback: homes[self.callback_home],
            receiver: homes[self.receiver_home],
            token: homes[self.token_home],
        }
    }

    pub(super) fn save_order(&self, homes: &[u8]) -> [u8; 3] {
        let homes = self.homes(homes);
        [homes.token, homes.callback, homes.receiver]
    }
}

fn statement_calls_name(statement: &Statement, candidate: &str) -> bool {
    let mut found = false;
    visit_statement(statement, &mut |expression| {
        if matches!(expression, Expression::Call { name, .. } if name == candidate) {
            found = true;
        }
    });
    found
}
